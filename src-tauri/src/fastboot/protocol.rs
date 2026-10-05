//! fastboot 프로토콜 — OKAY/FAIL/INFO/DATA 유한 상태머신(§9-3)과 명령 API.
//! 원본 CLI 계승: `oem unlock 0x{code}` · `oem lock` · `flash <part>_a/_b` · `reboot` (src/adb.py)
//! INFO 프레임은 로그 콜백으로 흘리고 종결 응답(OKAY/FAIL/DATA)만 반환 — 무한 루프 방지 상한.

use crate::fastboot::transport::{
    FastbootTransport, GETVAR_ALL_TIMEOUT, LONG_RESPONSE_TIMEOUT, RESPONSE_TIMEOUT,
};
use std::collections::HashMap;
use std::time::{Duration, Instant};

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

    #[cfg(test)]
    pub fn into_transport(self) -> T {
        self.transport
    }

    /// 응답 읽기 — INFO는 로그로 흘리고(on_info에도 전달) 종결 프레임을 기다린다.
    /// timeout은 종결 응답까지의 전체 상한 — INFO가 와도 제한 시간을 늘리지 않는다.
    fn read_terminal_with(
        &mut self,
        timeout: Duration,
        on_info: &mut dyn FnMut(&str),
    ) -> Result<Terminal, String> {
        let deadline = Instant::now() + timeout;
        for frame in 0..=MAX_INFO_FRAMES {
            let mut buf = [0u8; 256];
            let remaining = deadline.saturating_duration_since(Instant::now());
            // libusb의 밀리초 timeout=0은 무제한이므로 1ms 미만은 보내지 않는다.
            if remaining < Duration::from_millis(1) {
                return Err("fastboot 종결 응답 시간 초과".into());
            }
            let n = self.transport.read_frame(&mut buf, remaining)?;
            if !(4..=buf.len()).contains(&n) {
                return Err(format!("응답이 너무 짧습니다({n}바이트)"));
            }
            let status = &buf[..4];
            let payload = String::from_utf8_lossy(&buf[4..n]).trim().to_string();
            match status {
                b"OKAY" => return Ok(Terminal::Ok(payload)),
                b"FAIL" => return Ok(Terminal::Fail(payload)),
                b"DATA" => {
                    if n != 12
                        || payload.len() != 8
                        || !payload.bytes().all(|b| b.is_ascii_hexdigit())
                    {
                        return Err("DATA 크기는 정확히 8자리 16진수여야 합니다".into());
                    }
                    let size = u64::from_str_radix(&payload, 16)
                        .map_err(|_| format!("DATA 크기 해석 실패: {payload}"))?;
                    return Ok(Terminal::Data(size));
                }
                b"INFO" | b"TEXT" if frame < MAX_INFO_FRAMES => {
                    on_info(&payload);
                    (self.on_log)(payload);
                }
                b"INFO" | b"TEXT" => break,
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

    fn read_terminal(&mut self, timeout: Duration) -> Result<Terminal, String> {
        self.read_terminal_with(timeout, &mut |_| {})
    }

    /// 단일 명령 → 종결 응답 (INFO는 로그)
    fn command(&mut self, cmd: &str) -> Result<Terminal, String> {
        self.command_with(cmd, RESPONSE_TIMEOUT)
    }

    fn command_with(&mut self, cmd: &str, timeout: Duration) -> Result<Terminal, String> {
        // 프로토콜 콜백 자체에도 언락 코드를 노출하지 않는다.
        let logged = if cmd.starts_with("oem unlock ") {
            "oem unlock [마스킹]"
        } else {
            cmd
        };
        (self.on_log)(format!("> {logged}"));
        self.transport.write_command(cmd)?;
        let t = self.read_terminal(timeout)?;
        if ["imei", "meid", "serialno", "serial-number"]
            .iter()
            .any(|key| cmd.to_ascii_lowercase().contains(key))
        {
            (self.on_log)("[기기 식별정보 응답 마스킹]".into());
            return Ok(t);
        }
        match &t {
            Terminal::Ok(v) => (self.on_log)(format!("OKAY {v}")),
            Terminal::Fail(r) => (self.on_log)(format!("FAIL {r}")),
            Terminal::Data(n) => (self.on_log)(format!("DATA {n:#x}")),
        }
        Ok(t)
    }

    /// 성공 필수 명령 — FAIL을 오류로 변환
    fn expect_ok(&mut self, cmd: &str) -> Result<String, String> {
        self.expect_ok_with(cmd, RESPONSE_TIMEOUT)
    }

    fn expect_ok_with(&mut self, cmd: &str, timeout: Duration) -> Result<String, String> {
        match self.command_with(cmd, timeout)? {
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
        let terminal = self.read_terminal_with(GETVAR_ALL_TIMEOUT, &mut |payload| {
            // "(bootloader)  key: value" / "key: value" 모두 허용
            let line = payload.trim_start_matches("(bootloader)").trim();
            // 슬롯 변수의 이름에도 ':'가 있다: slot-successful:a: yes.
            if let Some((k, v)) = line.split_once(": ").or_else(|| line.rsplit_once(':')) {
                vars.insert(k.trim().to_string(), v.trim().to_string());
            }
        });
        match terminal.map_err(|e| {
            e.replace(
                "기기가 응답을 끝내지 않습니다",
                "getvar:all이 끝나지 않습니다",
            )
        })? {
            Terminal::Ok(_) => Ok(vars),
            Terminal::Fail(payload) => Err(format!("getvar:all 거부: {payload}")),
            Terminal::Data(_) => Err("getvar에 DATA 응답(비정상)".into()),
        }
    }

    /// 개별 getvar — OKAY value / FAIL → None
    pub fn getvar(&mut self, name: &str) -> Result<Option<String>, String> {
        match self.command(&format!("getvar:{name}"))? {
            Terminal::Ok(v) => Ok(Some(v)),
            Terminal::Fail(_) => Ok(None),
            Terminal::Data(_) => Err("getvar에 DATA 응답(비정상)".into()),
        }
    }

    /// 상태 조회 실패/미지원/알 수 없는 값은 잠김으로 추측하지 않는다.
    pub fn unlocked(&mut self) -> Result<bool, String> {
        match self.getvar("unlocked")?.as_deref().map(str::trim) {
            Some(v) if v.eq_ignore_ascii_case("yes") => Ok(true),
            Some(v) if v.eq_ignore_ascii_case("no") => Ok(false),
            _ => Err("unlocked 상태를 확인할 수 없습니다 — 성공으로 처리하지 않습니다".into()),
        }
    }

    pub fn ensure_bootloader(&mut self) -> Result<(), String> {
        match self.getvar("is-userspace")?.as_deref().map(str::trim) {
            Some(v) if v.eq_ignore_ascii_case("no") => Ok(()),
            Some(v) if v.eq_ignore_ascii_case("yes") => {
                Err("fastbootd에서는 실행할 수 없습니다 — 부트로더 모드가 필요합니다".into())
            }
            _ => Err("부트로더 모드를 확인할 수 없습니다(is-userspace)".into()),
        }
    }

    /// 부트로더 언락 — Sony 전용 형식 `oem unlock 0x{code}` (원본 adb.py oemUnlock 계승)
    pub fn oem_unlock(&mut self, code: &str) -> Result<(), String> {
        validate_unlock_code(code)?;
        // Sony는 이 응답 전에 사용자 데이터를 초기화한다 — 긴 대기
        match self.command_with(&format!("oem unlock 0x{code}"), LONG_RESPONSE_TIMEOUT)? {
            Terminal::Ok(_) => Ok(()),
            Terminal::Fail(r) => Err(format!("언락 거부(FAIL): {r} — 코드를 다시 확인해 주세요")),
            Terminal::Data(_) => Err("예상치 못한 DATA 응답".into()),
        }
    }

    /// 부트로더 리락 — `oem lock`
    #[cfg(test)]
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
        if len == 0 {
            return Err("빈 이미지는 전송하지 않습니다".into());
        }
        if len > MAX_DOWNLOAD {
            return Err(format!(
                "이미지가 너무 큽니다({len}바이트 > {MAX_DOWNLOAD})"
            ));
        }
        // 실제로 보내는 명령 그대로 기록한다
        (self.on_log)(format!("> download:{len:08x}"));
        self.transport
            .write_command(&format!("download:{len:08x}"))?;
        match self.read_terminal(RESPONSE_TIMEOUT)? {
            Terminal::Data(offer) if offer == len => {}
            Terminal::Data(offer) => {
                // 크기 불일치 — 본문을 보내면 기기 상태가 꼬일 수 있으니 즉시 중단
                return Err(format!(
                    "DATA 크기 불일치: 기기 {offer}바이트, 요청 {len}바이트"
                ));
            }
            Terminal::Fail(r) => return Err(format!("다운로드 거부(FAIL): {r}")),
            Terminal::Ok(_) => return Err("본문 없이 OKAY(비정상)".into()),
        }
        self.transport.write_data(data)?;
        match self.read_terminal(LONG_RESPONSE_TIMEOUT)? {
            Terminal::Ok(_) => Ok(()),
            Terminal::Fail(r) => Err(format!("전송 후 기기 거부(FAIL): {r}")),
            Terminal::Data(_) => Err("본문 후 DATA(비정상)".into()),
        }
    }

    /// 파티션 기록 — download 후 `flash:<partition>`
    pub fn flash(&mut self, partition: &str, image: &[u8]) -> Result<(), String> {
        validate_partition(partition)?;
        // max-download-size 확인(알 수 없으면 상한만)
        if let Some(max) = self.getvar("max-download-size")? {
            let cap = parse_size(&max)
                .filter(|n| *n > 0)
                .ok_or("기기 다운로드 상한을 해석할 수 없습니다")?;
            if image.len() as u64 > cap {
                return Err(format!(
                    "이미지({}바이트)가 기기 다운로드 상한({cap}바이트)을 넘습니다",
                    image.len()
                ));
            }
        }
        self.download(image)?;
        self.expect_ok_with(&format!("flash:{partition}"), LONG_RESPONSE_TIMEOUT)?;
        Ok(())
    }

    /// 재부팅 — target "os"(reboot) | "bootloader"(reboot-bootloader)
    pub fn reboot(&mut self, target: &str) -> Result<(), String> {
        let cmd = match target {
            "os" => "reboot",
            "bootloader" => "reboot-bootloader",
            other => return Err(format!("알 수 없는 재부팅 대상: {other}")),
        };
        // FAIL/DATA/타임아웃은 성공 확인이 아니다.
        self.expect_ok(cmd).map(|_| ())
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
        Raw(&'static [u8]),
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
        /// read_frame마다 받은 대기 시간
        pub timeouts: Vec<std::time::Duration>,
        pub latency: Duration,
    }

    impl FakeTransport {
        pub fn new(frames: Vec<Frame>) -> Self {
            Self {
                sent_cmds: vec![],
                sent_data: vec![],
                frames: frames.into(),
                timeouts: vec![],
                latency: Duration::ZERO,
            }
        }
    }

    impl FastbootTransport for FakeTransport {
        fn write_command(&mut self, cmd: &str) -> Result<(), String> {
            self.sent_cmds.push(cmd.to_string());
            Ok(())
        }

        fn read_frame(
            &mut self,
            buf: &mut [u8; 256],
            timeout: std::time::Duration,
        ) -> Result<usize, String> {
            self.timeouts.push(timeout);
            if !self.latency.is_zero() {
                std::thread::sleep(self.latency.min(timeout));
                if self.latency >= timeout {
                    return Err("fastboot 응답 시간 초과".into());
                }
            }
            let Some(frame) = self.frames.pop_front() else {
                return Err("fastboot 응답 시간 초과".into());
            };
            let (status, payload): ([u8; 4], String) = match frame {
                Frame::Raw(bytes) => {
                    if bytes.len() > buf.len() {
                        return Err("프레임이 너무 큽니다".into());
                    }
                    buf[..bytes.len()].copy_from_slice(bytes);
                    return Ok(bytes.len());
                }
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

    fn dev_with(
        frames: Vec<Frame>,
    ) -> (
        FastbootDevice<FakeTransport>,
        std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    ) {
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
            Frame::Info("slot-successful:a: yes"),
            Frame::Info("slot-retry-count:b:3"),
            Frame::Ok(""),
        ]);
        let vars = d.getvar_all().unwrap();
        assert_eq!(
            vars.get("version-bootloader").map(String::as_str),
            Some("1.0")
        );
        assert_eq!(vars.get("unlocked").map(String::as_str), Some("yes"));
        assert_eq!(vars.get("current-slot").map(String::as_str), Some("a"));
        assert_eq!(
            vars.get("slot-successful:a").map(String::as_str),
            Some("yes")
        );
        assert_eq!(
            vars.get("slot-retry-count:b").map(String::as_str),
            Some("3")
        );
        assert_eq!(logs.lock().unwrap().len(), 6);
    }

    #[test]
    fn single_getvar_fail_is_none() {
        let (mut d, _) = dev_with(vec![Frame::Fail("no value")]);
        assert_eq!(d.getvar("nonexistent").unwrap(), None);
    }

    #[test]
    fn unknown_unlock_state_cannot_be_mistaken_for_locked() {
        for frame in [
            Frame::Timeout,
            Frame::Fail("unsupported"),
            Frame::Ok(""),
            Frame::Ok("unknown"),
        ] {
            let (mut d, _) = dev_with(vec![frame]);
            assert!(d.unlocked().is_err());
        }
        let (mut d, _) = dev_with(vec![Frame::Ok("no")]);
        assert!(!d.unlocked().unwrap());
        let (mut d, _) = dev_with(vec![Frame::Ok("yes")]);
        assert!(d.unlocked().unwrap());
    }

    #[test]
    fn fastbootd_and_unknown_mode_cannot_pass_bootloader_gate() {
        for frame in [
            Frame::Ok("yes"),
            Frame::Fail("unknown variable"),
            Frame::Timeout,
            Frame::Ok(""),
        ] {
            let (mut d, _) = dev_with(vec![frame]);
            assert!(d.ensure_bootloader().is_err());
        }
        let (mut d, _) = dev_with(vec![Frame::Ok("no")]);
        d.ensure_bootloader().unwrap();
    }

    #[test]
    fn identifier_getvar_does_not_log_the_value() {
        let (mut d, logs) = dev_with(vec![Frame::Ok("AB12345678")]);
        assert_eq!(d.getvar("serialno").unwrap().as_deref(), Some("AB12345678"));
        assert!(!logs
            .lock()
            .unwrap()
            .iter()
            .any(|s| s.contains("AB12345678")));
    }

    #[test]
    fn transport_failure_or_bad_limit_aborts_before_download() {
        for frame in [Frame::Timeout, Frame::Ok("invalid"), Frame::Ok("0")] {
            let (mut d, _) = dev_with(vec![frame]);
            assert!(d.flash("boot_a", b"image").is_err());
            assert_eq!(d.transport.sent_cmds, ["getvar:max-download-size"]);
            assert!(d.transport.sent_data.is_empty());
        }
    }

    #[test]
    fn empty_download_does_not_send_commands() {
        let (mut d, _) = dev_with(vec![]);
        assert!(d.download(&[]).is_err());
        assert!(d.transport.sent_cmds.is_empty());
    }

    #[test]
    fn malformed_data_frames_never_send_payload() {
        for raw in [
            b"DATA1".as_slice(),
            b"DATA00000004 ",
            b"DATAzzzzzzzz",
            b"XYZ!",
            b"OK",
        ] {
            let (mut d, _) = dev_with(vec![Frame::Raw(raw)]);
            assert!(d.download(b"data").is_err());
            assert!(d.transport.sent_data.is_empty());
        }
    }

    #[test]
    fn text_progress_does_not_interrupt_the_command() {
        let (mut d, logs) = dev_with(vec![Frame::Raw(b"TEXTworking"), Frame::Ok("yes")]);
        assert!(d.unlocked().unwrap());
        assert!(logs.lock().unwrap().iter().any(|s| s == "working"));
    }

    #[test]
    fn exactly_256_info_frames_allow_the_terminal_response() {
        let mut frames = vec![Frame::Info("progress"); MAX_INFO_FRAMES];
        frames.push(Frame::Ok("yes"));
        let (mut d, _) = dev_with(frames.clone());
        assert_eq!(d.getvar("unlocked").unwrap().as_deref(), Some("yes"));
        let (mut d, _) = dev_with(frames);
        d.getvar_all().unwrap();
    }

    #[test]
    fn unlock_validates_and_sends_sony_format() {
        let (mut d, _) = dev_with(vec![Frame::Ok("")]);
        d.oem_unlock("1234567890abcdef").unwrap();
        assert_eq!(d.transport.sent_cmds[0], "oem unlock 0x1234567890abcdef");
        // 초기화가 끝날 때까지 기다린다(10초 고정 대기로 성공을 실패로 기록하지 않는다)
        assert_eq!(d.transport.timeouts.len(), 1);
        assert!(
            d.transport.timeouts[0] <= LONG_RESPONSE_TIMEOUT
                && d.transport.timeouts[0] > RESPONSE_TIMEOUT
        );

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
        assert_eq!(d.transport.timeouts.len(), 4);
        for (actual, limit) in d.transport.timeouts.iter().zip([
            RESPONSE_TIMEOUT,
            RESPONSE_TIMEOUT,
            LONG_RESPONSE_TIMEOUT,
            LONG_RESPONSE_TIMEOUT,
        ]) {
            assert!(*actual <= limit && *actual > limit - Duration::from_secs(1));
        }
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
    fn slow_info_does_not_restart_the_response_deadline() {
        let (mut d, _) = dev_with(vec![Frame::Info("working"); 20]);
        d.transport.latency = Duration::from_millis(6);
        let err = d.read_terminal(Duration::from_millis(15)).unwrap_err();
        assert!(err.contains("시간 초과"));
        assert!(d.transport.timeouts.len() < 20);
        assert!(d.transport.timeouts.windows(2).all(|t| t[1] < t[0]));
    }

    #[test]
    fn timeout_maps_to_error() {
        let (mut d, _) = dev_with(vec![Frame::Timeout]);
        let err = d.expect_ok("reboot").unwrap_err();
        assert!(err.contains("시간 초과"));
    }

    #[test]
    fn reboot_requires_okay() {
        for frame in [Frame::Timeout, Frame::Fail("denied"), Frame::Data(Some(4))] {
            let (mut d, _) = dev_with(vec![frame]);
            assert!(d.reboot("os").is_err());
        }
        let (mut d, _) = dev_with(vec![Frame::Ok("")]);
        d.reboot("os").unwrap();
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
