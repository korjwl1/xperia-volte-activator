use byteorder::ByteOrder;
use byteorder::LittleEndian;
use std::str;

use crate::message_devices::adb_message_device::ADBMessageDevice;
use crate::message_devices::adb_message_transport::ADBMessageTransport;
use crate::message_devices::adb_session::ADBSession;
use crate::message_devices::adb_session::trace_sync;
use crate::message_devices::adb_transport_message::ADBTransportMessage;
use crate::message_devices::message_commands::MessageCommand;
use crate::message_devices::message_commands::MessageSubcommand;
use crate::message_devices::utils::BinaryEncodable;
use crate::models::{ADBListItem, ADBListItemType};
use crate::{Result, RustADBError};

impl<T: ADBMessageTransport> ADBMessageDevice<T> {
    /// List the entries in the given directory on the device.
    /// note: path uses internal file paths, so Documents is at /storage/emulated/0/Documents
    pub(crate) fn list<A: AsRef<str>>(&mut self, path: A) -> Result<Vec<ADBListItemType>> {
        let mut session = self.take_sync_session()?;

        let output = Self::handle_list(&mut session, path);

        self.finish_sync_request(session, output)
    }

    /// Request amount of bytes from transport, potentially across payloads
    ///
    /// This automatically request a new payload by sending back "Okay" and waiting for the next payload
    /// It reads the request bytes across the existing payload, and if there is not enough bytes left,
    /// reads the rest from the next payload
    ///
    ///   Current index
    /// ┼───────────────┼   Requested
    ///                 ┌─────────────┐
    /// ┌───────────────┼───────┐     │
    /// └───────────────────────┘
    ///     Current             └─────┘
    ///     payload          Wanted in
    ///                      Next payload
    fn read_bytes_from_transport(
        session: &mut ADBSession<T>,
        requested_bytes: usize,
        current_index: &mut usize,
        payload: &mut Vec<u8>,
    ) -> Result<Vec<u8>> {
        let mut bytes = Vec::with_capacity(requested_bytes);
        let mut empty_packets = 0;
        while bytes.len() < requested_bytes {
            if *current_index == payload.len() {
                let message = session.recv_and_reply_okay()?;
                message.assert_command(MessageCommand::Write)?;
                *payload = message.into_payload();
                *current_index = 0;
                if payload.is_empty() {
                    empty_packets += 1;
                    if empty_packets > 4 {
                        return Err(RustADBError::ADBRequestFailed(
                            "LIST response made no progress".into(),
                        ));
                    }
                    continue;
                }
                empty_packets = 0;
            }
            let count = (requested_bytes - bytes.len()).min(payload.len() - *current_index);
            bytes.extend_from_slice(&payload[*current_index..*current_index + count]);
            *current_index += count;
        }
        Ok(bytes)
    }

    fn handle_list<A: AsRef<str>>(
        session: &mut ADBSession<T>,
        path: A,
    ) -> Result<Vec<ADBListItemType>> {
        session.mark_incomplete(true);
        // TODO: use LIS2 to support files over 2.14 GB in size.
        // SEE: https://github.com/cstyan/adbDocumentation?tab=readme-ov-file#adb-list
        {
            let mut len_buf = Vec::from([0_u8; 4]);
            LittleEndian::write_u32(&mut len_buf, u32::try_from(path.as_ref().len())?);

            let subcommand_data = MessageSubcommand::List;

            let mut serialized_message = subcommand_data.encode();

            serialized_message.append(&mut len_buf);
            let mut path_bytes: Vec<u8> = Vec::from(path.as_ref().as_bytes());
            serialized_message.append(&mut path_bytes);

            session.send_and_expect_okay(ADBTransportMessage::try_new(
                MessageCommand::Write,
                session.local_id(),
                session.remote_id(),
                &serialized_message,
            )?)?;
            trace_sync("LIST request acknowledged");
        }

        let mut list_items = Vec::new();

        // Acknowledge every WRTE immediately, including the final packet before QUIT.
        let message = session.recv_and_reply_okay()?;
        message.assert_command(MessageCommand::Write)?;
        let mut payload = message.into_payload();
        trace_sync(&format!("LIST initial payload bytes={}", payload.len()));
        let mut current_index = 0;
        loop {
            // Loop though the response for all the entries
            const STATUS_CODE_LENGTH_IN_BYTES: usize = 4;
            let status_code = Self::read_bytes_from_transport(
                session,
                STATUS_CODE_LENGTH_IN_BYTES,
                &mut current_index,
                &mut payload,
            )?;
            match str::from_utf8(&status_code)? {
                "DENT" => {
                    // Read the file mode, size, mod time and name length in one go, since all their sizes are predictable
                    const U32_SIZE_IN_BYTES: usize = 4;
                    const SIZE_OF_METADATA: usize = U32_SIZE_IN_BYTES * 4;
                    let metadata = Self::read_bytes_from_transport(
                        session,
                        SIZE_OF_METADATA,
                        &mut current_index,
                        &mut payload,
                    )?;
                    let mode = metadata[..U32_SIZE_IN_BYTES].to_vec();
                    let size = metadata[U32_SIZE_IN_BYTES..2 * U32_SIZE_IN_BYTES].to_vec();
                    let time = metadata[2 * U32_SIZE_IN_BYTES..3 * U32_SIZE_IN_BYTES].to_vec();
                    let name_len = metadata[3 * U32_SIZE_IN_BYTES..4 * U32_SIZE_IN_BYTES].to_vec();

                    let mode = LittleEndian::read_u32(&mode);
                    let size = LittleEndian::read_u32(&size);
                    let time = LittleEndian::read_u32(&time);
                    let name_len = LittleEndian::read_u32(&name_len) as usize;
                    if name_len > 65536 {
                        return Err(RustADBError::ADBRequestFailed(
                            "LIST filename exceeds limit".into(),
                        ));
                    }
                    // Read the file name, since it requires the length from the name_len
                    let name_buf = Self::read_bytes_from_transport(
                        session,
                        name_len,
                        &mut current_index,
                        &mut payload,
                    )?;
                    // A non-UTF-8 name must not abort the listing (the rest of the folder
                    // would be lost) nor become a mangled path. Keep reading to DONE.
                    let (name, valid_name) = match String::from_utf8(name_buf) {
                        Ok(name) => (name, true),
                        Err(error) => (String::from_utf8_lossy(error.as_bytes()).into_owned(), false),
                    };

                    // First 9 bits are the file permissions
                    let permissions = mode & 0b1_1111_1111;

                    let entry = ADBListItem {
                        name,
                        time,
                        permissions,
                        size,
                    };

                    list_items.push(if valid_name {
                        ADBListItemType::from_mode_and_entry(mode, entry)
                    } else {
                        ADBListItemType::InvalidName(entry)
                    });
                }
                "DONE" => {
                    trace_sync(&format!(
                        "LIST DONE remaining={}",
                        payload.len() - current_index
                    ));
                    Self::read_bytes_from_transport(session, 16, &mut current_index, &mut payload)?;
                    trace_sync("LIST DONE consumed");
                    session.mark_incomplete(false);
                    return Ok(list_items);
                }
                x => {
                    return Err(RustADBError::ADBRequestFailed(format!(
                        "Unknown LIST response: {x}"
                    )));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adb_transport::ADBTransport;
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex},
        time::Duration,
    };
    #[derive(Clone)]
    struct Transport {
        incoming: Arc<Mutex<VecDeque<ADBTransportMessage>>>,
        outgoing: Arc<Mutex<Vec<MessageCommand>>>,
        awaiting_ack: Arc<Mutex<bool>>,
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
            if *self.awaiting_ack.lock().unwrap() {
                return Err(RustADBError::ADBRequestFailed(
                    "Previous LIST packet was not acknowledged".into(),
                ));
            }
            let message = self.incoming.lock().unwrap().pop_front().ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "test EOF")
            })?;
            if message.header().command() == MessageCommand::Write {
                *self.awaiting_ack.lock().unwrap() = true;
            }
            Ok(message)
        }
        fn write_message_with_timeout(
            &mut self,
            message: ADBTransportMessage,
            _: Duration,
        ) -> Result<()> {
            if message.header().command() == MessageCommand::Okay {
                *self.awaiting_ack.lock().unwrap() = false;
            }
            self.outgoing
                .lock()
                .unwrap()
                .push(message.header().command());
            Ok(())
        }
    }
    fn session(chunks: &[&[u8]]) -> (ADBSession<Transport>, Transport) {
        let packet =
            |command, payload| ADBTransportMessage::try_new(command, 2, 1, payload).unwrap();
        let mut incoming = VecDeque::from([packet(MessageCommand::Okay, b"")]);
        incoming.extend(
            chunks
                .iter()
                .map(|bytes| packet(MessageCommand::Write, bytes)),
        );
        incoming.push_back(packet(MessageCommand::Okay, b""));
        incoming.push_back(packet(MessageCommand::Clse, b""));
        let transport = Transport {
            incoming: Arc::new(Mutex::new(incoming)),
            outgoing: Arc::new(Mutex::new(Vec::new())),
            awaiting_ack: Arc::new(Mutex::new(false)),
        };
        (ADBSession::new(transport.clone(), 1, 2), transport)
    }
    #[test]
    fn fragmented_list_acknowledges_final_packet_before_quit() {
        let name = "messages-2027-01-02 (1).zip";
        let mut bytes = b"DENT".to_vec();
        for field in [0o100644u32, 123, 456, name.len() as u32] {
            bytes.extend_from_slice(&field.to_le_bytes());
        }
        bytes.extend_from_slice(name.as_bytes());
        bytes.extend_from_slice(b"DONE");
        bytes.extend_from_slice(&[0; 16]);
        for chunk_size in [1, 3, 17, bytes.len()] {
            let chunks: Vec<_> = bytes.chunks(chunk_size).collect();
            let (mut s, t) = session(&chunks);
            let listed = ADBMessageDevice::<Transport>::handle_list(&mut s, "/sdcard").unwrap();
            assert_eq!(listed.len(), 1);
            match &listed[0] {
                ADBListItemType::File(entry) => assert_eq!(entry.name, name),
                _ => panic!("not a file"),
            }
            assert!(!*t.awaiting_ack.lock().unwrap());
            s.send_and_expect_okay(
                ADBTransportMessage::try_new(
                    MessageCommand::Write,
                    1,
                    2,
                    &MessageSubcommand::Quit.with_arg(0).encode(),
                )
                .unwrap(),
            )
            .unwrap();
            s.recv_and_reply_okay()
                .unwrap()
                .assert_command(MessageCommand::Clse)
                .unwrap();
            let sent = t.outgoing.lock().unwrap();
            assert_eq!(
                sent.iter()
                    .filter(|cmd| **cmd == MessageCommand::Okay)
                    .count(),
                chunks.len()
            );
            assert_eq!(sent.last(), Some(&MessageCommand::Clse));
            assert!(t.incoming.lock().unwrap().is_empty());
        }
    }
    #[test]
    fn truncated_or_unknown_list_response_fails_without_panicking() {
        for payload in [b"DONE".as_slice(), b"FAILbad"] {
            let (mut s, _) = session(&[payload]);
            assert!(ADBMessageDevice::<Transport>::handle_list(&mut s, "/sdcard").is_err());
        }
    }
    #[test]
    fn malformed_list_aborts_and_drains_pending_frames_without_sync_quit() {
        let (mut s,t)=session(&[b"BAD!",b"pending bytes"]);
        assert!(ADBMessageDevice::<Transport>::handle_list(&mut s,"/sdcard").is_err());
        s.close_sync().unwrap();
        let sent=t.outgoing.lock().unwrap();
        assert_eq!(sent.iter().filter(|c|**c==MessageCommand::Write).count(),1);
        assert!(sent.contains(&MessageCommand::Clse));
        assert!(t.incoming.lock().unwrap().is_empty());
    }
    #[test]
    fn invalid_utf8_name_is_flagged_and_the_rest_of_the_folder_is_listed() {
        let mut bytes = Vec::new();
        for name in [b"bad-\xff\xfe.bin".as_slice(), b"good.txt"] {
            bytes.extend_from_slice(b"DENT");
            for field in [0o100644u32, 7, 9, name.len() as u32] {
                bytes.extend_from_slice(&field.to_le_bytes());
            }
            bytes.extend_from_slice(name);
        }
        bytes.extend_from_slice(b"DONE");
        bytes.extend_from_slice(&[0; 16]);
        let chunks: Vec<_> = bytes.chunks(5).collect();
        let (mut s, t) = session(&chunks);
        let listed = ADBMessageDevice::<Transport>::handle_list(&mut s, "/sdcard").unwrap();
        assert_eq!(listed.len(), 2);
        assert!(matches!(&listed[0], ADBListItemType::InvalidName(e) if e.name.starts_with("bad-")));
        assert!(matches!(&listed[1], ADBListItemType::File(e) if e.name == "good.txt"));
        // DONE까지 읽었으므로 세션은 정상 상태 — 중단 없이 모든 WRTE에 응답했다
        assert!(!*t.awaiting_ack.lock().unwrap());
    }
}
