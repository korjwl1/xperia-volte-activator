use rand::RngExt;
use std::{
    path::Path,
    time::{Duration, Instant},
};

use crate::{
    Result, RustADBError,
    error::UNAUTHORIZED_MARKER,
    message_devices::{
        adb_message_transport::ADBMessageTransport,
        adb_session::ADBSession,
        adb_transport_message::{
            ADBTransportMessage, AUTH_RSAPUBLICKEY, AUTH_SIGNATURE, AUTH_TOKEN,
        },
        message_commands::MessageCommand,
        models::{ADBRsaKey, read_adb_private_key},
    },
    models::ADBLocalCommand,
};

/// Generic structure representing an ADB device reachable over an [`ADBMessageTransport`].
/// Structure is totally agnostic over which transport is truly used.
#[derive(Debug)]
pub struct ADBMessageDevice<T: ADBMessageTransport> {
    transport: T,
    sync_batch: bool,
    sync_session: Option<ADBSession<T>>,
    pub(crate) sync_broken: Option<String>,
}

impl<T: ADBMessageTransport> ADBMessageDevice<T> {
    /// Instantiate a new [`ADBMessageDevice`]
    pub fn new<P: AsRef<Path>>(transport: T, adb_private_key_path: P) -> Result<Self> {
        let private_key = if let Some(private_key) = read_adb_private_key(&adb_private_key_path)? {
            private_key
        } else {
            log::warn!(
                "No private key found at path {}. Generating a new random.",
                adb_private_key_path.as_ref().display()
            );
            ADBRsaKey::new_random()?
        };

        let mut message_device = Self {
            transport,
            sync_batch: false,
            sync_session: None,
            sync_broken: None,
        };
        message_device.connect(&private_key)?;

        Ok(message_device)
    }

    pub(crate) const fn get_transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    /// Send initial connect
    fn connect(&mut self, private_key: &ADBRsaKey) -> Result<()> {
        self.get_transport_mut().connect()?;

        let message = ADBTransportMessage::try_new(
            MessageCommand::Cnxn,
            0x0100_0000,
            1_048_576,
            format!("host::{}\0", env!("CARGO_PKG_NAME")).as_bytes(),
        )?;

        self.get_transport_mut().write_message(message)?;

        let message = self.read_connection_message(Duration::from_secs(30))?;

        // Check if a client is requesting a secure connection and upgrade it if necessary
        match message.header().command() {
            MessageCommand::Stls => {
                self.get_transport_mut()
                    .write_message(ADBTransportMessage::try_new(
                        MessageCommand::Stls,
                        1,
                        0,
                        &[],
                    )?)?;
                self.get_transport_mut().upgrade_connection()?;
                log::debug!("Connection successfully upgraded from TCP to TLS");
                Ok(())
            }
            MessageCommand::Cnxn => {
                log::debug!("Unencrypted connection established");
                Ok(())
            }
            MessageCommand::Auth => {
                log::debug!("Authentication required");
                self.auth_handshake(message, private_key)
            }
            _ => Err(crate::RustADBError::WrongResponseReceived(
                "Expected CNXN, STLS or AUTH command".to_string(),
                message.header().command().to_string(),
            )),
        }
    }

    // [xvolte patch] A killed USB client can leave stream replies that arrive
    // AFTER the pre-CNXN drain. Before connection/authentication completes there
    // are no streams owned by this client. Ignore whole stale stream frames,
    // without ACKs or replaying requests, as required by the ADB protocol:
    // https://android.googlesource.com/platform/packages/modules/adb/+/refs/heads/main/docs/dev/protocol.md
    // Frame parsing/integrity errors still propagate. Bound both time and count.
    fn read_connection_message(&mut self, timeout: Duration) -> Result<ADBTransportMessage> {
        const MAX_STALE_FRAMES: usize = 64;
        let started = Instant::now();
        for stale in 0..=MAX_STALE_FRAMES {
            let remaining = timeout
                .checked_sub(started.elapsed())
                .filter(|d| !d.is_zero())
                .ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "ADB connection handshake timed out",
                    )
                })?;
            let message = self.transport.read_message_with_timeout(remaining)?;
            match message.header().command() {
                MessageCommand::Cnxn | MessageCommand::Auth | MessageCommand::Stls => {
                    return Ok(message);
                }
                MessageCommand::Open
                | MessageCommand::Okay
                | MessageCommand::Write
                | MessageCommand::Clse => {
                    if stale == MAX_STALE_FRAMES {
                        return Err(RustADBError::ADBRequestFailed(
                            "Too many stale stream frames during ADB connection handshake".into(),
                        ));
                    }
                    log::debug!(
                        "Ignoring stale {} before ADB connection completes",
                        message.header().command()
                    );
                }
            }
        }
        unreachable!("stale frame limit returns an error")
    }

    fn auth_handshake(
        &mut self,
        message: ADBTransportMessage,
        private_key: &ADBRsaKey,
    ) -> Result<()> {
        match message.header().command() {
            MessageCommand::Auth => {
                log::debug!("Authentication required");
            }
            _ => return Ok(()),
        }

        // At this point, we should have received an AUTH message with arg0 == 1
        let auth_message = match message.header().arg0() {
            AUTH_TOKEN => message,
            v => {
                return Err(RustADBError::ADBRequestFailed(format!(
                    "Received AUTH message with type != 1 ({v})"
                )));
            }
        };

        let sign = private_key.sign(auth_message.into_payload())?;

        let message = ADBTransportMessage::try_new(MessageCommand::Auth, AUTH_SIGNATURE, 0, &sign)?;

        self.transport.write_message(message)?;

        let received_response = self.read_connection_message(Duration::from_secs(30))?;

        if received_response.header().command() == MessageCommand::Cnxn {
            log::info!(
                "Authentication OK, device info {}",
                String::from_utf8(received_response.into_payload())?
            );
            return Ok(());
        }

        let mut pubkey = private_key.android_pubkey_encode()?.into_bytes();
        pubkey.push(b'\0');

        let message =
            ADBTransportMessage::try_new(MessageCommand::Auth, AUTH_RSAPUBLICKEY, 0, &pubkey)?;

        self.transport.write_message(message)?;

        // [xvolte patch] 공개 키를 보낸 뒤 응답이 없으면 폰에 "USB 디버깅 허용" 창이 떠 있는 상태다.
        // 일반 시간 초과와 구분할 수 있게 고정 문구(UNAUTHORIZED_MARKER)로 돌려준다.
        let response = self
            .read_connection_message(Duration::from_secs(10))
            .map_err(|e| match e {
                #[cfg(feature = "usb")]
                RustADBError::UsbError(rusb::Error::Timeout) => {
                    RustADBError::ADBRequestFailed(UNAUTHORIZED_MARKER.into())
                }
                RustADBError::IOError(io)
                    if matches!(
                        io.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) =>
                {
                    RustADBError::ADBRequestFailed(UNAUTHORIZED_MARKER.into())
                }
                other => other,
            })
            .and_then(|message| {
                message.assert_command(MessageCommand::Cnxn)?;
                Ok(message)
            })?;

        log::info!(
            "Authentication OK, device info {}",
            String::from_utf8(response.into_payload())?
        );
        Ok(())
    }

    pub(crate) fn open_synchronization_session(&mut self) -> Result<ADBSession<T>> {
        self.open_session(&ADBLocalCommand::Sync)
    }

    pub(crate) fn begin_sync_batch(&mut self) -> Result<bool> {
        if self.sync_batch {
            return Err(RustADBError::ADBRequestFailed(
                "SYNC batch is already active".into(),
            ));
        }
        if let Some(reason) = &self.sync_broken {
            return Err(RustADBError::ADBRequestFailed(reason.clone()));
        }
        self.sync_batch = true;
        Ok(true)
    }

    pub(crate) fn end_sync_batch(&mut self) -> Result<()> {
        self.sync_batch = false;
        self.close_cached_sync()
    }

    pub(crate) fn needs_reconnect(&self) -> bool {
        self.sync_broken.is_some()
    }

    fn close_cached_sync(&mut self) -> Result<()> {
        if let Some(mut session) = self.sync_session.take() {
            if let Err(error) = self.end_transaction(&mut session) {
                let reason = format!("SYNC_BATCH_BROKEN|Cannot close SYNC stream: {error}");
                self.sync_broken = Some(reason.clone());
                return Err(RustADBError::ADBRequestFailed(reason));
            }
        }
        Ok(())
    }

    pub(crate) fn take_sync_session(&mut self) -> Result<ADBSession<T>> {
        if let Some(reason) = &self.sync_broken {
            return Err(RustADBError::ADBRequestFailed(reason.clone()));
        }
        match self.sync_session.take() {
            Some(session) => Ok(session),
            None => self.open_synchronization_session(),
        }
    }

    pub(crate) fn finish_sync_request<R>(
        &mut self,
        mut session: ADBSession<T>,
        result: Result<R>,
    ) -> Result<R> {
        if result.is_ok() && self.sync_batch {
            self.sync_session = Some(session);
            return result;
        }
        // adbd terminates SYNC after a RECV FAIL. QUIT would race its CLSE
        // and can leave an extra OKAY queued for the next stream.
        let recv_failed = matches!(&result,
            Err(RustADBError::ADBRequestFailed(reason)) if reason.starts_with("SYNC RECV failed:"));
        if result.is_err() && !recv_failed {session.mark_incomplete(true);}
        let closing = if recv_failed {
            session.finish_failed_recv()
        } else {
            self.end_transaction(&mut session)
        };
        if let Err(error) = closing {
            let reason = format!(
                "SYNC_BATCH_BROKEN|Cannot finish SYNC stream: {error}; request: {}",
                result
                    .as_ref()
                    .err()
                    .map(ToString::to_string)
                    .unwrap_or_default()
            );
            self.sync_broken = Some(reason.clone());
            return Err(RustADBError::ADBRequestFailed(reason));
        }
        result
    }

    /// Open a new ADB session with the given local command.
    pub(crate) fn open_session(&mut self, cmd: &ADBLocalCommand) -> Result<ADBSession<T>> {
        // Direct transports do not multiplex streams. Finish cached SYNC before
        // another service (including a shell size check) reads the transport.
        self.close_cached_sync()?;
        if let Some(reason) = &self.sync_broken {
            return Err(RustADBError::ADBRequestFailed(reason.clone()));
        }
        let mut rng = rand::rng();
        let local_id: u32 = rng.random();

        // adbd used to expect a null-terminated string.
        // keep doing so to maintain compatibility with older versions.
        // https://cs.android.com/android/platform/superproject/+/android-latest-release:packages/modules/adb/sockets.cpp;l=560?q=sockets.cpp
        let mut destination = cmd.to_string().into_bytes();
        if !destination.ends_with(&[0]) {
            destination.push(0);
        }

        let message = ADBTransportMessage::try_new(
            MessageCommand::Open,
            local_id, // Our 'local-id'
            0,
            &destination,
        )?;
        let result = (|| {
            self.transport
                .write_message_with_timeout(message, Duration::from_secs(10))?;
            let started = Instant::now();
            for _ in 0..16 {
                let timeout = Duration::from_secs(10)
                    .checked_sub(started.elapsed())
                    .filter(|d| !d.is_zero())
                    .ok_or_else(|| {
                        std::io::Error::new(std::io::ErrorKind::TimedOut, "OPEN timed out")
                    })?;
                let response = self.transport.read_message_with_timeout(timeout)?;
                if response.header().command() == MessageCommand::Clse
                    && response.header().arg1() != local_id
                {
                    continue;
                }
                if response.header().command() != MessageCommand::Okay
                    || response.header().arg1() != local_id
                {
                    return Err(RustADBError::ADBRequestFailed(
                        "Open session failed: unexpected response/stream IDs".into(),
                    ));
                }
                return Ok(ADBSession::new(
                    self.transport.clone(),
                    local_id,
                    response.header().arg0(),
                ));
            }
            Err(RustADBError::ADBRequestFailed(
                "Open session failed: stale response limit exceeded".into(),
            ))
        })();
        result.map_err(|error| {
            let reason =
                format!("SYNC_BATCH_BROKEN|Open session failed; reconnect required: {error}");
            self.sync_broken = Some(reason.clone());
            RustADBError::ADBRequestFailed(reason)
        })
    }

    pub(crate) fn end_transaction(&mut self, session: &mut ADBSession<T>) -> Result<()> {
        session.close_sync()
    }
}

impl<T: ADBMessageTransport> Drop for ADBMessageDevice<T> {
    fn drop(&mut self) {
        let _ = self.end_sync_batch();
        // Best effort here
        let _ = self.get_transport_mut().disconnect();
    }
}

#[cfg(test)]
mod handshake_tests {
    use super::*;
    use crate::ADBTransport;
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex, OnceLock},
    };

    #[derive(Default)]
    struct Wire {
        // Each outbound handshake message releases its scripted response.
        // Thus stale CLSE arrives after CNXN, reproducing the missed drain race.
        replies: VecDeque<Vec<ADBTransportMessage>>,
        incoming: VecDeque<ADBTransportMessage>,
        writes: Vec<(MessageCommand, u32)>,
        timeouts: Vec<Duration>,
        upgrades: usize,
        corrupt: bool,
    }
    #[derive(Clone, Default)]
    struct Transport(Arc<Mutex<Wire>>);
    impl ADBTransport for Transport {
        fn connect(&mut self) -> Result<()> {
            Ok(())
        }
        fn disconnect(&mut self) -> Result<()> {
            Ok(())
        }
    }
    impl ADBMessageTransport for Transport {
        fn upgrade_connection(&mut self) -> Result<()> {
            self.0.lock().unwrap().upgrades += 1;
            Ok(())
        }
        fn read_message_with_timeout(&mut self, timeout: Duration) -> Result<ADBTransportMessage> {
            let mut wire = self.0.lock().unwrap();
            wire.timeouts.push(timeout);
            if wire.corrupt {
                return Err(RustADBError::InvalidIntegrity(1, 2));
            }
            wire.incoming.pop_front().ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::TimedOut, "no handshake packet").into()
            })
        }
        fn write_message_with_timeout(
            &mut self,
            message: ADBTransportMessage,
            _: Duration,
        ) -> Result<()> {
            let mut wire = self.0.lock().unwrap();
            wire.writes
                .push((message.header().command(), message.header().arg0()));
            if let Some(reply) = wire.replies.pop_front() {
                wire.incoming.extend(reply);
            }
            Ok(())
        }
    }
    fn frame(command: MessageCommand) -> ADBTransportMessage {
        let (a, b, payload): (u32, u32, &[u8]) = match command {
            MessageCommand::Cnxn => (0x0100_0000, 1_048_576, b"device::\0"),
            MessageCommand::Auth => (AUTH_TOKEN, 0, &[7; 20]),
            MessageCommand::Stls => (1, 0, &[]),
            MessageCommand::Write => (7, 11, b"old file bytes"),
            _ => (7, 11, &[]),
        };
        ADBTransportMessage::try_new(command, a, b, payload).unwrap()
    }
    fn device(replies: Vec<Vec<ADBTransportMessage>>) -> (ADBMessageDevice<Transport>, Transport) {
        let transport = Transport::default();
        transport.0.lock().unwrap().replies = replies.into();
        (
            ADBMessageDevice {
                transport: transport.clone(),
                sync_batch: false,
                sync_session: None,
                sync_broken: None,
            },
            transport,
        )
    }
    fn key() -> &'static ADBRsaKey {
        static KEY: OnceLock<ADBRsaKey> = OnceLock::new();
        KEY.get_or_init(|| ADBRsaKey::new_random().unwrap())
    }

    #[test]
    fn delayed_stream_frames_before_cnxn_are_ignored_without_ack_or_service_replay() {
        let (mut device, transport) = device(vec![vec![
            frame(MessageCommand::Clse),
            frame(MessageCommand::Okay),
            frame(MessageCommand::Write),
            frame(MessageCommand::Open),
            frame(MessageCommand::Cnxn),
            frame(MessageCommand::Write),
        ]]);
        device.connect(key()).unwrap();
        let wire = transport.0.lock().unwrap();
        assert_eq!(wire.writes, vec![(MessageCommand::Cnxn, 0x0100_0000)]);
        // A fresh connection's next frame must remain available to its service.
        assert_eq!(wire.incoming.len(), 1);
        assert_eq!(wire.timeouts.len(), 5);
        assert!(
            wire.timeouts
                .iter()
                .all(|d| !d.is_zero() && *d <= Duration::from_secs(30))
        );
        assert!(wire.timeouts.windows(2).all(|w| w[1] <= w[0]));
    }

    #[test]
    fn delayed_close_during_signature_does_not_trigger_public_key_registration() {
        let (mut device, transport) = device(vec![
            vec![frame(MessageCommand::Clse), frame(MessageCommand::Auth)],
            vec![frame(MessageCommand::Clse), frame(MessageCommand::Cnxn)],
        ]);
        device.connect(key()).unwrap();
        assert_eq!(
            transport.0.lock().unwrap().writes,
            vec![
                (MessageCommand::Cnxn, 0x0100_0000),
                (MessageCommand::Auth, AUTH_SIGNATURE),
            ]
        );
    }

    #[test]
    fn public_key_confirmation_ignores_late_stream_close_and_keeps_authorization_timeout() {
        for confirmed in [true, false] {
            let mut confirmation = vec![frame(MessageCommand::Clse)];
            if confirmed {
                confirmation.push(frame(MessageCommand::Cnxn));
            }
            let (mut device, transport) = device(vec![
                vec![frame(MessageCommand::Auth)],
                vec![frame(MessageCommand::Auth)],
                confirmation,
            ]);
            let result = device.connect(key());
            if confirmed {
                result.unwrap();
            } else {
                assert!(
                    result
                        .unwrap_err()
                        .to_string()
                        .contains(UNAUTHORIZED_MARKER)
                );
            }
            let wire = transport.0.lock().unwrap();
            assert_eq!(
                wire.writes.last(),
                Some(&(MessageCommand::Auth, AUTH_RSAPUBLICKEY))
            );
            assert!(
                wire.timeouts[2..]
                    .iter()
                    .all(|d| *d <= Duration::from_secs(10))
            );
        }
    }

    #[test]
    fn stale_frame_flood_is_bounded_and_corrupt_frames_remain_errors() {
        let (mut device, transport) =
            device(vec![(0..65).map(|_| frame(MessageCommand::Clse)).collect()]);
        assert!(
            device
                .connect(key())
                .unwrap_err()
                .to_string()
                .contains("Too many stale")
        );
        let wire = transport.0.lock().unwrap();
        assert_eq!(wire.timeouts.len(), 65);
        assert_eq!(wire.writes.len(), 1);
        drop(wire);
        let (mut device, transport) = self::device(vec![vec![frame(MessageCommand::Cnxn)]]);
        transport.0.lock().unwrap().corrupt = true;
        assert!(matches!(
            device.connect(key()),
            Err(RustADBError::InvalidIntegrity(1, 2))
        ));
    }

    #[test]
    fn stale_close_before_tls_preserves_upgrade_and_no_response_stays_failure() {
        let (mut device, transport) = device(vec![vec![
            frame(MessageCommand::Clse),
            frame(MessageCommand::Stls),
        ]]);
        device.connect(key()).unwrap();
        let wire = transport.0.lock().unwrap();
        assert_eq!(wire.upgrades, 1);
        assert_eq!(
            wire.writes,
            vec![
                (MessageCommand::Cnxn, 0x0100_0000),
                (MessageCommand::Stls, 1)
            ]
        );
        drop(wire);
        let (mut device, _) = self::device(vec![vec![frame(MessageCommand::Clse)]]);
        assert!(
            matches!(device.connect(key()), Err(RustADBError::IOError(e))
            if e.kind() == std::io::ErrorKind::TimedOut)
        );
        assert!(device.read_connection_message(Duration::ZERO).is_err());
    }
    #[test]
    fn open_stale_close_limit_requires_reconnect_without_next_open() {
        let (mut d,t)=device(vec![(0..17).map(|_|frame(MessageCommand::Clse)).collect()]);
        assert!(d.open_session(&ADBLocalCommand::Shell).is_err());
        assert!(d.needs_reconnect());
        let writes=t.0.lock().unwrap().writes.len();
        assert!(d.open_session(&ADBLocalCommand::Shell).is_err());
        assert_eq!(writes,t.0.lock().unwrap().writes.len());
        assert_eq!(t.0.lock().unwrap().timeouts.len(),16);
    }

}

#[cfg(test)]
mod batch_tests {
    use super::*;
    use crate::ADBTransport;
    use std::{
        collections::{HashMap, HashSet, VecDeque},
        sync::{Arc, Mutex},
    };

    #[derive(Default)]
    struct Wire {
        incoming: VecDeque<ADBTransportMessage>,
        requests: Vec<Vec<u8>>,
        pending: Vec<u8>,
        files: HashMap<String, Vec<u8>>,
        denied: HashSet<String>,
        denied_close_mode: u8,
        local: u32,
        opens: usize,
        quits: usize,
        drains: usize,
        reject_quit: bool,
        recv_chunk: usize,
        awaiting_ack: bool,
        peer_closed: bool,
    }
    #[derive(Clone, Default)]
    struct Transport(Arc<Mutex<Wire>>);
    impl ADBTransport for Transport {
        fn connect(&mut self) -> Result<()> {
            Ok(())
        }
        fn disconnect(&mut self) -> Result<()> {
            Ok(())
        }
    }
    impl ADBMessageTransport for Transport {
        fn read_message_with_timeout(&mut self, timeout: Duration) -> Result<ADBTransportMessage> {
            let mut wire = self.0.lock().unwrap();
            if wire.awaiting_ack {
                return Err(RustADBError::ADBRequestFailed(
                    "response not acknowledged".into(),
                ));
            }
            if timeout == Duration::from_millis(20) {
                wire.drains += 1;
            }
            let message = wire
                .incoming
                .pop_front()
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::TimedOut, "no packet"))?;
            wire.awaiting_ack = message.header().command() == MessageCommand::Write;
            if message.header().command() == MessageCommand::Clse {
                wire.peer_closed = true;
            }
            Ok(message)
        }
        fn write_message_with_timeout(
            &mut self,
            message: ADBTransportMessage,
            _: Duration,
        ) -> Result<()> {
            let mut wire = self.0.lock().unwrap();
            match message.header().command() {
                MessageCommand::Open => {
                    wire.local = message.header().arg0();
                    wire.opens += 1;
                    wire.peer_closed = false;
                    let local = wire.local;
                    wire.incoming.push_back(ADBTransportMessage::try_new(
                        MessageCommand::Okay,
                        7,
                        local,
                        &[],
                    )?);
                }
                MessageCommand::Write => {
                    let local = wire.local;
                    wire.incoming.push_back(ADBTransportMessage::try_new(
                        MessageCommand::Okay,
                        7,
                        local,
                        &[],
                    )?);
                    wire.pending.extend_from_slice(message.payload());
                    if wire.pending.len() < 8 {
                        return Ok(());
                    }
                    let length =
                        u32::from_le_bytes(wire.pending[4..8].try_into().unwrap()) as usize;
                    if wire.pending.len() < 8 + length {
                        return Ok(());
                    }
                    let request = std::mem::take(&mut wire.pending);
                    let path = String::from_utf8_lossy(&request[8..]).to_string();
                    let mut response = Vec::new();
                    let recv_denied = request.starts_with(b"RECV") && wire.denied.contains(&path);
                    match &request[..4] {
                        b"STAT" => {
                            response.extend_from_slice(b"STAT");
                            for field in [
                                0o100644u32,
                                wire.files.get(&path).map_or(0, |v| v.len() as u32),
                                123,
                            ] {
                                response.extend_from_slice(&field.to_le_bytes());
                            }
                        }
                        b"RECV" => {
                            if recv_denied {
                                let reason = b"open failed: Permission denied";
                                response.extend_from_slice(b"FAIL");
                                response.extend_from_slice(&(reason.len() as u32).to_le_bytes());
                                response.extend_from_slice(reason);
                            } else {
                                let bytes = wire.files.get(&path).cloned().unwrap_or_default();
                                response.extend_from_slice(b"DATA");
                                response.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                                response.extend_from_slice(&bytes);
                                response.extend_from_slice(b"DONE\0\0\0\0");
                            }
                        }
                        b"LIST" => {
                            response.extend_from_slice(b"DONE\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0")
                        }
                        b"QUIT" => {
                            wire.quits += 1;
                            let command = if wire.reject_quit {
                                MessageCommand::Write
                            } else {
                                MessageCommand::Clse
                            };
                            wire.incoming.push_back(ADBTransportMessage::try_new(
                                command,
                                7,
                                local,
                                &[],
                            )?);
                        }
                        _ => panic!("unexpected request"),
                    }
                    let is_recv = request.starts_with(b"RECV");
                    wire.requests.push(request);
                    if !response.is_empty() {
                        let chunk = if is_recv && wire.recv_chunk > 0 {
                            wire.recv_chunk
                        } else {
                            response.len()
                        };
                        for bytes in response.chunks(chunk) {
                            wire.incoming.push_back(ADBTransportMessage::try_new(
                                MessageCommand::Write,
                                7,
                                local,
                                bytes,
                            )?);
                        }
                    }
                    if recv_denied && wire.denied_close_mode != 1 {
                        let close_local = if wire.denied_close_mode == 2 {
                            local.wrapping_add(1)
                        } else {
                            local
                        };
                        wire.incoming.push_back(ADBTransportMessage::try_new(
                            MessageCommand::Clse,
                            7,
                            close_local,
                            &[],
                        )?);
                    }
                }
                MessageCommand::Okay => {
                    wire.awaiting_ack = false;
                }
                MessageCommand::Clse => {
                    wire.awaiting_ack = false;
                    // A host-initiated abort must get a peer CLSE after pending DATA.
                    if !wire.peer_closed
                        && wire.requests.last().is_some_and(|r| r.starts_with(b"RECV"))
                        && !wire
                            .incoming
                            .iter()
                            .any(|m| m.header().command() == MessageCommand::Clse)
                    {
                        let local = wire.local;
                        wire.peer_closed = true;
                        wire.incoming.push_back(ADBTransportMessage::try_new(
                            MessageCommand::Clse,
                            7,
                            local,
                            &[],
                        )?);
                    }
                }
                _ => panic!("unexpected command"),
            }
            Ok(())
        }
    }
    fn device() -> (ADBMessageDevice<Transport>, Transport) {
        let transport = Transport::default();
        transport
            .0
            .lock()
            .unwrap()
            .files
            .insert("/sdcard/사진 1.bin".into(), b"binaryDONEpayload".to_vec());
        let device = ADBMessageDevice {
            transport: transport.clone(),
            sync_batch: false,
            sync_session: None,
            sync_broken: None,
        };
        (device, transport)
    }
    #[test]
    fn recv_permission_failure_consumes_remote_close_without_quit_and_next_file_works() {
        let (mut device, transport) = device();
        transport
            .0
            .lock()
            .unwrap()
            .denied
            .insert("/sdcard/denied.bin".into());
        transport
            .0
            .lock()
            .unwrap()
            .files
            .insert("/sdcard/denied.bin".into(), vec![0]);
        // Split FAIL header/reason as on a real ADB stream.
        transport.0.lock().unwrap().recv_chunk = 3;
        device.begin_sync_batch().unwrap();
        let error = device.pull("/sdcard/denied.bin", Vec::new()).unwrap_err();
        assert!(error.to_string().contains("Permission denied"));
        assert!(!error.to_string().contains("SYNC_BATCH_BROKEN"));
        assert!(!device.needs_reconnect());
        assert_eq!(transport.0.lock().unwrap().quits, 0);
        let mut output = Vec::new();
        device.pull("/sdcard/사진 1.bin", &mut output).unwrap();
        assert_eq!(output, b"binaryDONEpayload");
        device.end_sync_batch().unwrap();
        let wire = transport.0.lock().unwrap();
        assert_eq!((wire.opens, wire.quits), (2, 1));
        assert!(wire.incoming.is_empty());
    }

    #[test]
    fn missing_or_wrong_stream_close_after_recv_fail_requires_reconnection() {
        for mode in [1, 2] {
            let (mut device, transport) = device();
            {
                let mut wire = transport.0.lock().unwrap();
                wire.denied.insert("/sdcard/denied.bin".into());
                wire.files.insert("/sdcard/denied.bin".into(), vec![0]);
                wire.denied_close_mode = mode;
            }
            device.begin_sync_batch().unwrap();
            let error = device.pull("/sdcard/denied.bin", Vec::new()).unwrap_err();
            assert!(error.to_string().contains("SYNC_BATCH_BROKEN"));
            assert!(error.to_string().contains("Permission denied"));
            assert!(device.needs_reconnect());
            assert!(device.pull("/sdcard/사진 1.bin", Vec::new()).is_err());
            assert_eq!(transport.0.lock().unwrap().opens, 1);
        }
    }

    #[test]
    fn list_and_multiple_files_share_one_stream_without_per_file_close_wait() {
        let (mut device, transport) = device();
        device.begin_sync_batch().unwrap();
        assert!(device.list("/sdcard/empty").unwrap().is_empty());
        for path in [
            "/sdcard/사진 1.bin",
            "/sdcard/empty.bin",
            "/sdcard/사진 1.bin",
        ] {
            let mut output = Vec::new();
            device.pull(path, &mut output).unwrap();
            assert_eq!(
                output,
                transport
                    .0
                    .lock()
                    .unwrap()
                    .files
                    .get(path)
                    .cloned()
                    .unwrap_or_default()
            );
        }
        {
            let wire = transport.0.lock().unwrap();
            assert_eq!((wire.opens, wire.quits, wire.drains), (1, 0, 0));
        }
        device.end_sync_batch().unwrap();
        let wire = transport.0.lock().unwrap();
        assert_eq!((wire.opens, wire.quits, wire.drains), (1, 1, 0));
    }
    #[test]
    fn standalone_pull_keeps_one_request_lifetime_and_empty_batch_opens_nothing() {
        let (mut device, transport) = device();
        device.begin_sync_batch().unwrap();
        assert!(device.begin_sync_batch().is_err());
        device.end_sync_batch().unwrap();
        assert_eq!(transport.0.lock().unwrap().opens, 0);
        for _ in 0..2 {
            device.pull("/sdcard/사진 1.bin", Vec::new()).unwrap();
        }
        let wire = transport.0.lock().unwrap();
        assert_eq!((wire.opens, wire.quits, wire.drains), (2, 2, 0));
    }
    #[test]
    fn opening_another_service_finishes_sync_before_using_shared_transport() {
        let (mut device, transport) = device();
        device.begin_sync_batch().unwrap();
        device.pull("/sdcard/사진 1.bin", Vec::new()).unwrap();
        let session = device.open_session(&ADBLocalCommand::Shell).unwrap();
        assert_eq!(transport.0.lock().unwrap().quits, 1);
        drop(session);
        device.pull("/sdcard/사진 1.bin", Vec::new()).unwrap();
        device.end_sync_batch().unwrap();
        assert_eq!(transport.0.lock().unwrap().opens, 3);
    }
    #[test]
    fn failed_batch_close_blocks_further_transport_requests() {
        let (mut device, transport) = device();
        device.begin_sync_batch().unwrap();
        device.pull("/sdcard/사진 1.bin", Vec::new()).unwrap();
        transport.0.lock().unwrap().reject_quit = true;
        assert!(
            device
                .end_sync_batch()
                .unwrap_err()
                .to_string()
                .contains("SYNC_BATCH_BROKEN|")
        );
        assert!(device.needs_reconnect());
        let opens = transport.0.lock().unwrap().opens;
        assert!(device.pull("/sdcard/사진 1.bin", Vec::new()).is_err());
        assert!(device.open_session(&ADBLocalCommand::Shell).is_err());
        assert_eq!(transport.0.lock().unwrap().opens, opens);
    }

    #[test]
    fn fragmented_and_empty_files_do_not_leave_done_bytes_for_the_next_request() {
        for chunk in [1, 3, 7, 8, 17] {
            let (mut device, transport) = device();
            transport.0.lock().unwrap().recv_chunk = chunk;
            device.begin_sync_batch().unwrap();
            for path in [
                "/sdcard/사진 1.bin",
                "/sdcard/empty.bin",
                "/sdcard/사진 1.bin",
            ] {
                let mut output = Vec::new();
                device.pull(path, &mut output).unwrap();
                assert_eq!(
                    output,
                    transport
                        .0
                        .lock()
                        .unwrap()
                        .files
                        .get(path)
                        .cloned()
                        .unwrap_or_default()
                );
            }
            device.end_sync_batch().unwrap();
            assert!(transport.0.lock().unwrap().incoming.is_empty());
        }
    }

    #[test]
    fn failed_open_marks_connection_broken_and_blocks_following_opens() {
        let (mut device, transport) = device();
        transport
            .0
            .lock()
            .unwrap()
            .incoming
            .push_back(ADBTransportMessage::try_new(MessageCommand::Auth, 0, 0, &[]).unwrap());
        assert!(
            device
                .list("/sdcard")
                .unwrap_err()
                .to_string()
                .contains("SYNC_BATCH_BROKEN|")
        );
        assert!(device.needs_reconnect());
        assert!(device.list("/sdcard").is_err());
        assert_eq!(transport.0.lock().unwrap().opens, 1);
    }
    #[test]
    fn stale_close_before_open_does_not_shift_the_next_stream() {
        let (mut device, transport) = device();
        transport.0.lock().unwrap().incoming.push_back(
            ADBTransportMessage::try_new(MessageCommand::Clse, 9, u32::MAX, &[]).unwrap(),
        );
        device.list("/sdcard").unwrap();
        assert!(!device.needs_reconnect());
        assert!(transport.0.lock().unwrap().incoming.is_empty());
    }
    #[test]
    fn destination_write_error_discards_stream_and_next_file_opens_fresh_stream() {
        struct BrokenWriter;
        impl std::io::Write for BrokenWriter {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("destination failure"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let (mut device, transport) = device();
        transport.0.lock().unwrap().files.insert("/sdcard/사진 1.bin".into(),vec![42;160*1024]);
        transport.0.lock().unwrap().recv_chunk=32*1024;
        device.begin_sync_batch().unwrap();
        assert!(device.pull("/sdcard/사진 1.bin", BrokenWriter).is_err());
        {let wire=transport.0.lock().unwrap();assert!(wire.incoming.is_empty());assert!(!wire.awaiting_ack);assert_eq!(wire.quits,0);}
        device.pull("/sdcard/사진 1.bin", Vec::new()).unwrap();
        device.end_sync_batch().unwrap();
        assert_eq!(transport.0.lock().unwrap().opens, 2);
    }
}

#[cfg(test)]
mod exec_tests {
    use super::*;
    use crate::ADBTransport;
    use std::{collections::VecDeque,sync::{Arc,Mutex},io::{Read,Write}};
    #[derive(Default)]
    struct Wire {incoming:VecDeque<ADBTransportMessage>,local:u32,input:Vec<u8>,peer_closed:bool,reject:bool}
    #[derive(Clone,Default)]struct Transport(Arc<Mutex<Wire>>);
    impl ADBTransport for Transport {fn connect(&mut self)->Result<()>{Ok(())}fn disconnect(&mut self)->Result<()>{Ok(())}}
    impl ADBMessageTransport for Transport {
        fn read_message_with_timeout(&mut self,_:Duration)->Result<ADBTransportMessage>{self.0.lock().unwrap().incoming.pop_front().ok_or_else(||std::io::Error::new(std::io::ErrorKind::TimedOut,"empty wire").into())}
        fn write_message_with_timeout(&mut self,message:ADBTransportMessage,_:Duration)->Result<()> {
            let mut w=self.0.lock().unwrap();let packet=|cmd,local,payload:&[u8]|ADBTransportMessage::try_new(cmd,7,local,payload).unwrap();
            match message.header().command() {
                MessageCommand::Open=>{w.local=message.header().arg0();w.peer_closed=false;let id=w.local;w.incoming.push_back(packet(MessageCommand::Okay,id,b""));
                    if message.payload().starts_with(b"shell:"){w.incoming.push_back(packet(MessageCommand::Write,id,b"ok"));w.incoming.push_back(packet(MessageCommand::Clse,id,b""));w.peer_closed=true;}}
                MessageCommand::Write=>{let id=w.local;w.input.extend_from_slice(message.payload());
                    if w.reject {w.incoming.push_back(packet(MessageCommand::Write,id,b"tar rejected input"));w.incoming.push_back(packet(MessageCommand::Clse,id,b""));w.peer_closed=true;}
                    else {w.incoming.push_back(packet(MessageCommand::Write,id,b"status"));w.incoming.push_back(packet(MessageCommand::Okay,id,b""));if w.input.len()==100_000 {w.incoming.push_back(packet(MessageCommand::Clse,id,b""));w.peer_closed=true;}}}
                MessageCommand::Clse if !w.peer_closed=>{let id=w.local;w.incoming.push_back(packet(MessageCommand::Clse,id,b""));w.peer_closed=true;},
                _=>{},
            }Ok(())
        }
    }
    #[derive(Clone,Default)]struct Output(Arc<Mutex<Vec<u8>>>);
    impl Write for Output {fn write(&mut self,b:&[u8])->std::io::Result<usize>{self.0.lock().unwrap().extend_from_slice(b);Ok(b.len())}fn flush(&mut self)->std::io::Result<()>{Ok(())}}
    fn device()->(ADBMessageDevice<Transport>,Transport){let t=Transport::default();(ADBMessageDevice{transport:t.clone(),sync_batch:false,sync_session:None,sync_broken:None},t)}
    #[test]
    fn exec_serializes_input_ack_output_and_close_without_a_detached_reader() {
        let (mut d,t)=device();let output=Output::default();let mut input=std::io::Cursor::new(vec![42;100_000]);
        d.exec("tar -x",&mut input,Box::new(output.clone())).unwrap();
        assert_eq!(t.0.lock().unwrap().input.len(),100_000);assert_eq!(*output.0.lock().unwrap(),b"statusstatus");assert!(t.0.lock().unwrap().incoming.is_empty());
        let mut stdout=Vec::new();d.shell_command(&"true",Some(&mut stdout),None).unwrap();assert_eq!(stdout,b"ok");
    }
    #[test]
    fn early_remote_close_and_aborted_pc_input_allow_following_cleanup() {
        struct BadInput;
        impl Read for BadInput {fn read(&mut self,_:&mut[u8])->std::io::Result<usize>{Err(std::io::Error::other("PC source read failed"))}}
        for remote_close in [false,true] {
            let (mut d,t)=device();t.0.lock().unwrap().reject=remote_close;
            let output=Output::default();let error=if remote_close {d.exec("tar -x",&mut std::io::Cursor::new(vec![42;100_000]),Box::new(output)).unwrap_err()} else {d.exec("tar -x",&mut BadInput,Box::new(output)).unwrap_err()};
            if remote_close {assert!(error.to_string().contains("tar rejected input"));}
            assert!(!d.needs_reconnect());let mut stdout=Vec::new();d.shell_command(&"rm stage",Some(&mut stdout),None).unwrap();assert_eq!(stdout,b"ok");
        }
    }
}
