use super::{
    error::{Error, Result},
    hdlc::{self, Decoder},
    wire,
};
use std::{
    collections::{BTreeSet, VecDeque},
    io::{Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

pub trait Transport: Read + Write + Send {}
impl<T: Read + Write + Send> Transport for T {}

/// One owner, one in-flight request, never retries an ambiguous mutation.
pub struct Session<T: Transport> {
    io: T,
    decoder: Decoder,
    pending: VecDeque<Result<Vec<u8>>>,
    cancel: Arc<AtomicBool>,
    timeout: Duration,
    files: BTreeSet<u32>,
    dirs: BTreeSet<u32>,
    poisoned: bool,
    cleaning: bool,
    sync_sequence: u16,
}
impl<T: Transport> Session<T> {
    pub fn new(io: T, cancel: Arc<AtomicBool>, timeout: Duration) -> Self {
        Self {
            io,
            decoder: Decoder::default(),
            pending: VecDeque::new(),
            cancel,
            timeout,
            files: BTreeSet::new(),
            dirs: BTreeSet::new(),
            poisoned: false,
            cleaning: false,
            sync_sequence: 0,
        }
    }
    pub fn check_cancel(&self) -> Result<()> {
        if !self.cleaning && self.cancel.load(Ordering::Acquire) {
            Err(Error::new(
                "cancelled",
                "session",
                "Operation cancelled; device may be partially changed",
            ))
        } else {
            Ok(())
        }
    }
    pub fn next_sync_sequence(&mut self) -> u16 {
        self.sync_sequence = self.sync_sequence.wrapping_add(1);
        self.sync_sequence
    }
    fn check(&self, deadline: Instant) -> Result<()> {
        self.check_cancel()?;
        if Instant::now() >= deadline {
            return Err(Error::new(
                "timeout",
                "session",
                "DIAG deadline exceeded; outcome may be unknown",
            ));
        }
        Ok(())
    }
    pub fn request(&mut self, req: &[u8]) -> Result<Vec<u8>> {
        if self.poisoned && !self.cleaning {
            return Err(Error::new(
                "poisoned",
                "session",
                "Session cannot continue after transport failure",
            ));
        }
        let result = self.exchange(req);
        if let Err(e) = &result {
            if matches!(
                e.code.as_str(),
                "timeout" | "io" | "crc" | "framing" | "oversized" | "malformed" | "cancelled"
            ) {
                self.poisoned = true;
            }
        }
        result
    }
    fn exchange(&mut self, req: &[u8]) -> Result<Vec<u8>> {
        let deadline = Instant::now()
            + if self.cleaning {
                Duration::from_millis(500)
            } else {
                self.timeout
            };
        let encoded = hdlc::encode(req, false);
        let mut sent = 0;
        while sent < encoded.len() {
            self.check(deadline)?;
            match self.io.write(&encoded[sent..]) {
                Ok(0) => {
                    return Err(Error::new(
                        "io",
                        "send",
                        "Transport disconnected or made no write progress",
                    ))
                }
                Ok(n) => sent += n,
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::TimedOut
                            | std::io::ErrorKind::WouldBlock
                            | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(e) => return Err(Error::io("send", e)),
            }
        }
        loop {
            self.check(deadline)?;
            while let Some(frame) = self.pending.pop_front() {
                self.check(deadline)?;
                let frame = frame?;
                // Error packets echo rejected request. Unrelated errors cannot reject ours.
                if matches!(frame[0], 0x13..=0x18 | 0x42 | 0x47) {
                    if frame
                        .get(1..)
                        .is_some_and(|echo| !echo.is_empty() && req.starts_with(echo))
                    {
                        return Err(Error::new(
                            if frame[0] == 0x13 {
                                "unsupported"
                            } else {
                                "diagError"
                            },
                            "DIAG",
                            format!("Rejected command (0x{:02x})", frame[0]),
                        ));
                    }
                    continue;
                }
                if Self::matches(req, &frame)? {
                    return Ok(frame);
                }
                // Unsolicited logs and replies to other commands are consumed, never queued indefinitely.
            }
            let mut bytes = [0u8; 2048];
            match self.io.read(&mut bytes) {
                Ok(0) => return Err(Error::new("io", "receive", "Transport disconnected")),
                Ok(n) => self.pending.extend(self.decoder.feed(&bytes[..n])),
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::TimedOut
                            | std::io::ErrorKind::WouldBlock
                            | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(e) => return Err(Error::io("receive", e)),
            }
        }
    }
    fn matches(req: &[u8], response: &[u8]) -> Result<bool> {
        if req[0] != response[0] {
            return Ok(false);
        }
        if req[0] == 0x4b {
            if response.len() < 4 {
                return Err(Error::new(
                    "malformed",
                    "match",
                    "Truncated subsystem reply",
                ));
            }
            if req[..4] != response[..4] {
                return Ok(false);
            }
            match wire::u16_at(req, 2)? {
                4 | 5 => {
                    return Ok(wire::u32_at(req, 4)? == wire::u32_at(response, 4)?
                        && wire::u32_at(req, if req[2] == 4 { 12 } else { 8 })?
                            == wire::u32_at(response, 8)?);
                }
                12 => {
                    return Ok(req[4..12]
                        == *response.get(4..12).ok_or_else(|| {
                            Error::new("malformed", "match", "Truncated directory reply")
                        })?);
                }
                48 => {
                    return Ok(wire::u16_at(req, 4)? == wire::u16_at(response, 4)?);
                }
                _ => {}
            }
        } else if matches!(req[0], 0x26 | 0x27) {
            return Ok(wire::u16_at(req, 1)? == wire::u16_at(response, 1)?);
        } else if req[0] == 0x73 {
            if wire::u32_at(req, 4)? != wire::u32_at(response, 4)? {
                return Ok(false);
            }
            if wire::u32_at(req, 4)? == 3 {
                return Ok(req[8] as u32 == wire::u32_at(response, 12)?);
            }
            return Ok(true);
        } else if req[0] == 0x7d {
            if req.get(1) != response.get(1) {
                return Ok(false);
            }
            if req.get(1) == Some(&4) {
                return Ok(wire::u16_at(req, 2)? == wire::u16_at(response, 2)?
                    && wire::u16_at(req, 4)? == wire::u16_at(response, 4)?);
            }
        }
        Ok(true)
    }
    pub fn initialize(&mut self) -> Result<Vec<String>> {
        let mut warnings = vec![];
        for req in [
            vec![0x46, b'F', b'F', b'F', b'F', b'F', b'F', b'F', b'F'],
            vec![0x41, b'0', b'0', b'0', b'0', b'0', b'0'],
        ] {
            match self.request(&req) {
                Ok(r) if r.get(1) == Some(&1) => {}
                Ok(_) => {
                    return Err(Error::new(
                        "authentication",
                        "setup",
                        "Password/SPC rejected or malformed",
                    ))
                }
                Err(e) if e.code == "unsupported" => {
                    warnings.push(format!("Setup command 0x{:02x} unsupported", req[0]))
                }
                Err(e) => return Err(e),
            }
        }
        // Preserve upstream ranges -> zero masks ordering. Only explicit BAD_CMD is optional.
        for messages in [false, true] {
            match self.suppress(messages) {
                Ok(()) => {}
                Err(e) if e.code == "unsupported" => warnings.push(format!(
                    "{} suppression unsupported",
                    if messages { "Message" } else { "Log" }
                )),
                Err(e) => return Err(e),
            }
        }
        let hello = self.request(&wire::hello())?;
        if hello.len() != 44
            || wire::u32_at(&hello, 28)? != 1
            || wire::u32_at(&hello, 32)? > 1
            || wire::u32_at(&hello, 36)? < 1
        {
            return Err(Error::new(
                "incompatible",
                "hello",
                "Unsupported EFS protocol negotiation",
            ));
        }
        Ok(warnings)
    }
    fn suppress(&mut self, messages: bool) -> Result<()> {
        if messages {
            let r = self.request(&[0x7d, 1])?;
            if r.len() < 8 || (r.len() - 8) % 4 != 0 || r.len() > 264 {
                return Err(Error::new(
                    "malformed",
                    "message ranges",
                    "Invalid range list",
                ));
            }
            for pair in r[8..].as_chunks::<4>().0 {
                let req = wire::message_mask(wire::u16_at(pair, 0)?, wire::u16_at(pair, 2)?)?;
                let reply = self.request(&req)?;
                if reply.get(6) != Some(&1) {
                    return Err(Error::new("deviceStatus", "message mask", "Mask rejected"));
                }
                let count = (wire::u16_at(&req, 4)? - wire::u16_at(&req, 2)? + 1) as usize * 4;
                if reply.len() < 8 + count || reply[8..8 + count].iter().any(|b| *b != 0) {
                    return Err(Error::new(
                        "malformed",
                        "message mask",
                        "Mask was not disabled",
                    ));
                }
            }
        } else {
            let r = self.request(&wire::log_ranges())?;
            wire::status(&r, 8, "log ranges")?;
            if r.len() < 14 || (r.len() - 14) % 4 != 0 || r.len() > 78 {
                return Err(Error::new("malformed", "log ranges", "Invalid range list"));
            }
            for (i, pair) in r[14..].as_chunks::<4>().0.iter().enumerate() {
                let req = wire::log_mask(
                    (i + 1) as u32,
                    wire::u16_at(pair, 0)? as u32,
                    wire::u16_at(pair, 2)? as u32 + 0x1000,
                )?;
                let reply = self.request(&req)?;
                wire::status(&reply, 8, "log mask")?;
                if wire::u32_at(&reply, 12)? != (i + 1) as u32 {
                    return Err(Error::new("malformed", "log mask", "Wrong log scope"));
                }
                let bits = wire::u32_at(&reply, 16)? as usize;
                let count = bits.div_ceil(8);
                if count > 2048
                    || reply.len() < 20 + count
                    || reply[20..20 + count].iter().any(|b| *b != 0)
                {
                    return Err(Error::new("malformed", "log mask", "Mask was not disabled"));
                }
            }
        }
        Ok(())
    }
    pub fn open(&mut self, path: &str, flags: u32, mode: u32) -> Result<u32> {
        let r = self.request(&wire::open(path, flags, mode)?)?;
        wire::status(&r, 8, "open")?;
        let fd = wire::u32_at(&r, 4)?;
        if (fd as i32) < 0 {
            return Err(Error::new("malformed", "open", "Negative descriptor"));
        }
        self.files.insert(fd);
        Ok(fd)
    }
    pub fn opendir(&mut self, path: &str) -> Result<u32> {
        let r = self.request(&wire::path_request(
            11,
            if path == "/" { "." } else { path },
        )?)?;
        wire::status(&r, 8, "opendir")?;
        let fd = wire::u32_at(&r, 4)?;
        if (fd as i32) < 0 {
            return Err(Error::new("malformed", "opendir", "Negative descriptor"));
        }
        self.dirs.insert(fd);
        Ok(fd)
    }
    pub fn close(&mut self, fd: u32, directory: bool) -> Result<()> {
        let r = self.request(&wire::words(if directory { 13 } else { 3 }, &[fd]))?;
        wire::status(&r, 4, "close")?;
        if directory {
            self.dirs.remove(&fd);
        } else {
            self.files.remove(&fd);
        }
        Ok(())
    }
    pub fn cleanup(&mut self) -> Vec<String> {
        self.cleaning = true;
        let mut errors = vec![];
        for (fd, dir) in self
            .files
            .iter()
            .map(|f| (*f, false))
            .chain(self.dirs.iter().map(|f| (*f, true)))
            .collect::<Vec<_>>()
        {
            if let Err(e) = self.close(fd, dir) {
                errors.push(e.to_string());
            }
        }
        self.files.clear();
        self.dirs.clear();
        self.cleaning = false;
        errors
    }
}
impl<T: Transport> Drop for Session<T> {
    fn drop(&mut self) {
        if !self.files.is_empty() || !self.dirs.is_empty() {
            let _ = self.cleanup();
        }
    }
}
