use std::time::Duration;

use crate::{
    AdbStatResponse, BinaryDecodable, Result, RustADBError,
    message_devices::{
        adb_message_transport::ADBMessageTransport,
        adb_transport_message::ADBTransportMessage,
        message_commands::{MessageCommand, MessageSubcommand},
        utils::BinaryEncodable,
    },
};

const BUFFER_SIZE: usize = 65535;

/// Bounded developer diagnostics; contains no paths or file contents.
pub(crate) fn trace_sync(message: &str) {
    static ROWS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    if cfg!(debug_assertions)
        && std::env::var_os("XVA_ADB_TRACE").is_some()
        && ROWS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 10000
    {
        eprintln!("[xva sync] {message}");
    }
}

#[cfg(test)]
mod stream_tests {
    use super::*;
    use crate::adb_transport::ADBTransport;
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex},
    };
    #[derive(Clone)]
    struct Transport {
        incoming: Arc<Mutex<VecDeque<ADBTransportMessage>>>,
        replies: Arc<Mutex<Vec<ADBTransportMessage>>>,
    }
    impl ADBTransport for Transport {
        fn connect(&mut self) -> Result<()> {
            Ok(())
        }
        fn disconnect(&mut self) -> Result<()> {
            Ok(())
        }
    }
    impl ADBMessageTransport for Transport {
        fn read_message_with_timeout(&mut self, _: Duration) -> Result<ADBTransportMessage> {
            self.incoming.lock().unwrap().pop_front().ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "test EOF").into()
            })
        }
        fn write_message_with_timeout(
            &mut self,
            message: ADBTransportMessage,
            _: Duration,
        ) -> Result<()> {
            self.replies.lock().unwrap().push(message);
            Ok(())
        }
    }
    fn session(packets: &[(MessageCommand, &[u8])]) -> (ADBSession<Transport>, Transport) {
        let transport = Transport {
            incoming: Arc::new(Mutex::new(
                packets
                    .iter()
                    .map(|(cmd, payload)| {
                        ADBTransportMessage::try_new(*cmd, 2, 1, payload).unwrap()
                    })
                    .collect(),
            )),
            replies: Arc::new(Mutex::new(Vec::new())),
        };
        (ADBSession::new(transport.clone(), 1, 2), transport)
    }
    #[test]
    fn data_and_close_use_their_own_acknowledgements_and_stream_ids() {
        let (mut s, t) = session(&[
            (MessageCommand::Write, b"body"),
            (MessageCommand::Okay, b""),
            (MessageCommand::Clse, b""),
        ]);
        assert_eq!(s.recv_and_reply_okay().unwrap().payload(), b"body");
        s.recv_and_reply_okay().unwrap();
        s.recv_and_reply_okay().unwrap();
        let replies = t.replies.lock().unwrap();
        assert_eq!(
            replies
                .iter()
                .map(|m| m.header().command())
                .collect::<Vec<_>>(),
            vec![MessageCommand::Okay, MessageCommand::Clse]
        );
        assert!(
            replies
                .iter()
                .all(|m| m.header().arg0() == 1 && m.header().arg1() == 2)
        );
    }

    #[test]
    fn stat_acknowledges_response_and_rejects_short_or_wrong_payload() {
        let mut valid = b"STAT".to_vec();
        for field in [0o100644u32, 123, 456] {
            valid.extend_from_slice(&field.to_le_bytes());
        }
        for payload in [valid.as_slice(), b"STA", b"FAILdenied", b"STAT"] {
            let (mut s, t) = session(&[
                (MessageCommand::Okay, b""),
                (MessageCommand::Okay, b""),
                (MessageCommand::Write, payload),
            ]);
            let result = s.stat_with_explicit_ids("/sdcard/a (1).zip");
            if payload == valid {
                let stat = result.unwrap();
                assert_eq!(stat.file_size, 123);
                assert_eq!(stat.mod_time, 456);
            } else {
                assert!(result.is_err());
            }
            assert_eq!(
                t.replies.lock().unwrap().last().unwrap().header().command(),
                MessageCommand::Okay
            );
        }
    }
    #[test]
    fn push_acknowledges_final_status_and_rejects_sync_failure() {
        for (status, success) in [
            (b"OKAY\0\0\0\0".as_slice(), true),
            (b"FAIL\0\0\0\0".as_slice(), false),
        ] {
            let (mut s, t) = session(&[
                (MessageCommand::Okay, b""),
                (MessageCommand::Okay, b""),
                (MessageCommand::Write, status),
            ]);
            assert_eq!(
                s.push_file(std::io::Cursor::new(b"helper")).is_ok(),
                success
            );
            assert_eq!(
                t.replies.lock().unwrap().last().unwrap().header().command(),
                MessageCommand::Okay
            );
            assert!(t.incoming.lock().unwrap().is_empty());
        }
    }

    #[test]
    fn receive_parses_split_headers_and_binary_data_without_false_done() {
        let body = b"binary\0DATA\0DONE\0\0\0\0";
        let mut wire = b"DATA".to_vec();
        wire.extend_from_slice(&(body.len() as u32).to_le_bytes());
        wire.extend_from_slice(body);
        wire.extend_from_slice(b"DATA\0\0\0\0DONE\0\0\0\0");
        for chunk_size in [1, 3, 7, 8, 17, wire.len()] {
            let chunks: Vec<_> = wire
                .chunks(chunk_size)
                .map(|chunk| (MessageCommand::Write, chunk))
                .collect();
            let (mut s, t) = session(&chunks);
            let mut output = Vec::new();
            s.recv_file(&mut output).unwrap();
            assert_eq!(output, body);
            assert_eq!(t.replies.lock().unwrap().len(), chunks.len());
            assert!(t.incoming.lock().unwrap().is_empty());
        }
    }

    #[test]
    fn receive_handles_empty_files_and_rejects_truncation_failure_and_no_progress() {
        let (mut empty, _) = session(&[(MessageCommand::Write, b"DONE\0\0\0\0")]);
        assert!(empty.recv_file(Vec::new()).is_ok());
        for payload in [
            b"DON".as_slice(),
            b"DATA\x04\0\0\0x",
            b"FAIL\x06\0\0\0denied",
            b"FAIL\xff\xff\xff\xff",
            b"BAD!\0\0\0\0",
        ] {
            let (mut s, _) = session(&[(MessageCommand::Write, payload)]);
            assert!(s.recv_file(Vec::new()).is_err(), "{payload:?}");
        }
        let (mut s, _) = session(&[
            (MessageCommand::Write, b""),
            (MessageCommand::Write, b"DO"),
            (MessageCommand::Write, b"NE\0\0\0\0"),
        ]);
        assert!(s.recv_file(Vec::new()).is_ok());
        let (mut s, _) = session(&[(MessageCommand::Write, b"".as_slice()); 5]);
        assert!(s.recv_file(Vec::new()).is_err());
        let (mut s, _) = session(&[(MessageCommand::Clse, b"")]);
        assert!(s.recv_file(Vec::new()).is_err());
    }
    #[test]
    fn abort_consumes_pending_frames_has_a_limit_and_peer_close_needs_no_quit() {
        let (mut s,t)=session(&[(MessageCommand::Write,b"pending"),(MessageCommand::Clse,b"")]);
        s.mark_incomplete(true);s.close_sync().unwrap();
        let commands:Vec<_>=t.replies.lock().unwrap().iter().map(|m|m.header().command()).collect();
        assert_eq!(commands,vec![MessageCommand::Clse,MessageCommand::Okay]);
        let (mut s,t)=session(&[(MessageCommand::Clse,b"")]);
        s.recv_and_reply_okay().unwrap();let sent=t.replies.lock().unwrap().len();
        s.close_sync().unwrap();assert_eq!(t.replies.lock().unwrap().len(),sent);
        let (mut s,t)=session(&vec![(MessageCommand::Okay,b"".as_slice());1025]);
        s.mark_incomplete(true);assert!(s.close_sync().unwrap_err().to_string().contains("limit"));
        assert_eq!(t.incoming.lock().unwrap().len(),1);
    }

}

/// Represent a session between an `ADBDevice` and remote `adbd`.
#[derive(Debug)]
pub struct ADBSession<T: ADBMessageTransport> {
    transport: T,
    local_id: u32,
    remote_id: u32,
    peer_closed: bool,
    recv_incomplete: bool,
}

impl<T: ADBMessageTransport> ADBSession<T> {
    /// Create a new session with the given transport and IDs.
    pub const fn new(transport: T, local_id: u32, remote_id: u32) -> Self {
        Self {
            transport,
            local_id,
            remote_id,
            peer_closed: false,
            recv_incomplete: false,
        }
    }

    pub const fn get_transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    pub const fn local_id(&self) -> u32 {
        self.local_id
    }

    pub const fn remote_id(&self) -> u32 {
        self.remote_id
    }

    pub(crate) fn mark_incomplete(&mut self, incomplete:bool) {self.recv_incomplete=incomplete;}

    /// Receive a message and acknowledge it by replying with an `OKAY` command
    pub(crate) fn recv_and_reply_okay(&mut self) -> Result<ADBTransportMessage> {
        let message = self.transport.read_message()?;
        if message.header().arg1() != self.local_id || message.header().arg0() != self.remote_id {
            return Err(RustADBError::ADBRequestFailed(
                "Response used different stream IDs".into(),
            ));
        }
        if message.header().command() == MessageCommand::Clse {
            self.peer_closed = true;
        }
        let reply = match message.header().command() {
            MessageCommand::Write => Some(MessageCommand::Okay),
            MessageCommand::Clse => Some(MessageCommand::Clse),
            MessageCommand::Okay => None,
            other => {
                return Err(RustADBError::ADBRequestFailed(format!(
                    "Unexpected stream response: {other}"
                )));
            }
        };
        if let Some(reply) = reply {
            self.transport.write_message(ADBTransportMessage::try_new(
                reply,
                self.local_id,
                self.remote_id,
                &[],
            )?)?;
        }
        Ok(message)
    }

    /// Expect a message with an `OKAY` command after sending a message.
    pub(crate) fn send_and_expect_okay(
        &mut self,
        message: ADBTransportMessage,
    ) -> Result<ADBTransportMessage> {
        self.transport.write_message(message)?;

        self.recv_and_reply_okay().and_then(|message| {
            message.assert_command(MessageCommand::Okay)?;
            Ok(message)
        })
    }

    pub(crate) fn recv_file<W: std::io::Write>(
        &mut self,
        mut output: W,
    ) -> std::result::Result<(), RustADBError> {
        let mut payload = Vec::new();
        let mut position = 0;
        self.recv_incomplete = true;
        loop {
            let mut header = [0; 8];
            self.read_sync_exact(&mut payload, &mut position, &mut header)?;
            let size = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
            match &header[..4] {
                b"DATA" => {
                    let mut remaining = size;
                    while remaining > 0 {
                        self.ensure_sync_payload(&mut payload, &mut position)?;
                        let count = remaining.min(payload.len() - position);
                        // Leave recv_incomplete set on a PC write failure. The caller
                        // closes the ADB stream and drains frames with a bounded timeout;
                        // another SYNC request must never consume this file's remaining DATA.
                        output.write_all(&payload[position..position + count])?;
                        position += count;
                        remaining -= count;
                    }
                }
                b"DONE" if position == payload.len() => {
                    self.recv_incomplete = false;
                    return Ok(());
                }
                b"FAIL" if size <= 65536 => {
                    let mut reason = vec![0; size];
                    self.read_sync_exact(&mut payload, &mut position, &mut reason)?;
                    self.recv_incomplete = false;
                    return Err(RustADBError::ADBRequestFailed(format!(
                        "SYNC RECV failed: {}",
                        String::from_utf8_lossy(&reason)
                    )));
                }
                _ => {
                    return Err(RustADBError::ADBRequestFailed(
                        "Invalid SYNC RECV response".into(),
                    ));
                }
            }
        }
    }

    /// Control-plane cleanup never uses the transport's 300-second data timeout.
    /// If RECV was interrupted, close the ADB stream instead of writing SYNC QUIT
    /// into a stream whose DATA packets are still in flight.
    pub(crate) fn close_sync(&mut self) -> Result<()> {
        if self.peer_closed {
            return Ok(());
        }
        let started = std::time::Instant::now();
        let command = if self.recv_incomplete {
            MessageCommand::Clse
        } else {
            MessageCommand::Write
        };
        let payload = if self.recv_incomplete {
            Vec::new()
        } else {
            MessageSubcommand::Quit.with_arg(0u32).encode()
        };
        self.transport.write_message_with_timeout(
            ADBTransportMessage::try_new(command, self.local_id, self.remote_id, &payload)?,
            Duration::from_secs(10),
        )?;
        for _ in 0..1024 {
            let timeout = Duration::from_secs(10)
                .checked_sub(started.elapsed())
                .filter(|d| !d.is_zero())
                .ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::TimedOut, "SYNC close timed out")
                })?;
            let response = self.transport.read_message_with_timeout(timeout)?;
            if response.header().arg1() != self.local_id
                || response.header().arg0() != self.remote_id
            {
                return Err(RustADBError::ADBRequestFailed(
                    "SYNC close used different stream IDs".into(),
                ));
            }
            match response.header().command() {
                MessageCommand::Clse => {
                    self.peer_closed = true;
                    if !self.recv_incomplete {
                        self.transport.write_message_with_timeout(
                            ADBTransportMessage::try_new(
                                MessageCommand::Clse,
                                self.local_id,
                                self.remote_id,
                                &[],
                            )?,
                            timeout,
                        )?;
                    }
                    return Ok(());
                }
                MessageCommand::Okay => {}
                MessageCommand::Write if self.recv_incomplete => {
                    self.transport.write_message_with_timeout(
                        ADBTransportMessage::try_new(
                            MessageCommand::Okay,
                            self.local_id,
                            self.remote_id,
                            &[],
                        )?,
                        timeout,
                    )?;
                }
                _ => {
                    return Err(RustADBError::ADBRequestFailed(
                        "Unexpected SYNC close response".into(),
                    ));
                }
            }
        }
        Err(RustADBError::ADBRequestFailed(
            "SYNC close response limit exceeded".into(),
        ))
    }

    // [xvolte patch] AOSP recv_impl sends FAIL then ends its SYNC service on
    // open/read errors. Consume that close without sending another SYNC QUIT.
    pub(crate) fn finish_failed_recv(&mut self) -> Result<()> {
        let started = std::time::Instant::now();
        for _ in 0..5 {
            let remaining = Duration::from_secs(10)
                .checked_sub(started.elapsed())
                .filter(|d| !d.is_zero())
                .ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "SYNC failure close timed out",
                    )
                })?;
            let message = self.transport.read_message_with_timeout(remaining)?;
            if message.header().arg1() != self.local_id || message.header().arg0() != self.remote_id
            {
                return Err(RustADBError::ADBRequestFailed(
                    "SYNC failure close used different stream IDs".into(),
                ));
            }
            match message.header().command() {
                MessageCommand::Clse if message.payload().is_empty() => {
                    self.transport.write_message(ADBTransportMessage::try_new(
                        MessageCommand::Clse,
                        self.local_id,
                        self.remote_id,
                        &[],
                    )?)?;
                    return Ok(());
                }
                MessageCommand::Okay if message.payload().is_empty() => {}
                _ => {
                    return Err(RustADBError::ADBRequestFailed(
                        "Unexpected frame while closing failed SYNC RECV".into(),
                    ));
                }
            }
        }
        Err(RustADBError::ADBRequestFailed(
            "Too many frames while closing failed SYNC RECV".into(),
        ))
    }

    fn ensure_sync_payload(&mut self, payload: &mut Vec<u8>, position: &mut usize) -> Result<()> {
        for _ in 0..5 {
            if *position < payload.len() {
                return Ok(());
            }
            let message = self.recv_and_reply_okay()?;
            message.assert_command(MessageCommand::Write)?;
            *payload = message.into_payload();
            *position = 0;
        }
        if payload.is_empty() {
            return Err(RustADBError::ADBRequestFailed(
                "SYNC RECV made no progress".into(),
            ));
        }
        Ok(())
    }

    fn read_sync_exact(
        &mut self,
        payload: &mut Vec<u8>,
        position: &mut usize,
        target: &mut [u8],
    ) -> Result<()> {
        let mut offset = 0;
        while offset < target.len() {
            self.ensure_sync_payload(payload, position)?;
            let count = (target.len() - offset).min(payload.len() - *position);
            target[offset..offset + count].copy_from_slice(&payload[*position..*position + count]);
            offset += count;
            *position += count;
        }
        Ok(())
    }

    pub(crate) fn push_file<R: std::io::Read>(&mut self, mut reader: R) -> Result<()> {
        let mut buffer = vec![0; BUFFER_SIZE].into_boxed_slice();
        let amount_read = reader.read(&mut buffer)?;
        let subcommand_data = MessageSubcommand::Data.with_arg(u32::try_from(amount_read)?);

        let mut serialized_message = subcommand_data.encode();
        serialized_message.append(&mut buffer[..amount_read].to_vec());

        let message = ADBTransportMessage::try_new(
            MessageCommand::Write,
            self.local_id(),
            self.remote_id(),
            &serialized_message,
        )?;

        self.send_and_expect_okay(message)?;

        loop {
            let mut buffer = vec![0; BUFFER_SIZE].into_boxed_slice();

            match reader.read(&mut buffer) {
                Ok(0) => {
                    // Currently file mtime is not forwarded
                    let subcommand_data = MessageSubcommand::Done.with_arg(0);

                    let message = ADBTransportMessage::try_new(
                        MessageCommand::Write,
                        self.local_id(),
                        self.remote_id(),
                        &subcommand_data.encode(),
                    )?;

                    self.send_and_expect_okay(message)?;

                    // Command should end with a Write => Okay
                    let received = self.recv_and_reply_okay()?;
                    match received.header().command() {
                        MessageCommand::Write if received.payload() == b"OKAY\0\0\0\0" => {
                            return Ok(());
                        }
                        MessageCommand::Write => {
                            return Err(RustADBError::ADBRequestFailed(
                                "Push did not return sync OKAY".into(),
                            ));
                        }
                        c => {
                            return Err(RustADBError::ADBRequestFailed(format!(
                                "Wrong command received {c}"
                            )));
                        }
                    }
                }
                Ok(size) => {
                    let subcommand_data = MessageSubcommand::Data.with_arg(u32::try_from(size)?);

                    let mut serialized_message = subcommand_data.encode();
                    serialized_message.append(&mut buffer[..size].to_vec());

                    let message = ADBTransportMessage::try_new(
                        MessageCommand::Write,
                        self.local_id(),
                        self.remote_id(),
                        &serialized_message,
                    )?;

                    self.send_and_expect_okay(message)?;
                }
                Err(e) => {
                    return Err(RustADBError::IOError(e));
                }
            }
        }
    }

    pub(crate) fn stat_with_explicit_ids(&mut self, remote_path: &str) -> Result<AdbStatResponse> {
        let stat_buffer = MessageSubcommand::Stat.with_arg(u32::try_from(remote_path.len())?);
        let message = ADBTransportMessage::try_new(
            MessageCommand::Write,
            self.local_id(),
            self.remote_id(),
            &stat_buffer.encode(),
        )?;
        self.send_and_expect_okay(message)?;
        self.send_and_expect_okay(ADBTransportMessage::try_new(
            MessageCommand::Write,
            self.local_id(),
            self.remote_id(),
            remote_path.as_bytes(),
        )?)?;

        let response = self.recv_and_reply_okay()?;
        response.assert_command(MessageCommand::Write)?;
        let payload = response
            .payload()
            .strip_prefix(b"STAT")
            .ok_or_else(|| RustADBError::ADBRequestFailed("Invalid SYNC STAT response".into()))?;
        AdbStatResponse::decode(payload)
    }
}
