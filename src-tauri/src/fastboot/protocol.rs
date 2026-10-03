//! fastboot 프로토콜 — OKAY/FAIL/INFO/DATA 유한 상태머신(§9-3)과 명령 API.
//! 원본 CLI 계승: `oem unlock 0x{code}` · `oem lock` · `flash <part>_a/_b` · `reboot` (src/adb.py)
//! INFO 프레임은 로그 콜백으로 흘리고 종결 응답(OKAY/FAIL/DATA)만 반환 — 무한 루프 방지 상한.

use crate::fastboot::transport::FastbootTransport;
use std::collections::HashMap;

/// 종결 응답 — INFO는 콜백으로 처리되어 여기 오지 않는다
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Terminal {
    Ok(String),
    Fail(String),
    /// DATA + 수신 허용 크기(바이트)
    Data(u64),
}

/// INFO 프레임 상한 — 정상 기기도 getvar:all에 수십 프레임 보내지만, 고장 시 무한 대비
const MAX_INFO_FRAMES: usize = 256;
/// 단일 다운로드 상한 — max-download-size와 함께 검사(§9-3 유한 처리)
pub const MAX_DOWNLOAD: u64 = 1024 * 1024 * 1024;

pub struct FastbootDevice<T: FastbootTransport> {
    transport: T,
    on_log: Box<dyn FnMut(String) + Send>,
}

impl<T: FastbootTransport> FastbootDevice<T> {
    pub fn new(transport: T, on_log: Box<dyn FnMut(String) + Send>) -> Self {
        Self { transport, on_log }
    }

    /// 응답 읽기 — INFO는 로그로 흘리고 종결 프레임을 기다린다
    fn read_terminal(&mut self) -> Result<Terminal, String> {
        for _ in 0..MAX_INFO_FRAMES {
            let mut buf = [0u8; 256];
            let n = self.transport.read_frame(&mut buf)?;
            if n < 4 {
                return Err(format!("응답이 너무 짧습니다({n}바이트)"));
            }
            let status = &buf[..4];
            let payload = String::from_utf8_lossy(&buf[4..n]).trim().to_string();
            match status {
                b"OKAY" => return Ok(Terminal::Ok(payload)),
                b"FAIL" => return Ok(Terminal::Fail(payload)),
                b"DATA" => {
                    let size = u64::from_str_radix(&payload, 16)
                        .map_err(|_| format!("DATA 크기 해석 실패: {payload}"))?;
                    return Ok(Terminal::Data(size));
                }
                b"INFO" => (self.on_log)(payload),
                other => {
                    return Err(format!(
                        "알 수 없는 응답: {}",
                        String::from_utf8_lossy(other)
                    ));
                }
            }
        }
        Err("INFO 프레임 한도(256) 초과 — 기기가 응답을 끝내지 않습니다".into())
    }

    /// 단일 명령 → 종결 응답 (INFO는 로그)
    fn command(&mut self, cmd: &str) -> Result<Terminal, String> {
        (self.on_log)(format!("> {cmd}"));
        self.transport.write_command(cmd)?;
        let t = self.read_terminal()?;
        match &t {
            Terminal::Ok(v) => (self.on_log)(format!("OKAY {v}")),
            Terminal::Fail(r) => (self.on_log)(format!("FAIL {r}")),
            Terminal::Data(n) => (self.on_log)(format!("DATA {n:#x}")),
        }
        Ok(t)
    }

    /// 성공 필수 명령 — FAIL을 오류로 변환
    fn expect_ok(&mut self, cmd: &str) -> Result<String, String> {
        match self.command(cmd)? {
            Terminal::Ok(v) => Ok(v),
            Terminal::Fail(r) => Err(format!("기기 거부(FAIL): {r}")),
            Terminal::Data(_) => Err("예상치 못한 DATA 응답".into()),
        }
    }

    /// getvar:all — INFO "(bootloader) key: value" 수집 → 맵. 종결 FAIL은 그대로 반환(일부 기기는 all 미지원)
    pub fn getvar_all(&mut self) -> Result<HashMap<String, String>, String> {
        (self.on_log)("> getvar:all".into());
        self.transport.write_command("getvar:all")?;
        let mut vars = HashMap::new();
        for _ in 0..MAX_INFO_FRAMES {
            let mut buf = [0u8; 256];
            let n = self.transport.read_frame(&mut buf)?;
            if n < 4 {
                return Err(format!("응답이 너무 짧습니다({n}바이트)"));
            }
            let status = &buf[..4];
            let payload = String::from_utf8_lossy(&buf[4..n]).trim().to_string();
            match status {
                b"INFO" => {
                    (self.on_log)(payload.clone());
                    // "(bootloader)  key: value" / "key: value" 모두 허용
                    let line = payload
                        .trim_start_matches("(bootloader)")
                        .trim();
                    if let Some((k, v)) = line.split_once(':') {
                        vars.insert(k.trim().to_string(), v.trim().to_string());
                    }
                }
                b"OKAY" => return Ok(vars),
                b"FAIL" => return Err(format!("getvar:all 거부: {payload}")),
                b"DATA" => return Err("getvar에 DATA 응답(비정상)".into()),
                other => return Err(format!("알 수 없는 응답: {}", String::from_utf8_lossy(other))),
            }
        }
        Err("INFO 프레임 한도(256) 초과 — getvar:all이 끝나지 않습니다".into())
    }

    /// 개별 getvar — OKAY value / FAIL → None
    pub fn getvar(&mut self, name: &str) -> Result<Option<String>, String> {
        match self.command(&format!("getvar:{name}"))? {
            Terminal::Ok(v) => Ok(Some(v)),
            Terminal::Fail(_) => Ok(None),
            Terminal::Data(_) => Err("getvar에 DATA 응답(비정상)".into()),
        }
    }

    /// 부트로더 언락 — Sony 전용 형식 `oem unlock 0x{code}` (원본 adb.py oemUnlock 계승)
    pub fn oem_unlock(&mut self, code: &str) -> Result<(), String> {
        validate_unlock_code(code)?;
        match self.command(&format!("oem unlock 0x{code}"))? {
            Terminal::Ok(_) => Ok(()),
            Terminal::Fail(r) => Err(format!("언락 거부(FAIL): {r} — 코드를 다시 확인해 주세요")),
            Terminal::Data(_) => Err("예상치 못한 DATA 응답".into()),
        }
    }

    /// 부트로더 리락 — `oem lock`
    pub fn oem_lock(&mut self) -> Result<(), String> {
        match self.command("oem lock")? {
            Terminal::Ok(_) => Ok(()),
            Terminal::Fail(r) => Err(format!("리락 거부(FAIL): {r}")),
            Terminal::Data(_) => Err("예상치 못한 DATA 응답".into()),
        }
    }

    /// 데이터 다운로드 — DATA 협상(크기 일치 필수) → 본문 전송 → 최종 OKAY/FAIL
    pub fn download(&mut self, data: &[u8]) -> Result<(), String> {
        let len = data.len() as u64;
        if len > MAX_DOWNLOAD {
            return Err(format!("이미지가 너무 큽니다({len}바이트 > {MAX_DOWNLOAD})"));
        }
        (self.on_log)(format!("> download:{len:#010x}"));
        self.transport.write_command(&format!("download:{len:08x}"))?;
        match self.read_terminal()? {
            Terminal::Data(offer) if offer == len => {}
            Terminal::Data(offer) => {
                // 크기 불일치 — 본문을 보내면 기기 상태가 꼬일 수 있으니 즉시 중단
                return Err(format!("DATA 크기 불일치: 기기 {offer}바이트, 요청 {len}바이트"));
            }
            Terminal::Fail(r) => return Err(format!("다운로드 거부(FAIL): {r}")),
            Terminal::Ok(_) => return Err("본문 없이 OKAY(비정상)".into()),
        }
        self.transport.write_data(data)?;
        match self.read_terminal()? {
            Terminal::Ok(_) => Ok(()),
            Terminal::Fail(r) => Err(format!("전송 후 기기 거부(FAIL): {r}")),
            Terminal::Data(_) => Err("본문 후 DATA(비정상)".into()),
        }
    }

    /// 파티션 기록 — download 후 `flash:<partition>`
    pub fn flash(&mut self, partition: &str, image: &[u8]) -> Result<(), String> {
        validate_partition(partition)?;
        // max-download-size 확인(알 수 없으면 상한만)
        if let Ok(Some(max)) = self.getvar("max-download-size") {
            let cap = parse_size(&max).unwrap_or(MAX_DOWNLOAD);
            if image.len() as u64 > cap {
                return Err(format!(
                    "이미지({}바이트)가 기기 다운로드 상한({cap}바이트)을 넘습니다",
                    image.len()
                ));
            }
        }
        self.download(image)?;
        self.expect_ok(&format!("flash:{partition}"))?;
        Ok(())
    }

    /// 재부팅 — target "os"(reboot) | "bootloader"(reboot-bootloader)
    pub fn reboot(&mut self, target: &str) -> Result<(), String> {
        let cmd = match target {
            "os" => "reboot",
            "bootloader" => "reboot-bootloader",
            other => return Err(format!("알 수 없는 재부팅 대상: {other}")),
        };
        // 재부팅은 응답이 없을 수 있다 — Ok이면 좋고, 타임아웃이면 성공으로 본다(USB가 끊기며 프레임이 안 옴)
        match self.command(cmd) {
            Ok(_) => Ok(()),
            Err(e) if e.contains("시간 초과") => {
                (self.on_log)("[재부팅] 응답 없음(정상 — USB 연결이 끊어짐)".into());
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// 연결 해제(인터페이스 반납은 Drop)
    pub fn into_inner(self) -> T {
        self.transport
    }
}

/// 언락 코드 형식 — 16자리 16진수(Sony 발급). 0x 접두는 프론트에서 이미 제거됨
pub fn validate_unlock_code(code: &str) -> Result<(), String> {
    if code.len() == 16 && code.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err("언락 코드는 16자리 16진수여야 합니다".into())
    }
}

/// 파티션명 검증 — 알파벳·숫자·밑줄만 (boot, init_boot, boot_a …)
fn validate_partition(part: &str) -> Result<(), String> {
    if !part.is_empty()
        && part
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        Ok(())
    } else {
        Err(format!("파티션명이 올바르지 않습니다: {part}"))
    }
}

/// fastboot 크기 표기 해석 — "0x2000000" 또는 "33554432"
fn parse_size(s: &str) -> Option<u64> {
    let t = s.trim();
    if let Some(hex) = t.strip_prefix("0x") {
        u64::from_str_radix(hex, 16).ok()
    } else {
        t.parse().ok()
    }
}

#[cfg(test)]
pub mod fake {
    //! 테스트 전용 가짜 트랜스포트 — 시나리오 스크립트로 응답 재생
    use super::*;
    use std::collections::VecDeque;

    #[derive(Debug, Clone)]
    pub enum Frame {
        Ok(&'static str),
        Fail(&'static str),
        Info(&'static str),
        /// DATA 응답 크기 — None이면 download 명령의 요청 크기를 그대로 승인
        Data(Option<u64>),
        /// 읽기 즉시 타임아웃
        Timeout,
    }

    pub struct FakeTransport {
        pub sent_cmds: Vec<String>,
        pub sent_data: Vec<u8>,
        pub frames: VecDeque<Frame>,
    }

    impl FakeTransport {
        pub fn new(frames: Vec<Frame>) -> Self {
            Self { sent_cmds: vec![], sent_data: vec![], frames: frames.into() }
        }
        /// download:%08x 명령에 자동으로 DATA 승인 프레임을 준비하는 헬퍼
        pub fn with_download(mut self) -> Self {
            self.frames.push_back(Frame::Data(None));
            self
        }
    }

    impl FastbootTransport for FakeTransport {
        fn write_command(&mut self, cmd: &str) -> Result<(), String> {
            self.sent_cmds.push(cmd.to_string());
            Ok(())
        }

        fn read_frame(&mut self, buf: &mut [u8; 256]) -> Result<usize, String> {
            let Some(frame) = self.frames.pop_front() else {
                return Err("fastboot 응답 시간 초과".into());
            };
            let (status, payload): ([u8; 4], String) = match frame {
                Frame::Ok(v) => (*b"OKAY", v.to_string()),
                Frame::Fail(v) => (*b"FAIL", v.to_string()),
                Frame::Info(v) => (*b"INFO", v.to_string()),
                Frame::Data(size) => {
                    // None이면 마지막 download 명령의 크기를 승인
                    let offer = size.unwrap_or_else(|| {
                        self.sent_cmds
                            .iter()
                            .rev()
                            .find_map(|c| c.strip_prefix("download:"))
                            .and_then(|h| u64::from_str_radix(h, 16).ok())
                            .unwrap_or(0)
                    });
                    (*b"DATA", format!("{offer:08x}"))
                }
                Frame::Timeout => return Err("fastboot 응답 시간 초과".into()),
            };
            buf[..4].copy_from_slice(&status);
            let p = payload.as_bytes();
            buf[4..4 + p.len()].copy_from_slice(p);
            Ok(4 + p.len())
        }

        fn write_data(&mut self, data: &[u8]) -> Result<(), String> {
            self.sent_data.extend_from_slice(data);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::*;
    use super::*;

    fn dev_with(frames: Vec<Frame>) -> (FastbootDevice<FakeTransport>, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        let logs = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let l2 = logs.clone();
        let dev = FastbootDevice::new(
            FakeTransport::new(frames),
            Box::new(move |line| l2.lock().unwrap().push(line)),
        );
        (dev, logs)
    }

    #[test]
    fn getvar_all_parses_bootloader_lines() {
        let (mut d, logs) = dev_with(vec![
            Frame::Info("(bootloader)  version-bootloader: 1.0"),
            Frame::Info("(bootloader) unlocked: yes"),
            Frame::Info("current-slot: a"),
            Frame::Ok(""),
        ]);
        let vars = d.getvar_all().unwrap();
        assert_eq!(vars.get("version-bootloader").map(String::as_str), Some("1.0"));
        assert_eq!(vars.get("unlocked").map(String::as_str), Some("yes"));
        assert_eq!(vars.get("current-slot").map(String::as_str), Some("a"));
        assert_eq!(logs.lock().unwrap().len(), 4); // > getvar:all + INFO 3 (종결 OKAY는 로그 없음)
    }

    #[test]
    fn single_getvar_fail_is_none() {
        let (mut d, _) = dev_with(vec![Frame::Fail("no value")]);
        assert_eq!(d.getvar("nonexistent").unwrap(), None);
    }

    #[test]
    fn unlock_validates_and_sends_sony_format() {
        let (mut d, _) = dev_with(vec![Frame::Ok("")]);
        d.oem_unlock("1234567890abcdef").unwrap();
        assert_eq!(d.transport.sent_cmds[0], "oem unlock 0x1234567890abcdef");

        let (mut bad, _) = dev_with(vec![]);
        assert!(bad.oem_unlock("0x1234").is_err()); // 0x 포함/짧음 거부
        assert!(bad.oem_unlock("GGGGGGGGGGGGGGGG").is_err());
        assert!(bad.transport.sent_cmds.is_empty()); // 검증 실패 시 전송 없음
    }

    #[test]
    fn unlock_fail_reports_reason() {
        let (mut d, _) = dev_with(vec![Frame::Fail("Invalid unlock code")]);
        let err = d.oem_unlock("1234567890abcdef").unwrap_err();
        assert!(err.contains("Invalid unlock code"));
    }

    #[test]
    fn lock_sends_oem_lock() {
        let (mut d, _) = dev_with(vec![Frame::Ok("")]);
        d.oem_lock().unwrap();
        assert_eq!(d.transport.sent_cmds, vec!["oem lock".to_string()]);
    }

    #[test]
    fn flash_full_flow_download_then_flash() {
        let image = vec![0xABu8; 1024];
        let frames = vec![
            Frame::Ok("0x10000000"), // max-download-size
            Frame::Data(None),       // download 승인(요청 크기 그대로)
            Frame::Ok(""),           // 본문 전송 후 OKAY
            Frame::Ok(""),           // flash:boot_a
        ];
        let (mut d, _) = dev_with(frames);
        d.flash("boot_a", &image).unwrap();
        assert_eq!(d.transport.sent_cmds[0], "getvar:max-download-size");
        assert_eq!(d.transport.sent_cmds[1], format!("download:{:08x}", 1024));
        assert_eq!(d.transport.sent_cmds[2], "flash:boot_a");
        assert_eq!(d.transport.sent_data, image);
    }

    #[test]
    fn flash_rejects_oversize_vs_max_download() {
        let image = vec![0u8; 4096];
        let (mut d, _) = dev_with(vec![Frame::Ok("0x1000")]); // 상한 4096
        // 이미지 4096 == 상한 4096은 통과… 초과 케이스로 4097
        let bigger = vec![0u8; 4097];
        assert!(d.flash("boot_a", &bigger).is_err());
        let _ = image;
    }

    #[test]
    fn download_size_mismatch_aborts_before_payload() {
        let (mut d, _) = dev_with(vec![Frame::Data(Some(999))]); // 요청과 다른 승인
        let err = d.download(&vec![0u8; 1024]).unwrap_err();
        assert!(err.contains("불일치"));
        assert!(d.transport.sent_data.is_empty()); // 본문 미전송 — 기기 보호
    }

    #[test]
    fn info_frames_logged_until_terminal() {
        let (mut d, logs) = dev_with(vec![
            Frame::Info("erasing..."),
            Frame::Info("writing..."),
            Frame::Ok(""),
        ]);
        d.expect_ok("oem unlock 0x1234567890abcdef").unwrap();
        let l = logs.lock().unwrap();
        assert!(l.iter().any(|x| x.contains("erasing")));
        assert!(l.iter().any(|x| x.contains("writing")));
    }

    #[test]
    fn info_flood_hits_limit() {
        let frames: Vec<Frame> = (0..300).map(|_| Frame::Info("loop")).collect();
        let (mut d, _) = dev_with(frames);
        assert!(d.expect_ok("getvar:x").is_err());
    }

    #[test]
    fn timeout_maps_to_error() {
        let (mut d, _) = dev_with(vec![Frame::Timeout]);
        let err = d.expect_ok("reboot").unwrap_err();
        assert!(err.contains("시간 초과"));
    }

    #[test]
    fn reboot_treats_timeout_as_success() {
        let (mut d, logs) = dev_with(vec![Frame::Timeout]);
        d.reboot("os").unwrap(); // 타임아웃 → 성공 간주(USB 끊김)
        let l = logs.lock().unwrap();
        assert!(l.iter().any(|x| x.contains("정상")));
    }

    #[test]
    fn size_parsing() {
        assert_eq!(parse_size("0x2000000"), Some(0x2000000));
        assert_eq!(parse_size("33554432"), Some(33554432));
        assert_eq!(parse_size("abc"), None);
    }

    #[test]
    fn partition_name_validation() {
        assert!(validate_partition("boot_a").is_ok());
        assert!(validate_partition("init_boot").is_ok());
        assert!(validate_partition("boot;rm").is_err());
        assert!(validate_partition("").is_err());
    }
}
