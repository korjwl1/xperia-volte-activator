use super::Result;
use std::time::Duration;

#[cfg(windows)]
// Compiled and tested for path filtering, but no driver/device operations are exposed yet.
#[allow(dead_code)]
pub mod windows;

/// Control reads preserve device response boundaries; bulk OUT can complete partially.
/// The implementor must stop/wait outstanding OS I/O before returning an error.
pub trait FlashTransport {
    fn write(&mut self, data: &[u8], timeout: Duration) -> Result<usize>;
    fn read_response(&mut self, limit: usize, timeout: Duration) -> Result<Vec<u8>>;
}
