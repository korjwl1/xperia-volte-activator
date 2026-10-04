use super::error::{Error, Result};
use std::{
    io::{Read, Write},
    time::Duration,
};
pub struct Com {
    port: Box<dyn serialport::SerialPort>,
}
impl Com {
    pub fn open(name: &str) -> Result<Self> {
        // Explicit selection only. Never guess a DIAG port or open another phone.
        super::config::validate_port(name)?;
        let port = serialport::new(name, 38400)
            .data_bits(serialport::DataBits::Eight)
            .parity(serialport::Parity::None)
            .stop_bits(serialport::StopBits::One)
            .flow_control(serialport::FlowControl::None)
            .timeout(Duration::from_millis(50))
            .open()
            .map_err(|e| Error::io("COM open", e))?;
        port.clear(serialport::ClearBuffer::All)
            .map_err(|e| Error::io("COM clear", e))?;
        Ok(Self { port })
    }
}
impl Read for Com {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        self.port.read(b)
    }
}
impl Write for Com {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.port.write(b)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.port.flush()
    }
}
