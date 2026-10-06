use std::{fs::File, path::Path};

use crate::{
    Result,
    message_devices::{
        adb_message_device::ADBMessageDevice, adb_message_transport::ADBMessageTransport,
        adb_session::ADBSession, adb_transport_message::ADBTransportMessage,
        commands::utils::MessageWriter, message_commands::MessageCommand,
    },
    models::ADBLocalCommand,
    utils::check_extension_is_apk,
};

impl<T: ADBMessageTransport> ADBMessageDevice<T> {
    pub(crate) fn install(&mut self, apk_path: &dyn AsRef<Path>, user: Option<&str>) -> Result<()> {
        let mut apk_file = File::open(apk_path)?;

        check_extension_is_apk(apk_path)?;

        let file_size = apk_file.metadata()?.len();

        let mut session = self.open_session(&ADBLocalCommand::Install(
            file_size,
            user.map(ToString::to_string),
        ))?;

        {
            // Read data from apk_file and write it to the underlying session
            let mut writer = MessageWriter::new(&mut session);
            std::io::copy(&mut apk_file, &mut writer)?;
        }

        read_install_status(&mut session)?;
        log::info!(
            "APK file {} successfully installed",
            apk_path.as_ref().display()
        );
        Ok(())
    }
}

// [xvolte patch] An empty OKAY is transport flow control, not the install result.
// Android may split stdout across WRTE packets. Acknowledge each packet and consume
// CLSE before returning, including on a reported install failure.
fn read_install_status<T: ADBMessageTransport>(session: &mut ADBSession<T>) -> Result<()> {
    let mut output = Vec::new();
    loop {
        let message = session.get_transport_mut().read_message()?;
        match message.header().command() {
            MessageCommand::Okay => continue,
            command @ (MessageCommand::Write | MessageCommand::Clse) => {
                let reply = ADBTransportMessage::try_new(
                    if command == MessageCommand::Write {
                        MessageCommand::Okay
                    } else {
                        MessageCommand::Clse
                    },
                    session.local_id(),
                    session.remote_id(),
                    &[],
                )?;
                session.get_transport_mut().write_message(reply)?;
                if command == MessageCommand::Clse {
                    break;
                }
                output.extend_from_slice(&message.into_payload());
                if output.len() > 65536 {
                    return Err(crate::RustADBError::ADBRequestFailed(
                        "Install output exceeded 64 KiB".into(),
                    ));
                }
            }
            command => {
                return Err(crate::RustADBError::ADBRequestFailed(format!(
                    "Unexpected install response: {command}"
                )));
            }
        }
    }
    if output == b"Success\n" {
        Ok(())
    } else {
        Err(crate::RustADBError::ADBRequestFailed(String::from_utf8(
            output,
        )?))
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
            self.outgoing
                .lock()
                .unwrap()
                .push(message.header().command());
            Ok(())
        }
    }
    fn session(packets: &[(MessageCommand, &[u8])]) -> (ADBSession<Transport>, Transport) {
        let transport = Transport {
            incoming: Arc::new(Mutex::new(
                packets
                    .iter()
                    .map(|(command, payload)| {
                        ADBTransportMessage::try_new(*command, 2, 1, payload).unwrap()
                    })
                    .collect(),
            )),
            outgoing: Arc::new(Mutex::new(Vec::new())),
        };
        (ADBSession::new(transport.clone(), 1, 2), transport)
    }
    #[test]
    fn install_status_handles_flow_control_split_output_and_close() {
        let (mut session, transport) = session(&[
            (MessageCommand::Okay, b""),
            (MessageCommand::Write, b"Suc"),
            (MessageCommand::Write, b"cess\n"),
            (MessageCommand::Clse, b""),
        ]);
        read_install_status(&mut session).unwrap();
        assert!(transport.incoming.lock().unwrap().is_empty());
        assert_eq!(
            *transport.outgoing.lock().unwrap(),
            vec![
                MessageCommand::Okay,
                MessageCommand::Okay,
                MessageCommand::Clse
            ]
        );
    }
    #[test]
    fn failed_install_is_drained_and_missing_or_truncated_success_is_rejected() {
        for packets in [
            vec![
                (MessageCommand::Write, b"Failure [DENIED]\n".as_slice()),
                (MessageCommand::Clse, b"".as_slice()),
            ],
            vec![(MessageCommand::Clse, b"".as_slice())],
            vec![(MessageCommand::Write, b"Success\n".as_slice())],
        ] {
            let (mut session, transport) = session(&packets);
            assert!(read_install_status(&mut session).is_err());
            assert!(transport.incoming.lock().unwrap().is_empty());
        }
    }
}
