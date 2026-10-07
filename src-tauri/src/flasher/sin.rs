//! Bounded streaming index for raw/gzip SIN TARs; no filesystem extraction.
use super::{error, policy, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom};

pub const MAX_MEMBER: u64 = 1024 * 1024 * 1024;
const MAX_SIGNATURE: u64 = 16 * 1024 * 1024;
const MAX_EXPANDED: u64 = 128 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    pub name: String,
    pub bytes: u64,
    pub sha256: String,
}
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SinIndex {
    pub partition: String,
    pub compressed: bool,
    pub members: Vec<Member>,
}

pub fn decoded<'a, R: Read + Seek + 'a>(reader: &'a mut R) -> Result<(bool, Box<dyn Read + 'a>)> {
    let mut magic = [0u8; 2];
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|_| error("READ", "Cannot seek SIN"))?;
    reader
        .read_exact(&mut magic)
        .map_err(|_| error("SIN", "SIN is truncated"))?;
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|_| error("READ", "Cannot seek SIN"))?;
    let compressed = magic == [0x1f, 0x8b];
    let stream: Box<dyn Read> = if compressed {
        Box::new(flate2::read::MultiGzDecoder::new(reader))
    } else {
        Box::new(reader)
    };
    Ok((compressed, stream))
}

pub fn inspect<R: Read + Seek>(reader: &mut R) -> Result<SinIndex> {
    let (compressed, stream) = decoded(reader)?;
    let mut archive = tar::Archive::new(stream.take(MAX_EXPANDED));
    let mut members = Vec::new();
    let mut partition = String::new();
    for entry in archive
        .entries()
        .map_err(|_| error("SIN", "Cannot read TAR entries"))?
        .raw(true)
    {
        let mut entry = entry.map_err(|_| error("SIN", "Invalid TAR header or checksum"))?;
        if !entry.header().entry_type().is_file() {
            return Err(error("SIN_TYPE", "Only regular SIN members are supported"));
        }
        let name = std::str::from_utf8(&entry.path_bytes())
            .map_err(|_| error("SIN_PATH", "Non-UTF8 SIN member"))?
            .to_owned();
        policy::safe_relative(&name)?;
        if name.contains('/') {
            return Err(error("SIN_PATH", "SIN members must be basenames"));
        }
        let (stem, extension) = name
            .rsplit_once('.')
            .ok_or_else(|| error("SIN", "Missing SIN member extension"))?;
        policy::partition_name(stem)?;
        if members.is_empty() {
            if extension != "cms" {
                return Err(error(
                    "SIN_SIGNATURE",
                    "First SIN member must be CMS signature",
                ));
            }
            partition = stem.into();
        } else if stem != partition
            || extension != format!("{:03}", members.len() - 1)
            || members.len() > 1000
        {
            return Err(error(
                "SIN_ORDER",
                "Missing, duplicate or out-of-order SIN chunk",
            ));
        }
        let bytes = entry.size();
        let limit = if members.is_empty() {
            MAX_SIGNATURE
        } else {
            MAX_MEMBER
        };
        if bytes == 0 || bytes > limit {
            return Err(error("SIN_SIZE", "SIN member size outside allowed bounds"));
        }
        let mut hash = Sha256::new();
        let mut total = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let n = entry
                .read(&mut buffer)
                .map_err(|_| error("SIN_READ", "Truncated or corrupt SIN data"))?;
            if n == 0 {
                break;
            }
            total += n as u64;
            hash.update(&buffer[..n]);
        }
        if total != bytes {
            return Err(error("SIN_READ", "SIN member length mismatch"));
        }
        members.push(Member {
            name,
            bytes,
            sha256: hex::encode(hash.finalize()),
        });
    }
    if members.len() < 2 {
        return Err(error("SIN", "SIN needs signature and image chunks"));
    }
    // Drain the decoder to check gzip CRC/trailer, and reject hidden TAR entries after end markers.
    let mut tail = archive.into_inner();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = tail
            .read(&mut buffer)
            .map_err(|_| error("SIN_READ", "Invalid gzip trailer or TAR ending"))?;
        if n == 0 {
            break;
        }
        if buffer[..n].iter().any(|&b| b != 0) {
            return Err(error("SIN_TRAILING", "Unexpected data after TAR ending"));
        }
    }
    if tail.limit() == 0 {
        return Err(error("SIN_SIZE", "Expanded SIN size limit reached"));
    }
    Ok(SinIndex {
        partition,
        compressed,
        members,
    })
}
