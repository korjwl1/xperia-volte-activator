//! Magisk 부트 패치 핵심 로직 — 2026-10-03 실측 절차(계약 문서 참조).
//! 전 함수가 `&mut dyn ADBDeviceExt`를 받는 주입형(단위 테스트는 FakeADBDevice).
//! 기기 작업 폴더는 고정 경로 — rm -rf도 그 경로만.

use crate::boot_image::sha256 as sha256_hex;
use crate::device_io::shell_write;
use crate::firmware::zip_extract_named;
use adb_client::ADBDeviceExt;
use std::io::Write;
use std::path::Path;

pub const WORKDIR: &str = "/data/local/tmp/xvolte-magisk";
/// 기기 측 부트 이미지명 — 로컬 파일명과 무관한 고정명(셸 보간 없음, §12.5)
const REMOTE_BOOT_IMG: &str = "boot.img";

/// APK 항목 → 기기 측 파일명 (계약 2번 단계, arm64 고정)
pub const APK_ENTRIES: &[(&str, &str)] = &[
    ("lib/arm64-v8a/libmagiskboot.so", "magiskboot"),
    ("lib/arm64-v8a/libmagiskinit.so", "magiskinit"),
    ("lib/arm64-v8a/libmagisk.so", "magisk"),
    ("lib/arm64-v8a/libinit-ld.so", "init-ld"),
    ("lib/arm64-v8a/libbusybox.so", "busybox"),
    ("assets/boot_patch.sh", "boot_patch.sh"),
    ("assets/util_functions.sh", "util_functions.sh"),
    ("assets/stub.apk", "stub.apk"),
];

/// 실행 권한 필요 항목(stub.apk 제외 전부)
fn executables() -> impl Iterator<Item = &'static str> {
    APK_ENTRIES
        .iter()
        .map(|(_, dev_name)| *dev_name)
        .filter(|dev_name| *dev_name != "stub.apk")
}

/// 검증된 환경 변수 세트 — v30.7 실측 그대로 (계약 4번 단계)
const PATCH_ENV: &str = "KEEPVERITY=true KEEPFORCEENCRYPT=true PATCHVBMETAFLAG=false RECOVERYMODE=false LEGACYSAR=false";

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchOutcome {
    pub path: String,
    pub orig_sha256: String,
    pub patched_sha256: String,
    pub bytes: u64,
    pub log: Vec<String>,
}

/// 출력(stdout+stderr, 손상 바이트는 대체)과 종료 코드 — 로그 보존이 목적이라 UTF-8 오류로 실패하지 않는다
fn shell_out(dev: &mut dyn ADBDeviceExt, cmd: &str) -> Result<(String, u8), String> {
    let out = crate::device_io::shell_run(dev, cmd)?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok((text, out.code))
}

/// 상한을 넘는 쓰기를 거부하고 그 사실을 기억한다. adb 서버 경로의 pull은 마지막 버퍼를
/// drop 시점에 쓰며 그 오류를 버리므로, 반환값 대신 `overflow`로 초과를 판정해야 한다.
struct LimitedImage {
    bytes: Vec<u8>,
    max: usize,
    overflow: bool,
}
impl Write for LimitedImage {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.max.saturating_sub(self.bytes.len()) {
            self.overflow = true;
            return Err(std::io::Error::other(
                "패치 결과가 원본 이미지의 최대 크기를 초과했습니다",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// APK에서 패치 도구를 메모리로 추출 → (기기 측 이름, 내용) 목록.
/// PC 임시 폴더를 거치지 않는다(백신의 임시 파일 격리·잔류 파일 방지).
/// 입력은 해시·서명 검증을 마친 APK 바이트 — 검증한 내용과 실행하는 내용이 같다.
pub fn extract_payloads(apk: &[u8]) -> Result<Vec<(&'static str, Vec<u8>)>, String> {
    let mut zip = crate::firmware::MemZip(apk);
    let names: Vec<&str> = APK_ENTRIES.iter().map(|(n, _)| *n).collect();
    zip_extract_named(&mut zip, &names)?
        .into_iter()
        .map(|(apk_name, data)| {
            APK_ENTRIES
                .iter()
                .find(|(n, _)| *n == apk_name)
                .map(|(_, dev_name)| (*dev_name, data))
                .ok_or_else(|| format!("APK 항목이 예상과 다릅니다: {apk_name}"))
        })
        .collect()
}

/// 기기 측 결과 파일을 크기 확인 후 받는다. 원본보다 크면 받기 전에 거부하고,
/// 받은 길이가 기기 측 크기와 다르면(잘림·초과) 실패한다.
fn pull_patched(dev: &mut dyn ADBDeviceExt, remote: &str, max: usize) -> Result<Vec<u8>, String> {
    let remote_len = dev
        .stat(&remote)
        .map_err(|e| format!("new-boot.img 크기 확인 실패: {e}"))?
        .file_size as usize;
    if remote_len > max {
        return Err(format!(
            "패치 결과({remote_len}B)가 원본 이미지의 최대 크기를 초과했습니다({max}B) — 파티션에 넣을 수 없습니다"
        ));
    }
    let mut writer = LimitedImage {
        bytes: Vec::with_capacity(remote_len),
        max: remote_len,
        overflow: false,
    };
    dev.pull(&remote, &mut writer)
        .map_err(|e| format!("new-boot.img 수신 실패: {e}"))?;
    if writer.overflow || writer.bytes.len() != remote_len {
        return Err(format!(
            "new-boot.img 수신 크기({}B)가 기기 측 크기({remote_len}B)와 다릅니다",
            writer.bytes.len()
        ));
    }
    Ok(writer.bytes)
}

/// 패치 실행(계약 3~5번 단계): 스테이징 → boot_patch.sh → 검증 → pull → 정리.
/// 검증: `ANDROID!` 매직 · 크기 ≤ 원본(파티션 적합) · 해시 ≠ 원본(두 번 썼다고 성공 아님 원칙).
pub fn run_patch(
    dev: &mut dyn ADBDeviceExt,
    apk: &[u8],
    image: &Path,
    out_path: &Path,
    expected_sha256: Option<&str>,
    on_log: &mut dyn FnMut(String),
) -> Result<PatchOutcome, String> {
    let orig = crate::boot_image::read(image)?;
    let orig_sha = sha256_hex(&orig);
    if expected_sha256.is_some_and(|hash| hash != orig_sha) {
        return Err("부트 이미지가 사전 검사 후 변경됐습니다".into());
    }
    // 기기를 건드리기 전에 도구 추출부터 — 손상된 APK면 기기 명령 없이 실패
    let payloads = extract_payloads(apk)?;
    let result = (|| -> Result<PatchOutcome, String> {
        // 정리 후 스테이징 — 고정 경로만 rm
        shell_write(dev, &format!("rm -rf {WORKDIR}"))?;
        shell_write(dev, &format!("mkdir -p {WORKDIR}"))?;
        on_log(format!("작업 폴더 준비: {WORKDIR}"));
        // 도구 push
        for (name, bytes) in &payloads {
            let remote = format!("{WORKDIR}/{name}");
            let mut reader = &bytes[..];
            dev.push(&mut reader, &remote)
                .map_err(|e| format!("{name} 전송 실패: {e}"))?;
        }
        // 순정 이미지 push — 기기 측 고정명(로컬 파일명은 셸에 넣지 않는다, §12.5)
        let remote_img = format!("{WORKDIR}/{REMOTE_BOOT_IMG}");
        {
            let mut reader = &orig[..];
            dev.push(&mut reader, &remote_img)
                .map_err(|e| format!("순정 이미지 전송 실패: {e}"))?;
        }
        on_log("패치 도구·부트 이미지 전송 완료".into());
        // 실행 권한
        let execs = executables()
            .map(|e| format!("{WORKDIR}/{e}"))
            .collect::<Vec<_>>()
            .join(" ");
        shell_write(dev, &format!("chmod 755 {execs}"))?;
        // 패치 스크립트 — 종료 코드 판정(위장 성공 금지)
        let script = format!("cd {WORKDIR} && {PATCH_ENV} ./busybox sh -o standalone ./boot_patch.sh {REMOTE_BOOT_IMG}");
        on_log("> boot_patch.sh".into());
        let (out, code) = shell_out(dev, &script)?;
        for line in out.lines().filter(|l| !l.trim().is_empty()) {
            on_log(line.to_string());
        }
        if code != 0 {
            return Err(format!(
                "boot_patch.sh 실패(종료 코드 {code}) — 출력을 확인해 주세요"
            ));
        }
        // 결과 수신
        let remote_new = format!("{WORKDIR}/new-boot.img");
        let buf = pull_patched(dev, &remote_new, orig.len())?;
        // 검증 — 매직·크기 상하한·해시. 하한 없이는 9바이트 "ANDROID!"도 통과해 버릴 수 있다
        if !buf.starts_with(b"ANDROID!") {
            return Err("패치 결과가 ANDROID! 부트 이미지가 아닙니다".into());
        }
        if buf.len() as u64 > orig.len() as u64 {
            return Err(format!(
                "패치 결과({}B)가 순정 이미지({}B)보다 큽니다 — 파티션에 넣을 수 없습니다",
                buf.len(),
                orig.len()
            ));
        }
        if (buf.len() as u64) * 2 < orig.len() as u64 {
            return Err(format!(
                "패치 결과({}B)가 순정 이미지({}B)의 절반보다 작습니다 — 정상 패치 결과로 볼 수 없습니다",
                buf.len(),
                orig.len()
            ));
        }
        let patched_sha = sha256_hex(&buf);
        if patched_sha == orig_sha {
            return Err("패치 결과가 순정 이미지와 동일합니다 — 패치가 적용되지 않았습니다".into());
        }
        if buf.len() as u64 == orig.len() as u64 {
            on_log(format!("크기 확인: 원본과 동일({}B)", buf.len()));
        } else {
            on_log(format!(
                "크기 주의: 원본 {}B → {}B (파티션 이내)",
                orig.len(),
                buf.len()
            ));
        }
        // 저장(atomic_write가 상위 폴더를 만든다)
        let output = out_path.with_file_name(format!("patched-{patched_sha}.img"));
        crate::storage::atomic_write(&output, &buf)
            .map_err(|e| format!("패치 결과 저장 실패: {e}"))?;
        Ok(PatchOutcome {
            path: output.to_string_lossy().to_string(),
            orig_sha256: orig_sha,
            patched_sha256: patched_sha,
            bytes: buf.len() as u64,
            log: vec![],
        })
    })();
    // 성공·실패 무관하게 기기 작업 폴더 정리(고정 경로)
    if let Err(error) = shell_write(dev, &format!("rm -rf {WORKDIR}")) {
        on_log(format!("[경고] 기기 패치 작업 폴더 정리 실패: {error}"));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::fake_device::FakeADBDevice;
    use crate::firmware::tests::build_zip;
    use std::fs::read;
    use std::path::PathBuf;

    /// Magisk APK 픽스처 + 순정 이미지 셋업 (new-boot은 fake 기기 측에 심는다)
    struct Fixture {
        dir: tempfile::TempDir,
        apk: PathBuf,
        image: PathBuf,
    }

    fn image(len: usize, fill: u8) -> Vec<u8> {
        let mut bytes = vec![fill; len];
        bytes[..8].copy_from_slice(b"ANDROID!");
        bytes
    }

    fn fixture(orig: &[u8]) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let apk = dir.path().join("Magisk-v30.7.apk");
        std::fs::write(
            &apk,
            build_zip(
                &APK_ENTRIES
                    .iter()
                    .map(|(n, _)| (*n, n.as_bytes()))
                    .collect::<Vec<_>>(),
            ),
        )
        .unwrap();
        let image = dir.path().join("init_boot.img");
        std::fs::write(&image, orig).unwrap();
        Fixture { dir, apk, image }
    }

    fn dev_ready(patched: &[u8]) -> FakeADBDevice {
        let mut d = FakeADBDevice::new();
        d.answer_shell("rm -rf", "");
        d.answer_shell("mkdir", "");
        d.answer_shell("chmod", "");
        d.answer_shell("cd /data/local/tmp/xvolte-magisk", "- Checking ramdisk status\n- Stock boot image detected\n- Patching ramdisk\n- Repack boot image\n");
        // boot_patch.sh가 만든 결과물을 기기에 심는다
        d.add_dir(WORKDIR);
        d.add_file(
            &format!("{WORKDIR}/new-boot.img"),
            patched,
            1700000000,
            0o644,
        );
        d
    }

    #[test]
    fn patch_roundtrip_verified_and_cleaned() {
        let orig = image(4096, 0x41);
        let patched = {
            let mut p = b"ANDROID!".to_vec();
            p.extend_from_slice(&[0xBB; 4088]);
            p
        };
        let f = fixture(&orig);
        let mut d = dev_ready(&patched);
        let out = f.dir.path().join("patched.img");
        let mut logs: Vec<String> = vec![];
        let r = run_patch(
            &mut d,
            &read(&f.apk).unwrap(),
            &f.image,
            &out,
            None,
            &mut |l| logs.push(l),
        )
        .unwrap();
        // 검증
        assert_eq!(r.bytes, 4096);
        assert_ne!(r.orig_sha256, r.patched_sha256);
        assert_eq!(std::fs::read(&r.path).unwrap(), patched);
        // 스테이징: 페이로드 8종 + 이미지(기기 측 고정명 — 로컬 파일명 미사용)
        for (_, name) in APK_ENTRIES {
            assert!(
                d.pushed.contains_key(&format!("{WORKDIR}/{name}")),
                "{name} 미전송"
            );
        }
        assert!(d.pushed.contains_key(&format!("{WORKDIR}/boot.img")));
        // 스크립트 명령 형태 — 실측 환경 변수·standalone·고정명 포함 (chmod에 파일명이 겹치니 실행 구문으로 찾는다)
        let script = d
            .shell_calls
            .iter()
            .find(|c| c.contains("./busybox sh -o standalone"))
            .expect("boot_patch.sh 실행 명령");
        assert!(script.contains("KEEPVERITY=true"));
        assert!(script.contains("KEEPFORCEENCRYPT=true"));
        assert!(script.contains("./busybox sh -o standalone ./boot_patch.sh boot.img"));
        // 마지막에 정리(고정 경로 rm 2회: 시작+종료)
        assert!(
            d.shell_calls
                .iter()
                .filter(|c| c.contains("rm -rf /data/local/tmp/xvolte-magisk"))
                .count()
                >= 2
        );
        // 로그에 실측 마커
        assert!(logs.iter().any(|l| l.contains("Stock boot image detected")));
    }

    #[test]
    fn wrong_magic_fails_but_cleans_up() {
        let orig = image(4096, 0x41);
        let bad = b"NOTANDROID........".to_vec();
        let f = fixture(&orig);
        let mut d = dev_ready(&bad);
        let out = f.dir.path().join("patched.img");
        let err = run_patch(
            &mut d,
            &read(&f.apk).unwrap(),
            &f.image,
            &out,
            None,
            &mut |_| {},
        )
        .unwrap_err();
        assert!(err.contains("ANDROID"));
        assert!(!out.exists());
        assert!(d
            .shell_calls
            .iter()
            .any(|c| c.contains("rm -rf /data/local/tmp/xvolte-magisk")));
    }

    #[test]
    fn identical_output_fails() {
        let orig = {
            let mut o = b"ANDROID!".to_vec();
            o.extend_from_slice(&[0x41; 4088]);
            o
        };
        let f = fixture(&orig);
        let mut d = dev_ready(&orig); // 패치 결과 == 원본
        let out = f.dir.path().join("patched.img");
        let err = run_patch(
            &mut d,
            &read(&f.apk).unwrap(),
            &f.image,
            &out,
            None,
            &mut |_| {},
        )
        .unwrap_err();
        assert!(err.contains("동일"));
    }

    #[test]
    fn oversize_output_fails() {
        let orig = image(4096, 0x41);
        let mut patched = b"ANDROID!".to_vec();
        patched.extend_from_slice(&[0xCC; 8192]);
        let f = fixture(&orig);
        let mut d = dev_ready(&patched);
        let out = f.dir.path().join("patched.img");
        let err = run_patch(
            &mut d,
            &read(&f.apk).unwrap(),
            &f.image,
            &out,
            None,
            &mut |_| {},
        )
        .unwrap_err();
        assert!(err.contains("최대 크기를 초과"));
    }

    #[test]
    fn tiny_output_fails_lower_bound() {
        // 매직은 맞지만 터무니없이 작은 결과 — 하한 없이는 성공으로 오판할 수 있다
        let orig = image(8192, 0x41);
        let tiny = b"ANDROID!".to_vec();
        let f = fixture(&orig);
        let mut d = dev_ready(&tiny);
        let out = f.dir.path().join("patched.img");
        let err = run_patch(
            &mut d,
            &read(&f.apk).unwrap(),
            &f.image,
            &out,
            None,
            &mut |_| {},
        )
        .unwrap_err();
        assert!(err.contains("절반보다 작습니다"));
        assert!(!out.exists());
    }

    #[test]
    fn weird_local_filename_is_never_interpolated() {
        // 로컬 이미지명에 셸 특수문자가 있어도 기기 측 명령은 고정명만 쓴다
        let orig = image(4096, 0x41);
        let mut patched = b"ANDROID!".to_vec();
        patched.extend_from_slice(&[0xBB; 4088]);
        let dir = tempfile::tempdir().unwrap();
        let apk = dir.path().join("Magisk-v30.7.apk");
        std::fs::write(
            &apk,
            build_zip(
                &APK_ENTRIES
                    .iter()
                    .map(|(n, _)| (*n, n.as_bytes()))
                    .collect::<Vec<_>>(),
            ),
        )
        .unwrap();
        let image = dir.path().join("my boot;rm -rf .img"); // 위험한 로컬명
        std::fs::write(&image, &orig).unwrap();
        let mut d = dev_ready(&patched);
        let out = dir.path().join("patched.img");
        run_patch(
            &mut d,
            &read(&apk).unwrap(),
            &image,
            &out,
            None,
            &mut |_| {},
        )
        .unwrap();
        let script = d
            .shell_calls
            .iter()
            .find(|c| c.contains("boot_patch.sh boot.img"))
            .unwrap();
        assert!(!script.contains("my boot"));
        assert!(!script.contains(";"));
        assert!(d.pushed.contains_key(&format!("{WORKDIR}/boot.img")));
    }

    #[test]
    fn changed_source_and_invalid_apk_do_not_touch_the_device() {
        let original = image(4096, 0x41);
        let fixture = fixture(&original);
        let mut device = dev_ready(&image(4096, 0xBB));
        let output = fixture.dir.path().join("patched.img");
        assert!(run_patch(
            &mut device,
            &read(&fixture.apk).unwrap(),
            &fixture.image,
            &output,
            Some(&"f".repeat(64)),
            &mut |_| {}
        )
        .is_err());
        assert!(device.shell_calls.is_empty());
        assert!(device.pushed.is_empty());
        std::fs::write(&fixture.apk, b"corrupt APK").unwrap();
        assert!(run_patch(
            &mut device,
            &read(&fixture.apk).unwrap(),
            &fixture.image,
            &output,
            None,
            &mut |_| {}
        )
        .is_err());
        assert!(device.shell_calls.is_empty());
    }

    #[test]
    fn missing_payload_entry_is_error() {
        // busybox 누락 APK
        let dir = tempfile::tempdir().unwrap();
        let apk = dir.path().join("broken.apk");
        let partial: Vec<(&str, &[u8])> = APK_ENTRIES
            .iter()
            .skip(1)
            .map(|(n, _)| (*n, n.as_bytes()))
            .collect();
        std::fs::write(&apk, build_zip(&partial)).unwrap();
        assert!(extract_payloads(&read(&apk).unwrap()).is_err());
    }

    #[test]
    fn limited_image_remembers_rejected_writes() {
        // adb 서버 경로는 마지막 버퍼의 쓰기 오류를 버린다 — 오류가 사라져도 초과는 남아야 한다
        let mut w = LimitedImage {
            bytes: Vec::new(),
            max: 4,
            overflow: false,
        };
        assert!(w.write(b"ABCD").is_ok());
        let _ = w.write(b"E");
        assert!(w.overflow);
        assert_eq!(w.bytes, b"ABCD");
    }
}
