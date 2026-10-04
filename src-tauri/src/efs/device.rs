use super::{
    error::{Error, Result},
    manifest::{safe_remote, MAX_FILE},
    session::{Session, Transport},
    wire,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    pub mode: u32,
    pub size: u32,
    pub entry_type: u32,
    pub atime: u32,
    pub mtime: u32,
    pub ctime: u32,
}
#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: String,
    pub metadata: Metadata,
}
impl<T: Transport> Session<T> {
    pub fn query(&mut self) -> Result<Vec<u32>> {
        let r = self.request(&wire::efs(1))?;
        if r.len() != 28 {
            return Err(Error::new("malformed", "query", "Invalid query length"));
        }
        (4..28).step_by(4).map(|at| wire::u32_at(&r, at)).collect()
    }
    pub fn stat(&mut self, path: &str) -> Result<Metadata> {
        let r = self.request(&wire::path_request(15, path)?)?;
        wire::status(&r, 4, "stat")?;
        Ok(Metadata {
            mode: wire::u32_at(&r, 8)?,
            size: wire::u32_at(&r, 12)?,
            entry_type: 0,
            atime: wire::u32_at(&r, 20)?,
            mtime: wire::u32_at(&r, 24)?,
            ctime: wire::u32_at(&r, 28)?,
        })
    }
    pub fn read_file(&mut self, path: &str) -> Result<Vec<u8>> {
        let metadata = self.stat(path)?;
        if metadata.size as usize > MAX_FILE {
            return Err(Error::new("limit", "read", "File exceeds 16 MiB"));
        }
        let fd = self.open(path, 0, 0)?;
        let result = (|| {
            let mut data = Vec::with_capacity(metadata.size as usize);
            while data.len() < metadata.size as usize {
                let count = (metadata.size as usize - data.len()).min(1024);
                let r = self.request(&wire::words(4, &[fd, count as u32, data.len() as u32]))?;
                wire::status(&r, 16, "read")?;
                let n = wire::u32_at(&r, 12)? as usize;
                if n == 0 || n > count || r.len() != 20 + n {
                    return Err(Error::new(
                        "malformed",
                        "read",
                        "Premature EOF or invalid read count",
                    ));
                }
                data.extend(&r[20..]);
            }
            let after = self.stat(path)?;
            if after.size != metadata.size || after.mtime != metadata.mtime {
                return Err(Error::new(
                    "changedDuringRead",
                    "read",
                    "File changed during readback",
                ));
            }
            Ok(data)
        })();
        let close = self.close(fd, false);
        combine(result, close)
    }
    pub fn unlink(&mut self, path: &str) -> Result<()> {
        let r = self.request(&wire::path_request(8, path)?)?;
        wire::status(&r, 4, "unlink")
    }
    pub fn mkdir(&mut self, path: &str, mode: u16) -> Result<()> {
        let mut req = wire::efs(9);
        req.extend(mode.to_le_bytes());
        wire::path(&mut req, path)?;
        let r = self.request(&req)?;
        match wire::status(&r, 4, "mkdir") {
            Err(e) if e.status == Some(wire::DIRECTORY_EXISTS) => Ok(()),
            r => r,
        }
    }
    pub fn sync(&mut self, path: &str) -> Result<()> {
        let mut req = wire::efs(48);
        req.extend(self.next_sync_sequence().to_le_bytes());
        wire::path(&mut req, path)?;
        let r = self.request(&req)?;
        wire::status(&r, 10, "sync")
    }
    pub fn write_file(
        &mut self,
        path: &str,
        mode: u32,
        entry_type: u32,
        data: &[u8],
    ) -> Result<()> {
        safe_remote(path)?;
        if data.len() > MAX_FILE || !matches!(entry_type, 0 | 15) {
            return Err(Error::new(
                "invalidItem",
                "write",
                "Unsupported entry type or size",
            ));
        }
        // Validate PUT before unlinking an existing target.
        let put = if entry_type == 15 {
            Some(wire::put(path, mode, data)?)
        } else {
            None
        };
        match self.stat(path) {
            Ok(_) => self.unlink(path)?,
            Err(e) if e.status == Some(2) => {}
            Err(e) => return Err(e),
        }
        let mut parent = String::new();
        let parts: Vec<_> = path.split('/').skip(1).collect();
        for part in &parts[..parts.len() - 1] {
            parent.push('/');
            parent.push_str(part);
            self.mkdir(&parent, wire::DEFAULT_MODE as u16)?;
        }
        if let Some(req) = put {
            let r = self.request(&req)?;
            let status = wire::u16_at(&r, 6)?;
            if status != 0 {
                return Err(Error::status("PUT", status as u32));
            }
            if wire::u16_at(&r, 8)? as usize != data.len() || wire::u16_at(&r, 4)? != mode as u16 {
                return Err(Error::new(
                    "malformed",
                    "PUT",
                    "Incorrect PUT mode or byte count",
                ));
            }
            return self.sync(path);
        }
        let fd = self.open(path, wire::WRITE_CREATE, mode)?;
        let result = (|| {
            let mut offset = 0;
            while offset < data.len() {
                let end = (offset + 1024).min(data.len());
                let mut req = wire::words(5, &[fd, offset as u32]);
                req.extend(&data[offset..end]);
                let r = self.request(&req)?;
                wire::status(&r, 16, "write")?;
                let n = wire::u32_at(&r, 12)? as usize;
                if n == 0 || n > end - offset {
                    return Err(Error::new("malformed", "write", "Invalid write progress"));
                }
                offset += n;
            }
            self.sync(path)
        })();
        let close = self.close(fd, false);
        combine(result, close)
    }
    pub fn list(&mut self, path: &str) -> Result<Vec<DirEntry>> {
        let fd = self.opendir(path)?;
        let result = (|| {
            let mut entries = vec![];
            for seq in 1..=10000 {
                let r = self.request(&wire::words(12, &[fd, seq]))?;
                // InvalidSequence is documented upstream as iterator termination.
                match wire::status(&r, 12, "readdir") {
                    Err(e) if e.status == Some(0x40000002) => return Ok(entries),
                    r => r?,
                }
                let bytes = r
                    .get(40..)
                    .ok_or_else(|| Error::new("malformed", "readdir", "Missing name"))?;
                let end = bytes
                    .iter()
                    .position(|b| *b == 0)
                    .ok_or_else(|| Error::new("malformed", "readdir", "Unterminated name"))?;
                if end == 0 {
                    return Ok(entries);
                }
                let name = std::str::from_utf8(&bytes[..end])
                    .map_err(|_| Error::new("invalidPath", "readdir", "Non-ASCII entry"))?
                    .to_owned();
                if name == "." || name == ".." {
                    continue;
                }
                safe_remote(&format!("/{name}"))?;
                if name.contains('/') {
                    return Err(Error::new(
                        "invalidPath",
                        "readdir",
                        "Entry includes separator",
                    ));
                }
                entries.push(DirEntry {
                    name,
                    metadata: Metadata {
                        entry_type: wire::u32_at(&r, 16)?,
                        mode: wire::u32_at(&r, 20)?,
                        size: wire::u32_at(&r, 24)?,
                        atime: wire::u32_at(&r, 28)?,
                        mtime: wire::u32_at(&r, 32)?,
                        ctime: wire::u32_at(&r, 36)?,
                    },
                });
            }
            Err(Error::new("limit", "readdir", "Too many directory entries"))
        })();
        let close = self.close(fd, true);
        combine(result, close)
    }
    pub fn nv_read(&mut self, id: u16) -> Result<Vec<u8>> {
        let r = self.request(&wire::nv_read(id))?;
        wire::nv_response(&r, "NV read")
    }
    pub fn nv_write(&mut self, id: u16, data: &[u8]) -> Result<()> {
        let r = self.request(&wire::nv_write(id, data)?)?;
        wire::nv_response(&r, "NV write").map(|_| ())
    }
}
fn combine<T>(result: Result<T>, close: Result<()>) -> Result<T> {
    match (result, close) {
        (Ok(v), Ok(())) => Ok(v),
        (Err(e), Ok(())) => Err(e),
        (Ok(_), Err(e)) => Err(e),
        (Err(mut e), Err(c)) => {
            e.cleanup.push(c.to_string());
            Err(e)
        }
    }
}
