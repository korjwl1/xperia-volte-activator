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
        let v2 = self.list_v2;
        let mut session = self.take_sync_session()?;

        let output = Self::handle_list_with(&mut session, path, v2);

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

    #[cfg(test)]
    fn handle_list<A: AsRef<str>>(
        session: &mut ADBSession<T>,
        path: A,
    ) -> Result<Vec<ADBListItemType>> {
        Self::handle_list_with(session, path, false)
    }

    /// [xvolte patch] LIST v1 reports size/mtime as u32, so files over 4 GiB arrive truncated.
    /// When the device advertises `ls_v2` (Android 11+), use LIS2: DNT2 entries carry a 64-bit
    /// size and 64-bit times. Wire layout (AOSP file_sync_protocol.h, dent_v2 after the 4-byte id):
    /// error u32, dev u64, ino u64, mode u32, nlink u32, uid u32, gid u32, size u64,
    /// atime i64, mtime i64, ctime i64, namelen u32 (72 bytes) + name. DONE carries the same 72 bytes.
    fn handle_list_with<A: AsRef<str>>(
        session: &mut ADBSession<T>,
        path: A,
        v2: bool,
    ) -> Result<Vec<ADBListItemType>> {
        session.mark_incomplete(true);
        // SEE: https://github.com/cstyan/adbDocumentation?tab=readme-ov-file#adb-list
        {
            let mut len_buf = Vec::from([0_u8; 4]);
            LittleEndian::write_u32(&mut len_buf, u32::try_from(path.as_ref().len())?);

            let subcommand_data = if v2 { MessageSubcommand::List2 } else { MessageSubcommand::List };

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
            const V1_METADATA: usize = 16; // mode u32, size u32, time u32, namelen u32
            const V2_METADATA: usize = 72;
            let entry_id = if v2 { "DNT2" } else { "DENT" };
            let metadata_len = if v2 { V2_METADATA } else { V1_METADATA };
            match str::from_utf8(&status_code)? {
                id if id == entry_id => {
                    let metadata = Self::read_bytes_from_transport(
                        session,
                        metadata_len,
                        &mut current_index,
                        &mut payload,
                    )?;
                    let (mode, size, time, name_len) = if v2 {
                        // error(0..4) dev(4..12) ino(12..20) mode(20..24) nlink(24..28) uid(28..32)
                        // gid(32..36) size(36..44) atime(44..52) mtime(52..60) ctime(60..68) namelen(68..72)
                        let mtime = LittleEndian::read_i64(&metadata[52..60]);
                        (
                            LittleEndian::read_u32(&metadata[20..24]),
                            LittleEndian::read_u64(&metadata[36..44]),
                            u32::try_from(mtime.max(0)).unwrap_or(u32::MAX),
                            LittleEndian::read_u32(&metadata[68..72]) as usize,
                        )
                    } else {
                        (
                            LittleEndian::read_u32(&metadata[0..4]),
                            u64::from(LittleEndian::read_u32(&metadata[4..8])),
                            LittleEndian::read_u32(&metadata[8..12]),
                            LittleEndian::read_u32(&metadata[12..16]) as usize,
                        )
                    };
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
                    Self::read_bytes_from_transport(session, metadata_len, &mut current_index, &mut payload)?;
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
    fn lis2_reports_64_bit_sizes_and_times_across_fragmented_frames() {
        // 23,347,495,814 B 동영상 — LIST v1에서는 하위 32비트만 와서 매번 다시 받던 크기
        let size: u64 = 23_347_495_814;
        let name = b"VideoPro_20240928_194145.mp4";
        let mut bytes = b"DNT2".to_vec();
        let mut meta = vec![0u8; 72];
        meta[20..24].copy_from_slice(&0o100660u32.to_le_bytes());
        meta[36..44].copy_from_slice(&size.to_le_bytes());
        meta[52..60].copy_from_slice(&1_727_520_105i64.to_le_bytes());
        meta[68..72].copy_from_slice(&(name.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&meta);
        bytes.extend_from_slice(name);
        bytes.extend_from_slice(b"DONE");
        bytes.extend_from_slice(&[0; 72]);
        for chunk in [7, 33, bytes.len()] {
            let chunks: Vec<_> = bytes.chunks(chunk).collect();
            let (mut s, t) = session(&chunks);
            let listed = ADBMessageDevice::<Transport>::handle_list_with(&mut s, "/sdcard/DCIM", true).unwrap();
            assert_eq!(listed.len(), 1);
            match &listed[0] {
                ADBListItemType::File(e) => {
                    assert_eq!(e.size, size);
                    assert_eq!(e.time, 1_727_520_105);
                    assert_eq!(e.name, "VideoPro_20240928_194145.mp4");
                }
                other => panic!("{other}"),
            }
            assert!(!*t.awaiting_ack.lock().unwrap());
        }
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
