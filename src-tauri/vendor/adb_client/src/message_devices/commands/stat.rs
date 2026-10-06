use crate::{
    AdbStatResponse, Result,
    message_devices::{
        adb_message_device::ADBMessageDevice, adb_message_transport::ADBMessageTransport,
    },
};

impl<T: ADBMessageTransport> ADBMessageDevice<T> {
    pub(crate) fn stat(&mut self, remote_path: &dyn AsRef<str>) -> Result<AdbStatResponse> {
        let mut session = self.take_sync_session()?;
        let result = session.stat_with_explicit_ids(remote_path.as_ref());
        self.finish_sync_request(session,result)
    }
}
