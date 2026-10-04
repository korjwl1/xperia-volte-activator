//! Magisk 부트 패치 핵심 로직 — 2026-10-03 실측 절차(계약 문서 참조).
//! 전 함수가 `&mut dyn ADBDeviceExt`를 받는 주입형(단위 테스트는 FakeADBDevice).
//! 기기 작업 폴더는 고정 경로 — rm -rf도 그 경로만.

use crate::firmware::zip_extract_named;
use adb_client::ADBDeviceExt;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

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
const EXECUTABLES: &[&str] = &["magiskboot", "magiskinit", "magisk", "init-ld", "busybox", "boot_patch.sh", "util_functions.sh"];

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

fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

fn shell_out(dev: &mut dyn ADBDeviceExt, cmd: &str) -> Result<(String, Option<u8>), String> {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = dev
        .shell_command(&cmd, Some(&mut out), Some(&mut err))
        .map_err(|e| format!("{e}: {}", String::from_utf8_lossy(&err).trim()))?;
    Ok((String::from_utf8_lossy(&out).to_string(), code))
}

/// 실패 판정은 종료 코드 + stderr — 기존(settings.rs run)과 같은 규칙. 출력 문자열 휴리스틱 없음
fn must_ok(dev: &mut dyn ADBDeviceExt, cmd: &str) -> Result<String, String> {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = dev
        .shell_command(&cmd, Some(&mut out), Some(&mut err))
        .map_err(|e| format!("{e}: {}", String::from_utf8_lossy(&err).trim()))?;
    if matches!(code, Some(c) if c != 0) {
        return Err(format!("명령 실패(`{cmd}`): {}", String::from_utf8_lossy(&out).trim()));
    }
    if !err.is_empty() {
        return Err(format!("명령 실패(`{cmd}`): {}", String::from_utf8_lossy(&err).trim()));
    }
    Ok(String::from_utf8_lossy(&out).to_string())
}

/// APK에서 패치 도구를 PC 임시 폴더로 추출 → (기기 측 이름, 로컬 경로) 목록
pub fn extract_payloads(apk: &Path, out_dir: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    std::fs::create_dir_all(out_dir).map_err(|e| format!("추출 폴더 생성 실패: {e}"))?;
    let mut zip = crate::firmware::LocalZip::open(apk)?;
    let names: Vec<&str> = APK_ENTRIES.iter().map(|(n, _)| *n).collect();
    let entries = zip_extract_named(&mut zip, &names)?;
    let mut out = Vec::with_capacity(entries.len());
    for (apk_name, data) in entries {
        let dev_name = APK_ENTRIES
            .iter()
            .find(|(n, _)| *n == apk_name)
            .map(|(_, d)| d.to_string())
            .unwrap_or_else(|| apk_name.rsplit('/').next().unwrap_or("file").to_string());
        let path = out_dir.join(&dev_name);
        std::fs::write(&path, &data).map_err(|e| format!("{dev_name} 기록 실패: {e}"))?;
        out.push((dev_name, path));
    }
    Ok(out)
}

/// 패치 실행(계약 3~5번 단계): 스테이징 → boot_patch.sh → 검증 → pull → 정리.
/// 검증: `ANDROID!` 매직 · 크기 ≤ 원본(파티션 적합) · 해시 ≠ 원본(두 번 썼다고 성공 아님 원칙).
pub fn run_patch(
    dev: &mut dyn ADBDeviceExt,
    apk: &Path,
    image: &Path,
    out_path: &Path,
    on_log: &mut dyn FnMut(String),
) -> Result<PatchOutcome, String> {
    let orig = std::fs::read(image).map_err(|e| format!("순정 이미지 읽기 실패: {e}"))?;
    let orig_sha = sha256_hex(&orig);

    // 임시 추출 — 패치 도구 (pid+나노초 — 병렬 실행 충돌 방지)
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let extract_dir = std::env::temp_dir().join(format!("xvolte-magisk-{}-{nonce}", std::process::id()));
    let payloads = extract_payloads(apk, &extract_dir);
    let result = (|| -> Result<PatchOutcome, String> {
        let payloads = payloads?;
        // 정리 후 스테이징 — 고정 경로만 rm
        must_ok(dev, &format!("rm -rf {WORKDIR}"))?;
        must_ok(dev, &format!("mkdir -p {WORKDIR}"))?;
        on_log(format!("작업 폴더 준비: {WORKDIR}"));
        // 도구 push
        for (name, path) in &payloads {
            let bytes = std::fs::read(path).map_err(|e| format!("{name} 읽기 실패: {e}"))?;
            let remote = format!("{WORKDIR}/{name}");
            let mut reader = &bytes[..];
            dev.push(&mut reader, &remote).map_err(|e| format!("{name} 전송 실패: {e}"))?;
        }
        // 순정 이미지 push — 기기 측 고정명(로컬 파일명은 셸에 넣지 않는다, §12.5)
        let remote_img = format!("{WORKDIR}/{REMOTE_BOOT_IMG}");
        {
            let mut reader = &orig[..];
            dev.push(&mut reader, &remote_img).map_err(|e| format!("순정 이미지 전송 실패: {e}"))?;
        }
        on_log("패치 도구·부트 이미지 전송 완료".into());
        // 실행 권한
        let execs = EXECUTABLES.iter().map(|e| format!("{WORKDIR}/{e}")).collect::<Vec<_>>().join(" ");
        must_ok(dev, &format!("chmod 755 {execs}"))?;
        // 패치 스크립트 — 종료 코드 판정(위장 성공 금지)
        let script = format!("cd {WORKDIR} && {PATCH_ENV} ./busybox sh -o standalone ./boot_patch.sh {REMOTE_BOOT_IMG}");
        on_log("> boot_patch.sh".into());
        let (out, code) = shell_out(dev, &script)?;
        for line in out.lines().filter(|l| !l.trim().is_empty()) {
            on_log(line.to_string());
        }
        match code {
            Some(0) => {}
            Some(c) => return Err(format!("boot_patch.sh 실패(종료 코드 {c}) — 출력을 확인해 주세요")),
            None => return Err("boot_patch.sh 종료 코드를 확인하지 못했습니다".into()),
        }
        // 결과 수신
        let remote_new = format!("{WORKDIR}/new-boot.img");
        let mut buf = Vec::new();
        dev.pull(&remote_new, &mut buf).map_err(|e| format!("new-boot.img 수신 실패: {e}"))?;
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
            on_log(format!("크기 주의: 원본 {}B → {}B (파티션 이내)", orig.len(), buf.len()));
        }
        // 저장
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("결과 폴더 생성 실패: {e}"))?;
        }
        std::fs::write(out_path, &buf).map_err(|e| format!("패치 결과 저장 실패: {e}"))?;
        Ok(PatchOutcome {
            path: out_path.to_string_lossy().to_string(),
            orig_sha256: orig_sha,
            patched_sha256: patched_sha,
            bytes: buf.len() as u64,
            log: vec![],
        })
    })();
    // 성공·실패 무관하게 기기 작업 폴더 정리(고정 경로)
    let _ = must_ok(dev, &format!("rm -rf {WORKDIR}"));
    let _ = std::fs::remove_dir_all(&extract_dir);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::fake_device::FakeADBDevice;
    use crate::firmware::tests::build_zip;

    /// Magisk APK 픽스처 + 순정 이미지 셋업 (new-boot은 fake 기기 측에 심는다)
    struct Fixture {
        dir: tempfile::TempDir,
        apk: PathBuf,
        image: PathBuf,
    }

    fn fixture(orig: &[u8]) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let apk = dir.path().join("Magisk-v30.7.apk");
        std::fs::write(
            &apk,
            build_zip(&APK_ENTRIES.iter().map(|(n, _)| (*n, n.as_bytes())).collect::<Vec<_>>()),
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
        d.add_file(&format!("{WORKDIR}/new-boot.img"), patched, 1700000000, 0o644);
        d
    }

    #[test]
    fn patch_roundtrip_verified_and_cleaned() {
        let orig = vec![0x41u8; 4096];
        let patched = {
            let mut p = b"ANDROID!".to_vec();
            p.extend_from_slice(&[0xBB; 4088]);
            p
        };
        let f = fixture(&orig);
        let mut d = dev_ready(&patched);
        let out = f.dir.path().join("patched.img");
        let mut logs: Vec<String> = vec![];
        let r = run_patch(&mut d, &f.apk, &f.image, &out, &mut |l| logs.push(l)).unwrap();
        // 검증
        assert_eq!(r.bytes, 4096);
        assert_ne!(r.orig_sha256, r.patched_sha256);
        assert_eq!(std::fs::read(&out).unwrap(), patched);
        // 스테이징: 페이로드 8종 + 이미지(기기 측 고정명 — 로컬 파일명 미사용)
        for (_, name) in APK_ENTRIES {
            assert!(d.pushed.contains_key(&format!("{WORKDIR}/{name}")), "{name} 미전송");
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
        assert!(d.shell_calls.iter().filter(|c| c.contains("rm -rf /data/local/tmp/xvolte-magisk")).count() >= 2);
        // 로그에 실측 마커
        assert!(logs.iter().any(|l| l.contains("Stock boot image detected")));
    }

    #[test]
    fn wrong_magic_fails_but_cleans_up() {
        let orig = vec![0x41u8; 1024];
        let bad = b"NOTANDROID........".to_vec();
        let f = fixture(&orig);
        let mut d = dev_ready(&bad);
        let out = f.dir.path().join("patched.img");
        let err = run_patch(&mut d, &f.apk, &f.image, &out, &mut |_| {}).unwrap_err();
        assert!(err.contains("ANDROID"));
        assert!(!out.exists());
        assert!(d.shell_calls.iter().any(|c| c.contains("rm -rf /data/local/tmp/xvolte-magisk")));
    }

    #[test]
    fn identical_output_fails() {
        let orig = {
            let mut o = b"ANDROID!".to_vec();
            o.extend_from_slice(&[0x41; 1016]);
            o
        };
        let f = fixture(&orig);
        let mut d = dev_ready(&orig); // 패치 결과 == 원본
        let out = f.dir.path().join("patched.img");
        let err = run_patch(&mut d, &f.apk, &f.image, &out, &mut |_| {}).unwrap_err();
        assert!(err.contains("동일"));
    }

    #[test]
    fn oversize_output_fails() {
        let orig = vec![0x41u8; 512];
        let mut patched = b"ANDROID!".to_vec();
        patched.extend_from_slice(&[0xCC; 2048]);
        let f = fixture(&orig);
        let mut d = dev_ready(&patched);
        let out = f.dir.path().join("patched.img");
        let err = run_patch(&mut d, &f.apk, &f.image, &out, &mut |_| {}).unwrap_err();
        assert!(err.contains("파티션에 넣을 수 없습니다"));
    }

    #[test]
    fn tiny_output_fails_lower_bound() {
        // 매직은 맞지만 터무니없이 작은 결과 — 하한 없이는 성공으로 오판할 수 있다
        let orig = vec![0x41u8; 8192];
        let tiny = b"ANDROID!".to_vec();
        let f = fixture(&orig);
        let mut d = dev_ready(&tiny);
        let out = f.dir.path().join("patched.img");
        let err = run_patch(&mut d, &f.apk, &f.image, &out, &mut |_| {}).unwrap_err();
        assert!(err.contains("절반보다 작습니다"));
        assert!(!out.exists());
    }

    #[test]
    fn weird_local_filename_is_never_interpolated() {
        // 로컬 이미지명에 셸 특수문자가 있어도 기기 측 명령은 고정명만 쓴다
        let orig = vec![0x41u8; 2048];
        let mut patched = b"ANDROID!".to_vec();
        patched.extend_from_slice(&[0xBB; 2040]);
        let dir = tempfile::tempdir().unwrap();
        let apk = dir.path().join("Magisk-v30.7.apk");
        std::fs::write(
            &apk,
            build_zip(&APK_ENTRIES.iter().map(|(n, _)| (*n, n.as_bytes())).collect::<Vec<_>>()),
        )
        .unwrap();
        let image = dir.path().join("my boot;rm -rf .img"); // 위험한 로컬명
        std::fs::write(&image, &orig).unwrap();
        let mut d = dev_ready(&patched);
        let out = dir.path().join("patched.img");
        run_patch(&mut d, &apk, &image, &out, &mut |_| {}).unwrap();
        let script = d.shell_calls.iter().find(|c| c.contains("boot_patch.sh boot.img")).unwrap();
        assert!(!script.contains("my boot"));
        assert!(!script.contains(";"));
        assert!(d.pushed.contains_key(&format!("{WORKDIR}/boot.img")));
    }

    #[test]
    fn missing_payload_entry_is_error() {
        // busybox 누락 APK
        let dir = tempfile::tempdir().unwrap();
        let apk = dir.path().join("broken.apk");
        let partial: Vec<(&str, &[u8])> = APK_ENTRIES.iter().skip(1).map(|(n, _)| (*n, n.as_bytes())).collect();
        std::fs::write(&apk, build_zip(&partial)).unwrap();
        let out = dir.path().join("x");
        assert!(extract_payloads(&apk, &out).is_err());
    }
}
