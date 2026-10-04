//! DIAG HDLC wire compatibility adapted from JohnBel/EfsTools (MIT; see NOTICE).
use super::error::{Error, Result};
pub const MAX_PAYLOAD: usize = 8192;

pub fn crc16(data: &[u8]) -> u16 {
    let mut crc = 0xffffu16;
    for &b in data {
        crc ^= b as u16;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0x8408
            } else {
                crc >> 1
            };
        }
    }
    !crc
}
pub fn encode(data: &[u8], leading_delimiter: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() * 2 + 5);
    if leading_delimiter {
        out.push(0x7e);
    }
    for b in data.iter().copied().chain(crc16(data).to_le_bytes()) {
        if b == 0x7d || b == 0x7e {
            out.extend([0x7d, b ^ 0x20]);
        } else {
            out.push(b);
        }
    }
    out.push(0x7e);
    out
}
#[derive(Default)]
pub struct Decoder {
    bytes: Vec<u8>,
    escaped: bool,
    discard: bool,
}
impl Decoder {
    // Produces every complete frame; fragments and an escape at a read boundary survive.
    pub fn feed(&mut self, input: &[u8]) -> Vec<Result<Vec<u8>>> {
        let mut frames = vec![];
        for &b in input {
            if b == 0x7e {
                if self.discard {
                    self.discard = false;
                } else if self.escaped {
                    frames.push(Err(Error::new("framing", "HDLC", "Dangling escape")));
                } else if !self.bytes.is_empty() {
                    let n = self.bytes.len();
                    if n < 3 {
                        frames.push(Err(Error::new("framing", "HDLC", "Short frame")));
                    } else if crc16(&self.bytes[..n - 2]).to_le_bytes() != self.bytes[n - 2..] {
                        frames.push(Err(Error::new("crc", "HDLC", "Invalid CRC16")));
                    } else {
                        frames.push(Ok(self.bytes[..n - 2].to_vec()));
                    }
                }
                self.bytes.clear();
                self.escaped = false;
            } else if !self.discard {
                if self.escaped {
                    self.bytes.push(b ^ 0x20);
                    self.escaped = false;
                } else if b == 0x7d {
                    self.escaped = true;
                } else {
                    self.bytes.push(b);
                }
                if self.bytes.len() > MAX_PAYLOAD + 2 {
                    self.bytes.clear();
                    self.escaped = false;
                    self.discard = true;
                    frames.push(Err(Error::new("oversized", "HDLC", "Frame exceeds bound")));
                }
            }
        }
        frames
    }
}
