use std::time::Duration;

use crate::{
    Result, adb_transport::ADBTransport,
    message_devices::adb_transport_message::ADBTransportMessage,
};

// [xvolte patch] 상한 없는 읽기(u64::MAX초)는 기기가 응답 없이 멈추면 스레드와 연결 잠금을 영원히 잡는다.
// 300초: 출력 없이 오래 도는 명령(boot_patch.sh, 저장소 du, 대용량 pm install-commit의 dexopt)보다 길고,
// exec stdin 전송 중에는 기기가 WRTE마다 OKAY를 보내므로 읽기 스레드가 유휴 상태가 되지 않는다.
const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(300);
// [xvolte patch] 2초 → 30초: 기기 저장소가 느려 adbd가 잠시 USB 읽기를 멈추면(복원 tar 스트림)
// 512바이트 bulk 쓰기 하나가 2초를 넘을 수 있다. 여전히 유한하므로 멈춘 기기에서 영원히 막히지 않는다.
const DEFAULT_WRITE_TIMEOUT: Duration = Duration::from_secs(30);

/// Trait representing a transport able to read and write messages.
pub trait ADBMessageTransport: ADBTransport + Clone + Send + 'static {
    /// An upgrade of the connection has been asked by the device.
    /// Some transports may not need this feature, a blanket implementation is provided as default implementation.
    fn upgrade_connection(&mut self) -> Result<()> {
        log::trace!("not upgrade needed for this transport");
        Ok(())
    }

    /// Read a message using given timeout on the underlying transport
    fn read_message_with_timeout(&mut self, read_timeout: Duration) -> Result<ADBTransportMessage>;

    /// Read data to underlying connection, using default timeout
    fn read_message(&mut self) -> Result<ADBTransportMessage> {
        self.read_message_with_timeout(DEFAULT_READ_TIMEOUT)
    }

    /// Write a message using given timeout on the underlying transport
    fn write_message_with_timeout(
        &mut self,
        message: ADBTransportMessage,
        write_timeout: Duration,
    ) -> Result<()>;

    /// Write data to underlying connection, using default timeout
    fn write_message(&mut self, message: ADBTransportMessage) -> Result<()> {
        self.write_message_with_timeout(message, DEFAULT_WRITE_TIMEOUT)
    }
}
