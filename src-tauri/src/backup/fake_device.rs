//! 테스트 전용 가짜 기기 — ADBDeviceExt를 메모리 가상 파일시스템으로 구현.
//! 실기기 테스트 금지(사용자 승인 2026-10-03) 조건에서 엔진 전 과정을 검증한다.
//! 이 파일은 cfg(test)에서만 컴파일된다(제품 빌드에 포함되지 않음).

use adb_client::{
    ADBDeviceExt, ADBListItem, ADBListItemType, ADBStatExtendedResponse, ADBStatMapping,
    AdbStatResponse, RebootType, RemountInfo, RustADBError,
};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct FileRec {
    pub data: Vec<u8>,
    pub mtime: u32,
    pub perm: u32,
}

#[derive(Default)]
pub struct FakeADBDevice {
    files: BTreeMap<String, FileRec>,
    dirs: BTreeSet<String>,
    /// path → 링크 대상(이름만 기록, list에서 Symlink로 보고)
    symlinks: BTreeMap<String, String>,
    /// list()가 실패하게 만들 경로
    pub fail_list: BTreeSet<String>,
    /// list()가 보고할 크기 덮어쓰기 — SYNC u32 wrap 시뮬레이션
    pub list_size_override: BTreeMap<String, u32>,
    /// pull()이 실패하게 만들 경로
    pub fail_pull: BTreeSet<String>,
    /// shell_command 응답 규칙 — 명령 시작부 일치 시 텍스트 반환
    shell_answers: Vec<(String, String)>,
    /// 실행된 셸 명령 전체 기록
    pub shell_calls: Vec<String>,
    /// Some이면 SMS 역할 변경을 모의한다(내부 None은 기본 앱 없음).
    pub sms_role_holder: Option<Option<String>>,
    pub ignore_role_changes: bool,
    pub fail_shell: BTreeSet<String>,
    pub shell_exit_codes: BTreeMap<String, u8>,
    /// 인터랙티브 셸(shell(reader, writer)) 기록 — 명령 → stdin으로 받은 바이트
    pub shell_streams: Vec<(String, Vec<u8>)>,
    /// 대화형 셸이 stdin을 소비하는 최대 바이트(기본 무제한)
    pub shell_stream_read_limit: Option<usize>,
    /// push로 쓴 파일(복원 검증용)
    pub pushed: BTreeMap<String, Vec<u8>>,
    /// install 호출 기록
    pub installs: Vec<String>,
    /// 현재 시각(unix 초) — mtime 기본값
    pub now: u32,
    /// exec 명령의 종료 코드(종료 코드 표식을 붙인 명령에만 반영)
    pub exec_rc: i32,
    /// 설정하면 exec stdin을 메모리 대신 이 폴더의 파일로 받는다(실제 백업 크기 복원 모의용)
    pub spool_dir: Option<std::path::PathBuf>,
    /// spool_dir 사용 시 (명령, 받은 파일)
    pub spooled: Vec<(String, std::path::PathBuf)>,
}

impl FakeADBDevice {
    pub fn new() -> Self {
        Self {
            now: 1_700_000_000,
            ..Default::default()
        }
    }

    pub fn add_dir(&mut self, path: &str) {
        // 조상 경로 전부 등록("/sdcard/DCIM/Camera" → "/sdcard", "/sdcard/DCIM"도)
        let mut cur = String::new();
        for part in path.split('/').filter(|s| !s.is_empty()) {
            cur.push('/');
            cur.push_str(part);
            self.dirs.insert(cur.clone());
        }
    }

    pub fn add_file(&mut self, path: &str, data: &[u8], mtime: u32, perm: u32) {
        self.files.insert(
            path.to_string(),
            FileRec {
                data: data.to_vec(),
                mtime,
                perm,
            },
        );
    }

    pub fn add_symlink(&mut self, path: &str, target: &str) {
        self.symlinks.insert(path.to_string(), target.to_string());
    }

    pub fn fail_list(&mut self, path: &str) {
        self.fail_list.insert(path.to_string());
    }

    pub fn fail_pull(&mut self, path: &str) {
        self.fail_pull.insert(path.to_string());
    }

    /// 명령 시작부가 prefix와 일치하면 stdout으로 answer 반환
    pub fn answer_shell(&mut self, prefix: &str, answer: &str) {
        self.shell_answers
            .push((prefix.to_string(), answer.to_string()));
    }

    fn parent_of(path: &str) -> &str {
        path.rsplit_once('/').map(|(p, _)| p).unwrap_or("")
    }

    fn name_of(path: &str) -> &str {
        path.rsplit_once('/').map(|(_, n)| n).unwrap_or(path)
    }
}

impl ADBDeviceExt for FakeADBDevice {
    fn shell_command(
        &mut self,
        command: &dyn AsRef<str>,
        mut stdout: Option<&mut dyn Write>,
        mut stderr: Option<&mut dyn Write>,
    ) -> Result<Option<u8>, RustADBError> {
        let cmd = command.as_ref();
        self.shell_calls.push(cmd.to_string());
        if self.fail_shell.iter().any(|prefix| cmd.starts_with(prefix)) {
            if let Some(err) = stderr.as_deref_mut() {
                err.write_all(b"injected shell failure")?;
            }
            return Ok(Some(1));
        }
        if let Some(holder) = &mut self.sms_role_holder {
            if cmd == "cmd role get-role-holders android.app.role.SMS" {
                if let Some(out) = stdout.as_deref_mut() {
                    out.write_all(holder.as_deref().unwrap_or("").as_bytes())?;
                }
                return Ok(Some(0));
            }
            if let Some(package) =
                cmd.strip_prefix("cmd role add-role-holder android.app.role.SMS ")
            {
                if !self.ignore_role_changes {
                    *holder = Some(package.into());
                }
                return Ok(Some(0));
            }
            if cmd.starts_with("cmd role remove-role-holder android.app.role.SMS ") {
                if !self.ignore_role_changes {
                    *holder = None;
                }
                return Ok(Some(0));
            }
        }
        if let Some((_, answer)) = self
            .shell_answers
            .iter()
            .find(|(p, _)| cmd.starts_with(p.as_str()))
        {
            if let Some(out) = stdout.as_deref_mut() {
                out.write_all(answer.as_bytes())?;
            }
            return Ok(Some(self.shell_exit_codes.get(cmd).copied().unwrap_or(0)));
        }
        if let Some(err) = stderr.as_deref_mut() {
            write!(err, "unknown command: {cmd}")?;
        }
        Ok(Some(1))
    }

    fn shell(
        &mut self,
        reader: &mut dyn Read,
        mut writer: Box<dyn Write + Send>,
    ) -> Result<(), RustADBError> {
        // 마지막 셸 명령을 스트림 대상으로 간주 — 엔진은 "tar -xf - ..."를 shell로 흘린다
        let cmd = self.shell_calls.last().cloned().unwrap_or_default();
        let mut buf = Vec::new();
        match self.shell_stream_read_limit {
            Some(limit) => {
                let mut chunk = vec![0u8; limit];
                let n = reader.read(&mut chunk)?;
                chunk.truncate(n);
                buf = chunk;
            }
            None => {
                let _ = reader.read_to_end(&mut buf)?;
            }
        }
        self.shell_streams.push((cmd, buf));
        writer.write_all(b"OK\n")?;
        Ok(())
    }

    fn exec(
        &mut self,
        command: &str,
        reader: &mut dyn Read,
        mut writer: Box<dyn Write + Send>,
    ) -> Result<(), RustADBError> {
        // stdin으로 받은 바이트를 기록(tar 스트리밍·install-write 검증용)
        self.shell_calls.push(command.to_string());
        // 기록은 표식을 뗀 원래 명령으로 (검증 편의)
        let base = command
            .split(" 2>&1; echo __XV_RC=")
            .next()
            .unwrap_or(command)
            .to_string();
        if let Some(dir) = &self.spool_dir {
            let path = dir.join(format!("stream-{:05}.bin", self.spooled.len()));
            let mut f = std::fs::File::create(&path)?;
            std::io::copy(reader, &mut f)?;
            self.spooled.push((base, path));
        } else {
            let mut buf = Vec::new();
            reader.read_to_end(&mut buf)?;
            self.shell_streams.push((base, buf));
        }
        writer.write_all(b"Success\n")?;
        if command.contains("echo __XV_RC=") {
            writer.write_all(format!("__XV_RC={}\n", self.exec_rc).as_bytes())?;
        }
        Ok(())
    }

    fn stat(&mut self, remote_path: &dyn AsRef<str>) -> Result<AdbStatResponse, RustADBError> {
        let path = remote_path.as_ref();
        match self.files.get(path) {
            Some(rec) => Ok(AdbStatResponse {
                file_perm: rec.perm,
                file_size: rec.data.len() as u32,
                mod_time: rec.mtime,
            }),
            None => Err(RustADBError::ADBRequestFailed(format!(
                "stat failed for {path}"
            ))),
        }
    }

    /// 셸 파식 대신 메모리 기록에서 직접 구성 — 64비트 실제 크기 제공(u32 wrap 검증용)
    fn stat_extended(
        &mut self,
        remote_path: &dyn AsRef<str>,
    ) -> Result<Option<ADBStatExtendedResponse>, RustADBError> {
        let path = remote_path.as_ref();
        match self.files.get(path) {
            Some(rec) => {
                let mk = || ADBStatMapping {
                    id: 1023,
                    name: "media_rw".into(),
                };
                Ok(Some(ADBStatExtendedResponse {
                    path: path.to_string(),
                    size: rec.data.len() as u64,
                    blocks: (rec.data.len() as u64).div_ceil(4096),
                    io_blocks: 4096,
                    inode: 12345,
                    links: 1,
                    perms: rec.perm as u64,
                    user: mk(),
                    group: mk(),
                    atime: rec.mtime,
                    mtime: rec.mtime,
                    ctime: rec.mtime,
                }))
            }
            None => Ok(None),
        }
    }

    fn pull(
        &mut self,
        source: &dyn AsRef<str>,
        output: &mut dyn Write,
    ) -> Result<(), RustADBError> {
        let path = source.as_ref();
        if self.fail_pull.contains(path) {
            return Err(RustADBError::ADBRequestFailed(format!(
                "pull failed for {path}"
            )));
        }
        match self.files.get(path) {
            Some(rec) => {
                output.write_all(&rec.data)?;
                Ok(())
            }
            None => Err(RustADBError::ADBRequestFailed(format!(
                "no such file {path}"
            ))),
        }
    }

    fn push(&mut self, stream: &mut dyn Read, path: &dyn AsRef<str>) -> Result<(), RustADBError> {
        let mut buf = Vec::new();
        stream.read_to_end(&mut buf)?;
        self.pushed.insert(path.as_ref().to_string(), buf);
        Ok(())
    }

    fn list(&mut self, path: &dyn AsRef<str>) -> Result<Vec<ADBListItemType>, RustADBError> {
        let dir = path.as_ref();
        if self.fail_list.contains(dir) {
            return Err(RustADBError::ADBRequestFailed(format!(
                "list failed for {dir}"
            )));
        }
        if !self.dirs.contains(dir) {
            return Err(RustADBError::ADBRequestFailed(format!("no such dir {dir}")));
        }
        let mut out = Vec::new();
        // 하위 디렉터리
        for d in &self.dirs {
            if Self::parent_of(d) == dir && d != dir {
                out.push(ADBListItemType::Directory(ADBListItem {
                    name: Self::name_of(d).to_string(),
                    time: self.now,
                    permissions: 0o40755,
                    size: 4096,
                }));
            }
        }
        // 심볼릭 링크
        for (p, _) in &self.symlinks {
            if Self::parent_of(p) == dir {
                out.push(ADBListItemType::Symlink(ADBListItem {
                    name: Self::name_of(p).to_string(),
                    time: self.now,
                    permissions: 0o120777,
                    size: 0,
                }));
            }
        }
        // 파일
        for (p, rec) in &self.files {
            if Self::parent_of(p) == dir {
                let size = self
                    .list_size_override
                    .get(p)
                    .copied()
                    .unwrap_or(rec.data.len() as u32);
                out.push(ADBListItemType::File(ADBListItem {
                    name: Self::name_of(p).to_string(),
                    time: rec.mtime,
                    permissions: 0o100000 | rec.perm,
                    size,
                }));
            }
        }
        Ok(out)
    }

    fn reboot(&mut self, _reboot_type: RebootType) -> Result<(), RustADBError> {
        Ok(())
    }

    fn remount(&mut self) -> Result<Vec<RemountInfo>, RustADBError> {
        Ok(vec![])
    }

    fn root(&mut self) -> Result<(), RustADBError> {
        Err(RustADBError::ADBRequestFailed(
            "adbd cannot root in fake".into(),
        ))
    }

    fn install(
        &mut self,
        apk_path: &dyn AsRef<Path>,
        _user: Option<&str>,
    ) -> Result<(), RustADBError> {
        self.installs
            .push(apk_path.as_ref().to_string_lossy().to_string());
        Ok(())
    }

    fn uninstall(
        &mut self,
        package: &dyn AsRef<str>,
        _user: Option<&str>,
    ) -> Result<(), RustADBError> {
        self.shell_calls
            .push(format!("uninstall {}", package.as_ref()));
        Ok(())
    }

    fn enable_verity(&mut self) -> Result<(), RustADBError> {
        Ok(())
    }

    fn disable_verity(&mut self) -> Result<(), RustADBError> {
        Ok(())
    }
}
