use std::io::{Read, Write};

use crate::models::ADBLocalCommand;
use crate::{
    Result, RustADBError,
    message_devices::{
        adb_message_device::ADBMessageDevice, adb_message_transport::ADBMessageTransport,
        adb_transport_message::ADBTransportMessage,
        message_commands::MessageCommand,
    },
};

impl<T: ADBMessageTransport> ADBMessageDevice<T> {
    /// Runs 'command' in a shell on the device, and write its output and error streams into output.
    pub(crate) fn shell_command(
        &mut self,
        command: &dyn AsRef<str>,
        mut stdout: Option<&mut dyn Write>,
        _stderr: Option<&mut dyn Write>,
    ) -> Result<Option<u8>> {
        let mut session = self.open_session(&ADBLocalCommand::ShellCommand(
            command.as_ref().to_string(),
            Vec::new(),
        ))?;

        let result=(|| {
        loop {
            let message = session.recv_and_reply_okay()?;
            if message.header().command() == MessageCommand::Clse {
                break;
            }
            // should this just write for ::Write messages?
            if let Some(ref mut stdout) = stdout {
                stdout.write_all(&message.into_payload())?;
            }
        }

        Ok(None)
        })();
        if result.is_err() {
            session.mark_incomplete(true);
            if session.close_sync().is_err() {self.sync_broken=Some("SYNC_BATCH_BROKEN|Shell stream failed; reconnect required".into());}
        }
        result
    }

    /// Starts an interactive shell session on the device.
    /// Input data is read from [reader] and write to [writer].
    pub(crate) fn shell(
        &mut self,
        reader: &mut dyn Read,
        writer: Box<dyn Write + Send>,
    ) -> Result<()> {
        self.bidirectional_session(&ADBLocalCommand::Shell, reader, writer)
    }

    /// Runs `command` on the device.
    /// Input data is read from [reader] and write to [writer].
    pub(crate) fn exec(
        &mut self,
        command: &str,
        reader: &mut dyn Read,
        writer: Box<dyn Write + Send>,
    ) -> Result<()> {
        self.bidirectional_session(&ADBLocalCommand::Exec(command.to_string()), reader, writer)
    }

    /// Starts an bidirectional(interactive) session. This can be a shell or an exec session.
    fn bidirectional_session(
        &mut self,
        local_command: &ADBLocalCommand,
        reader: &mut dyn Read,
        mut writer: Box<dyn Write + Send>,
    ) -> Result<()> {
        let mut session = self.open_session(local_command)?;
        let result=(|| {
            let mut buffer=vec![0;64*1024];
            let mut diagnostic=Vec::<u8>::new();
            loop {
                let count=reader.read(&mut buffer)?;
                if count==0 {break;}
                let message=ADBTransportMessage::try_new(MessageCommand::Write,session.local_id(),session.remote_id(),&buffer[..count])?;
                session.get_transport_mut().write_message(message)?;
                loop {
                    let message=session.recv_and_reply_okay()?;
                    match message.header().command() {
                        MessageCommand::Okay=>break,
                        MessageCommand::Write=>{let payload=message.into_payload(); diagnostic.extend_from_slice(&payload);if diagnostic.len()>8192 {diagnostic.drain(..diagnostic.len()-8192);}writer.write_all(&payload)?;writer.flush()?;},
                        MessageCommand::Clse=>return Err(RustADBError::ADBRequestFailed(format!("Service closed before input completed: {}",String::from_utf8_lossy(&diagnostic).trim()))),
                        _=>unreachable!(),
                    }
                }
            }
            if matches!(local_command,ADBLocalCommand::Shell) {
                session.mark_incomplete(true);
                return session.close_sync();
            }
            loop {
                let message=session.recv_and_reply_okay()?;
                match message.header().command() {
                    MessageCommand::Clse=>return Ok(()),
                    MessageCommand::Write=>{let payload=message.into_payload(); diagnostic.extend_from_slice(&payload);if diagnostic.len()>8192 {diagnostic.drain(..diagnostic.len()-8192);}writer.write_all(&payload)?;writer.flush()?;},
                    MessageCommand::Okay=>{},
                    _=>unreachable!(),
                }
            }
        })();
        if result.is_err() {
            session.mark_incomplete(true);
            if session.close_sync().is_err() {self.sync_broken=Some("SYNC_BATCH_BROKEN|Exec stream failed; reconnect required".into());}
        }
        result
    }
}
