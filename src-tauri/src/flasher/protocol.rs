//! S1 subset ported from get_reply/process_sins; fastboot is a separate protocol.
use super::{error, policy, transport::FlashTransport, Result};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    time::{Duration, Instant},
};

const RESPONSE_LIMIT: usize = 4096;
const MAX_INFO: usize = 4096;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(120);
const PAYLOAD_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Debug, PartialEq, Eq)]
pub enum Reply {
    Okay(Vec<u8>),
    Data(u32),
}
#[derive(Clone, Copy, Debug)]
pub enum SignatureMode {
    Legacy,
    DownloadThenSignature,
}

pub struct S1<T> {
    transport: T,
    pub max_download: u32,
    signature_mode: SignatureMode,
}
impl<T: FlashTransport> S1<T> {
    pub fn new(transport: T, max_download: u32, signature_mode: SignatureMode) -> Result<Self> {
        if max_download == 0 || max_download as u64 > super::sin::MAX_MEMBER {
            return Err(error("DOWNLOAD_LIMIT", "Unsupported device download limit"));
        }
        Ok(Self {
            transport,
            max_download,
            signature_mode,
        })
    }
    #[cfg(test)]
    pub fn into_transport(self) -> T {
        self.transport
    }

    fn remaining(deadline: Instant) -> Result<Duration> {
        deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or_else(|| error("TIMEOUT", "S1 operation deadline reached"))
    }
    fn write_all(&mut self, mut bytes: &[u8], deadline: Instant) -> Result<()> {
        while !bytes.is_empty() {
            let n = self.transport.write(bytes, Self::remaining(deadline)?)?;
            if n == 0 || n > bytes.len() {
                return Err(error("SHORT_WRITE", "Invalid S1 write length"));
            }
            bytes = &bytes[n..];
        }
        Ok(())
    }
    fn reply(&mut self, deadline: Instant) -> Result<Reply> {
        for _ in 0..MAX_INFO {
            let response = self
                .transport
                .read_response(RESPONSE_LIMIT, Self::remaining(deadline)?)?;
            if response.len() < 4 || response.len() > RESPONSE_LIMIT {
                return Err(error("RESPONSE", "Invalid S1 response length"));
            }
            match &response[..4] {
                b"OKAY" => return Ok(Reply::Okay(response[4..].to_vec())),
                // Device messages can contain identifiers; return a stable code instead of raw reply bytes.
                b"FAIL" => return Err(error("DEVICE_FAIL", "Device rejected S1 command")),
                b"INFO" => continue,
                b"DATA" => {
                    // Upstream tolerates XQ-BT41's terminating NUL. Do not trim arbitrary payload.
                    let size = if response.len() == 13 && response[12] == 0 {
                        &response[4..12]
                    } else if response.len() == 12 {
                        &response[4..]
                    } else {
                        return Err(error("DATA", "Malformed S1 DATA reply"));
                    };
                    if !size.iter().all(u8::is_ascii_hexdigit) {
                        return Err(error("DATA", "Invalid S1 DATA length"));
                    }
                    let length = u32::from_str_radix(std::str::from_utf8(size).unwrap(), 16)
                        .map_err(|_| error("DATA", "Invalid S1 DATA length"))?;
                    return Ok(Reply::Data(length));
                }
                _ => return Err(error("RESPONSE", "Unknown S1 response kind")),
            }
        }
        Err(error("INFO_LIMIT", "Too many S1 INFO responses"))
    }
    pub fn command(&mut self, command: &str) -> Result<Reply> {
        if command.is_empty()
            || command.len() > 96
            || !command.is_ascii()
            || command.bytes().any(|b| b.is_ascii_control())
        {
            return Err(error("COMMAND", "Invalid S1 command"));
        }
        let deadline = Instant::now() + COMMAND_TIMEOUT;
        self.write_all(command.as_bytes(), deadline)?;
        self.reply(deadline)
    }
    pub fn okay(&mut self, command: &str) -> Result<()> {
        match self.command(command)? {
            Reply::Okay(_) => Ok(()),
            _ => Err(error("ACK", "Expected S1 OKAY")),
        }
    }
    pub fn payload(
        &mut self,
        bytes: u64,
        source: &mut dyn Read,
        expected_sha256: &str,
        signature: bool,
    ) -> Result<()> {
        if bytes == 0 || bytes > self.max_download as u64 {
            return Err(error(
                "DOWNLOAD_LIMIT",
                "Payload exceeds negotiated download limit",
            ));
        }
        if expected_sha256.len() != 64 || !expected_sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(error("HASH", "Invalid expected payload hash"));
        }
        let prefix = if signature && matches!(self.signature_mode, SignatureMode::Legacy) {
            "signature"
        } else {
            "download"
        };
        if self.command(&format!("{prefix}:{bytes:08x}"))? != Reply::Data(bytes as u32) {
            return Err(error("DATA", "Device download length mismatch"));
        }
        let deadline = Instant::now() + PAYLOAD_TIMEOUT;
        let mut remaining = bytes;
        let mut hash = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        while remaining > 0 {
            let count = remaining.min(buffer.len() as u64) as usize;
            source
                .read_exact(&mut buffer[..count])
                .map_err(|_| error("PAYLOAD", "Truncated SIN payload"))?;
            hash.update(&buffer[..count]);
            self.write_all(&buffer[..count], deadline)?;
            remaining -= count as u64;
        }
        if hex::encode(hash.finalize()) != expected_sha256.to_ascii_lowercase() {
            return Err(error("CHANGED", "SIN payload changed before completion"));
        }
        if !matches!(self.reply(deadline)?, Reply::Okay(_)) {
            return Err(error("ACK", "Missing payload acknowledgement"));
        }
        if signature && matches!(self.signature_mode, SignatureMode::DownloadThenSignature) {
            self.okay("signature")?;
        }
        Ok(())
    }
    pub fn erase(&mut self, target: &str) -> Result<()> {
        policy::partition_name(target)?;
        self.okay(&format!("erase:{target}"))
    }
    pub fn flash(&mut self, target: &str) -> Result<()> {
        policy::partition_name(target)?;
        self.okay(&format!("flash:{target}"))
    }
    pub fn session_flag(&mut self, active: bool) -> Result<()> {
        let value = [u8::from(active)];
        self.payload(
            1,
            &mut &value[..],
            &hex::encode(Sha256::digest(value)),
            false,
        )?;
        self.okay("Write-TA:2:10100")
    }
}
