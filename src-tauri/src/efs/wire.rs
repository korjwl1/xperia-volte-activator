//! Request layouts adapted from JohnBel/EfsTools (MIT). Decimal C# flags are deliberate.
use super::error::{Error, Result};
pub const WRITE_CREATE: u32 = 1 | 100;
pub const ITEM_FLAGS: u32 = 1 | 1000 | 100 | 1_000_000;
pub const DEFAULT_MODE: u32 = 777;
pub const DIRECTORY_EXISTS: u32 = 6; // QcdmEfsErrors.DirectoryExist, NOT FileExist=17.
/// Sizes measured by the offline C# ItemsFactory, not guessed from IDs.
pub fn nv_logical_size(id: u16) -> usize {
    match id {
        10 => 3,
        1897 => 12,
        3006 => 2,
        5280 => 8,
        1918 | 1920 | 1921 | 6792 | 7165 => 4,
        562 | 880 | 881 | 882 | 909 | 912 | 1016 | 1017 | 1030 | 1031 | 1896 | 3461 | 3532
        | 3533 | 3628 | 3851 | 4118 | 4210 | 4228 | 4229 | 4261 | 4432 | 4722 | 5596 | 5895
        | 6789 | 6831 | 6849 | 6850 | 6862 => 1,
        _ => 0,
    }
}
pub fn normalize_nv(id: u16, raw: &[u8], expected_len: usize) -> Result<Vec<u8>> {
    let known = nv_logical_size(id);
    if raw.len() != 128
        || expected_len == 0
        || expected_len > 128
        || (known > 0 && expected_len != known)
    {
        return Err(Error::new(
            "invalidNvLength",
            "NV verification",
            "Invalid raw or logical length; empty NV cannot prove a mutation",
        ));
    }
    // Unknown items compare only the explicitly supplied prefix (warning).
    // Never synthesize zeros or infer the unspecified tail.
    Ok(raw[..expected_len].to_vec())
}
pub fn log_ranges() -> Vec<u8> {
    vec![0x73, 0, 0, 0, 1, 0, 0, 0]
}
pub fn log_mask(scope: u32, start: u32, end: u32) -> Result<Vec<u8>> {
    if end < start || end - start > 16384 || scope > 16 {
        return Err(Error::new("limit", "log mask", "Invalid log range"));
    }
    let delta = scope * 0x1000;
    let begin = if start < delta { start } else { start - delta };
    let finish = if end < delta { end } else { end - delta };
    let mut v = vec![0; ((end - start) / 8 + 16) as usize];
    v[0] = 0x73;
    v[4] = 3;
    v[8] = scope as u8;
    v[10..12].copy_from_slice(&(begin as u16).to_le_bytes());
    v[12..14].copy_from_slice(&(finish as u16).to_le_bytes());
    Ok(v)
}
pub fn message_mask(start: u16, end: u16) -> Result<Vec<u8>> {
    if end < start || end - start > 1024 {
        return Err(Error::new("limit", "message mask", "Invalid message range"));
    }
    let mut v = vec![0; (end as usize - start as usize + 1) * 4 + 11];
    v[0] = 0x7d;
    v[1] = 4;
    v[2..4].copy_from_slice(&start.to_le_bytes());
    v[4..6].copy_from_slice(&end.to_le_bytes());
    Ok(v)
}
pub fn u16_at(data: &[u8], at: usize) -> Result<u16> {
    let s = data
        .get(at..at + 2)
        .ok_or_else(|| Error::new("malformed", "decode", "Truncated u16"))?;
    Ok(u16::from_le_bytes([s[0], s[1]]))
}
pub fn u32_at(data: &[u8], at: usize) -> Result<u32> {
    let s = data
        .get(at..at + 4)
        .ok_or_else(|| Error::new("malformed", "decode", "Truncated u32"))?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}
pub fn efs(op: u16) -> Vec<u8> {
    let mut v = vec![0x4b, 19];
    v.extend(op.to_le_bytes());
    v
}
pub fn words(op: u16, values: &[u32]) -> Vec<u8> {
    let mut v = efs(op);
    for value in values {
        v.extend(value.to_le_bytes());
    }
    v
}
pub fn path(v: &mut Vec<u8>, p: &str) -> Result<()> {
    if !p.is_ascii() || p.bytes().any(|b| b == 0 || b < 32) || p.len() > 1024 {
        return Err(Error::new(
            "invalidPath",
            "encode",
            "EFS paths must be bounded printable ASCII",
        ));
    }
    v.extend(p.as_bytes());
    v.push(0);
    Ok(())
}
pub fn path_request(op: u16, p: &str) -> Result<Vec<u8>> {
    let mut v = efs(op);
    path(&mut v, p)?;
    Ok(v)
}
pub fn hello() -> Vec<u8> {
    words(
        0,
        &[
            0x100000, 0x100000, 0x100000, 0x100000, 0x100000, 0x100000, 1, 1, 1, 0xffffffff,
        ],
    )
}
pub fn open(p: &str, flags: u32, mode: u32) -> Result<Vec<u8>> {
    let mut v = words(2, &[flags, mode]);
    path(&mut v, p)?;
    Ok(v)
}
pub fn put(p: &str, mode: u32, data: &[u8]) -> Result<Vec<u8>> {
    let mut check = vec![];
    path(&mut check, p)?;
    if data.len() > 2048 || mode > u16::MAX as u32 {
        return Err(Error::new(
            "invalidItem",
            "PUT",
            "Item exceeds 2048 bytes or mode exceeds u16",
        ));
    }
    // Exact upstream allocation: len u16 + two zero bytes, flags u32, mode u16,
    // data at 14 (overwrites high mode bytes), path + NUL, ten trailing zeros.
    let mut v = efs(38);
    v.extend((data.len() as u32).to_le_bytes());
    v.extend(ITEM_FLAGS.to_le_bytes());
    v.extend((mode as u16).to_le_bytes());
    v.extend(data);
    v.extend(check);
    v.resize(25 + p.len() + data.len(), 0);
    Ok(v)
}
pub fn nv_read(id: u16) -> Vec<u8> {
    let mut v = vec![0x26];
    v.extend(id.to_le_bytes());
    v.resize(131, 0);
    v
}
pub fn nv_write(id: u16, data: &[u8]) -> Result<Vec<u8>> {
    if data.len() > 128 {
        return Err(Error::new(
            "invalidNvLength",
            "NV write",
            "Payload exceeds 128 bytes",
        ));
    }
    let mut v = vec![0x27];
    v.extend(id.to_le_bytes());
    v.extend(data);
    Ok(v)
}
pub fn status(data: &[u8], at: usize, op: &str) -> Result<()> {
    let status = u32_at(data, at)?;
    if status == 0 {
        Ok(())
    } else {
        Err(Error::status(op, status))
    }
}
pub fn nv_response(data: &[u8], op: &str) -> Result<Vec<u8>> {
    if data.len() != 133 {
        return Err(Error::new(
            "malformed",
            op,
            "NV response must contain ID, 128 data bytes and u16 status",
        ));
    }
    let status = u16_at(data, 131)?;
    if status != 0 {
        return Err(Error::status(op, status as u32));
    }
    Ok(data[3..131].to_vec())
}
