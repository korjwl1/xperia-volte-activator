//! 다운로드 APK(Magisk·sms-ie) 무결성 검증 — GitHub 자산 다이제스트 + 서명 인증서 핀.
//!
//! 1. 다이제스트: GitHub 릴리스 API가 자산마다 주는 `"digest": "sha256:<hex>"`·`"size"`와
//!    받은 바이트를 대조한 뒤에만 캐시에 저장한다(잘못 받은 파일은 캐시에 남지 않는다).
//!    캐시에는 API 다이제스트를 옆 파일(`<apk>.sha256`)로 남기고, 재사용 때 파일 해시와 대조한다.
//! 2. 인증서 핀: APK Signing Block(v2/v3)의 서명자 인증서 SHA-256이 공식 배포자의 것인지 확인한다.
//!    이것은 서명 자체의 암호 검증이 아니다(인증서 블록은 복사할 수 있다). 앱 설치 때는 Android가
//!    서명을 검증하므로 위조 APK는 설치되지 않는다. Magisk에서 꺼내 기기에서 실행하는 바이너리는
//!    설치를 거치지 않으므로 GitHub 다이제스트(또는 다이제스트를 기록한 캐시)가 무결성의 근거다.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Magisk(topjohnwu) 공식 릴리스 서명 인증서 SHA-256.
/// 출처: 2026-10-04 github.com/topjohnwu/Magisk/releases/download/v30.7/Magisk-v30.7.apk
/// (API digest e0d32d21… 일치 확인)의 v2/v3 서명 블록에서 이 모듈의 파서로 계산,
/// META-INF v1 서명(PKCS#7)의 인증서를 openssl로 꺼낸 값과 교차 확인.
pub const MAGISK_CERT_SHA256: &[&str] = &[MAGISK_PIN];
const MAGISK_PIN: &str = "b4cb83b4dad99f997dbe872f013aa16c14eec41d167021f371f7e1330f273ee6";

/// SMS Import/Export(tmo1) GitHub 릴리스 서명 인증서 SHA-256.
/// 출처: 2026-10-04 github.com/tmo1/sms-ie/releases/download/v2.11.1/
/// com.github.tmo1.sms_ie-v2.11.1-standard-release.apk(API digest 9c93a839… 일치 확인)에서 같은 방법으로 계산.
/// F-Droid 빌드는 다른 키로 서명되므로 여기에 해당하지 않는다.
pub const SMSIE_CERT_SHA256: &[&str] = &[SMSIE_PIN];
const SMSIE_PIN: &str = "c105e6d9675542d434a9cde9dc79b249f1ac0afc3b8aaec7d5c8201136cffdbe";

/// 릴리스 자산 하나 — 이름·내려받기 주소·기대 해시(소문자 hex)·크기
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseAsset {
    pub name: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// GitHub 자산 JSON에서 다이제스트·크기를 읽는다. 없거나 형식이 다르면 실패(검증 없이 쓰지 않는다).
pub fn asset_from_json(
    asset: &serde_json::Value,
    name: &str,
    url: &str,
    max: u64,
) -> Result<ReleaseAsset, String> {
    let digest = asset["digest"]
        .as_str()
        .and_then(|d| d.strip_prefix("sha256:"))
        .filter(|h| is_hex64(h))
        .ok_or(
            "릴리스 자산에 SHA-256 다이제스트가 없습니다 — 받은 파일을 검증할 수 없어 중단합니다",
        )?;
    let size = asset["size"]
        .as_u64()
        .filter(|s| (1..=max).contains(s))
        .ok_or("릴리스 자산 크기가 올바르지 않습니다")?;
    Ok(ReleaseAsset {
        name: name.to_string(),
        url: url.to_string(),
        sha256: digest.to_ascii_lowercase(),
        size,
    })
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// 받은 바이트가 자산 다이제스트·크기와 같은지
pub fn verify_download(bytes: &[u8], asset: &ReleaseAsset) -> Result<(), String> {
    if bytes.len() as u64 != asset.size {
        return Err(format!(
            "{}: 받은 크기({}B)가 릴리스 정보({}B)와 다릅니다",
            asset.name,
            bytes.len(),
            asset.size
        ));
    }
    if sha256_hex(bytes) != asset.sha256 {
        return Err(format!(
            "{}: 받은 파일의 SHA-256이 릴리스 다이제스트와 다릅니다 — 저장하지 않습니다",
            asset.name
        ));
    }
    Ok(())
}

fn sidecar(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".sha256");
    path.with_file_name(name)
}

/// 검증을 마친 바이트를 캐시에 저장하고 다이제스트를 옆 파일로 남긴다.
/// APK를 먼저, 다이제스트를 나중에 쓴다 — 중간에 끊기면 다이제스트가 없어 재사용되지 않는다.
pub fn store_verified(path: &Path, bytes: &[u8], sha256: &str) -> Result<(), String> {
    let _ = std::fs::remove_file(sidecar(path));
    crate::storage::atomic_write(path, bytes).map_err(|e| format!("APK 저장 실패: {e}"))?;
    crate::storage::atomic_write(&sidecar(path), sha256.as_bytes())
        .map_err(|e| format!("APK 다이제스트 저장 실패: {e}"))
}

/// 캐시 재사용 — 파일 해시가 기록된 다이제스트와 같고, `expected`(온라인일 때 현재 API 값)가 있으면
/// 그것과도 같고, 서명 인증서가 핀과 맞을 때만 내용을 돌려준다. 하나라도 어긋나면 None.
pub fn load_cached(
    path: &Path,
    expected: Option<&str>,
    pins: &[&str],
    max: usize,
) -> Option<(Vec<u8>, String)> {
    let recorded = crate::storage::read_bounded(&sidecar(path), 128).ok()??;
    let recorded = String::from_utf8(recorded)
        .ok()?
        .trim()
        .to_ascii_lowercase();
    if !is_hex64(&recorded) || expected.is_some_and(|e| !e.eq_ignore_ascii_case(&recorded)) {
        return None;
    }
    let bytes = crate::storage::read_bounded(path, max).ok()??;
    let actual = sha256_hex(&bytes);
    if actual != recorded || check_pins(&bytes, pins).is_err() {
        return None;
    }
    Some((bytes, actual))
}

/// 오프라인 대비 — 폴더에서 이름 조건에 맞는 캐시 중 검증을 통과한 가장 최근(수정 시각) 파일
pub fn newest_verified_cache(
    dir: &Path,
    matches: impl Fn(&str) -> bool,
    pins: &[&str],
    max: usize,
) -> Option<(PathBuf, Vec<u8>, String)> {
    let mut candidates: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_name().to_str().is_some_and(&matches))
        .filter_map(|entry| {
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, entry.path()))
        })
        .collect();
    candidates.sort_by_key(|c| std::cmp::Reverse(c.0));
    candidates.into_iter().find_map(|(_, path)| {
        let (bytes, sha) = load_cached(&path, None, pins, max)?;
        Some((path, bytes, sha))
    })
}

// ── APK Signing Block(v2/v3) ──

const SIG_BLOCK_MAGIC: &[u8; 16] = b"APK Sig Block 42";
const V2_BLOCK_ID: u32 = 0x7109_871a;
const V3_BLOCK_ID: u32 = 0xf053_68c0;
const MALFORMED: &str = "APK 서명 블록 형식이 올바르지 않습니다";

fn u32_at(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at.checked_add(4).ok_or(MALFORMED)?)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(|| MALFORMED.into())
}

fn u64_at(b: &[u8], at: usize) -> Result<u64, String> {
    let s = b
        .get(at..at.checked_add(8).ok_or(MALFORMED)?)
        .ok_or(MALFORMED)?;
    let mut raw = [0u8; 8];
    raw.copy_from_slice(s);
    Ok(u64::from_le_bytes(raw))
}

/// u32 길이 접두 값 하나 → (값, 나머지)
fn prefixed(b: &[u8]) -> Result<(&[u8], &[u8]), String> {
    let len = u32_at(b, 0)? as usize;
    let end = 4usize.checked_add(len).ok_or(MALFORMED)?;
    let value = b.get(4..end).ok_or(MALFORMED)?;
    Ok((value, &b[end..]))
}

/// ZIP 끝 정보 → central directory 시작 위치
fn central_directory_offset(apk: &[u8]) -> Result<usize, String> {
    const EOCD_MIN: usize = 22;
    if apk.len() < EOCD_MIN {
        return Err("APK가 ZIP 형식이 아닙니다".into());
    }
    // 주석 최대 65535바이트까지 뒤에서부터 찾는다
    let lowest = apk.len().saturating_sub(EOCD_MIN + 0xFFFF);
    let eocd = (lowest..=apk.len() - EOCD_MIN)
        .rev()
        .find(|&i| apk[i..i + 4] == [0x50, 0x4b, 0x05, 0x06])
        .ok_or("APK 끝 정보(EOCD)를 찾을 수 없습니다")?;
    let offset = u32_at(apk, eocd + 16)?;
    if offset == u32::MAX || offset as usize > eocd {
        return Err("APK central directory 위치가 올바르지 않습니다".into());
    }
    Ok(offset as usize)
}

/// v2·v3 블록 각각에서 첫 서명자의 첫 인증서(DER) SHA-256을 모은다.
/// 서명 블록이 없으면(v1 전용) 실패 — 핀을 확인할 근거가 없다.
pub fn signer_cert_sha256(apk: &[u8]) -> Result<Vec<String>, String> {
    let cd = central_directory_offset(apk)?;
    if cd < 24 || apk.get(cd - 16..cd) != Some(&SIG_BLOCK_MAGIC[..]) {
        return Err("APK 서명 블록(v2/v3)이 없습니다".into());
    }
    let size = usize::try_from(u64_at(apk, cd - 24)?).map_err(|_| MALFORMED)?;
    // 블록 = [size u64][쌍들][size u64][magic 16] — 앞쪽 size 필드까지 포함한 시작 위치
    let start = cd
        .checked_sub(size)
        .and_then(|s| s.checked_sub(8))
        .ok_or(MALFORMED)?;
    if u64_at(apk, start)? != size as u64 {
        return Err(MALFORMED.into());
    }
    let mut pairs = &apk[start + 8..cd - 24];
    let mut certs = Vec::new();
    while !pairs.is_empty() {
        let len = usize::try_from(u64_at(pairs, 0)?).map_err(|_| MALFORMED)?;
        let end = 8usize.checked_add(len).ok_or(MALFORMED)?;
        let pair = pairs.get(8..end).ok_or(MALFORMED)?;
        pairs = &pairs[end..];
        let id = u32_at(pair, 0)?;
        if id != V2_BLOCK_ID && id != V3_BLOCK_ID {
            continue;
        }
        // 값 = signers(접두) → 첫 signer(접두) → signed data(접두) → digests(접두) → certificates(접두) → 첫 인증서(접두)
        let (signers, _) = prefixed(&pair[4..])?;
        let (signer, _) = prefixed(signers)?;
        let (signed_data, _) = prefixed(signer)?;
        let (_digests, rest) = prefixed(signed_data)?;
        let (certificates, _) = prefixed(rest)?;
        let (cert, _) = prefixed(certificates)?;
        if cert.is_empty() {
            return Err(MALFORMED.into());
        }
        certs.push(sha256_hex(cert));
    }
    if certs.is_empty() {
        return Err("APK 서명 블록(v2/v3)이 없습니다".into());
    }
    Ok(certs)
}

/// 모든 v2/v3 서명자 인증서가 핀 목록에 있어야 한다
pub fn check_pins(apk: &[u8], pins: &[&str]) -> Result<(), String> {
    let certs = signer_cert_sha256(apk)?;
    if certs
        .iter()
        .all(|c| pins.iter().any(|p| p.eq_ignore_ascii_case(c)))
    {
        Ok(())
    } else {
        Err("APK 서명 인증서가 공식 배포자의 것이 아닙니다 — 사용하지 않습니다".into())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn prefix(v: &[u8]) -> Vec<u8> {
        let mut out = (v.len() as u32).to_le_bytes().to_vec();
        out.extend_from_slice(v);
        out
    }

    /// 실제 ZIP(build_zip 결과)의 central directory 앞에 서명 블록을 끼워 넣는다 — 항목은 그대로 읽힌다
    pub(crate) fn sign_zip(zip: &[u8], cert: &[u8]) -> Vec<u8> {
        let cd = central_directory_offset(zip).unwrap();
        let signed = signed_apk(cert, false);
        let block_cd = central_directory_offset(&signed).unwrap();
        // signed_apk의 서명 블록 부분만 잘라낸다(앞쪽 가짜 로컬 항목 제외)
        let block_size = u64_at(&signed, block_cd - 24).unwrap() as usize;
        let block = &signed[block_cd - block_size - 8..block_cd];
        let mut out = zip[..cd].to_vec();
        out.extend_from_slice(block);
        let new_cd = out.len() as u32;
        out.extend_from_slice(&zip[cd..]);
        let eocd = (0..out.len() - 21)
            .rev()
            .find(|&i| out[i..i + 4] == [0x50, 0x4b, 0x05, 0x06])
            .unwrap();
        out[eocd + 16..eocd + 20].copy_from_slice(&new_cd.to_le_bytes());
        out
    }

    /// 지정한 인증서로 v2(그리고 선택적으로 v3) 서명 블록을 가진 최소 APK(ZIP) 바이트
    pub(crate) fn signed_apk(cert: &[u8], with_v3: bool) -> Vec<u8> {
        let value = |id: u32| {
            let signed_data = [prefix(&[]), prefix(&prefix(cert))].concat();
            let signer = prefix(&signed_data);
            let signers = prefix(&prefix(&signer));
            let mut pair = id.to_le_bytes().to_vec();
            pair.extend_from_slice(&signers);
            let mut out = (pair.len() as u64).to_le_bytes().to_vec();
            out.extend_from_slice(&pair);
            out
        };
        let mut pairs = value(V2_BLOCK_ID);
        if with_v3 {
            pairs.extend(value(V3_BLOCK_ID));
        }
        // 다른 ID 쌍은 건너뛴다(패딩 블록 등)
        let mut other = 0x4272_7a01u32.to_le_bytes().to_vec();
        other.extend_from_slice(&[0u8; 8]);
        pairs.extend((other.len() as u64).to_le_bytes());
        pairs.extend(other);
        let size = (pairs.len() + 8 + 16) as u64;
        let mut apk = b"PK\x03\x04local-entries".to_vec();
        apk.extend(size.to_le_bytes());
        apk.extend(&pairs);
        apk.extend(size.to_le_bytes());
        apk.extend(SIG_BLOCK_MAGIC);
        let cd = apk.len() as u32;
        let mut eocd = vec![0x50, 0x4b, 0x05, 0x06];
        eocd.extend([0u8; 12]);
        eocd.extend(cd.to_le_bytes());
        eocd.extend([0u8; 2]);
        apk.extend(eocd);
        apk
    }

    #[test]
    fn signing_block_certificate_is_extracted_and_pinned() {
        let apk = signed_apk(b"certificate-der", true);
        let expected = sha256_hex(b"certificate-der");
        assert_eq!(
            signer_cert_sha256(&apk).unwrap(),
            vec![expected.clone(), expected.clone()]
        );
        assert!(check_pins(&apk, &[&expected]).is_ok());
        assert!(check_pins(&apk, &[&expected.to_ascii_uppercase()]).is_ok());
        assert!(check_pins(&apk, &[&"0".repeat(64)]).is_err());
    }

    #[test]
    fn malformed_or_unsigned_apks_never_panic_and_fail() {
        let apk = signed_apk(b"certificate-der", false);
        // 바이트를 하나씩 잘라도·뒤집어도 패닉 없이 오류 또는 다른 값
        for cut in 0..apk.len() {
            let _ = signer_cert_sha256(&apk[..cut]);
        }
        for i in 0..apk.len() {
            let mut bad = apk.clone();
            bad[i] ^= 0xFF;
            let _ = signer_cert_sha256(&bad);
        }
        assert!(signer_cert_sha256(b"PK\x03\x04 no signing block").is_err());
        let unsigned = crate::firmware::tests::build_zip(&[("a", b"b")]);
        assert!(signer_cert_sha256(&unsigned).is_err());
    }

    #[test]
    fn digest_mismatch_is_rejected_and_nothing_is_cached() {
        let json =
            serde_json::json!({"digest": format!("sha256:{}", sha256_hex(b"good")), "size": 4});
        let asset = asset_from_json(&json, "x.apk", "https://x", 1024).unwrap();
        assert!(verify_download(b"good", &asset).is_ok());
        assert!(verify_download(b"evil", &asset).is_err());
        assert!(verify_download(b"good!", &asset).is_err());
        // 다이제스트 없는 자산은 쓰지 않는다
        assert!(asset_from_json(&serde_json::json!({"size": 4}), "x", "u", 1024).is_err());
        assert!(asset_from_json(
            &serde_json::json!({"digest": "md5:abc", "size": 4}),
            "x",
            "u",
            1024
        )
        .is_err());
        assert!(asset_from_json(
            &serde_json::json!({"digest": json["digest"], "size": 4096}),
            "x",
            "u",
            1024
        )
        .is_err());
    }

    #[test]
    fn cache_requires_recorded_digest_matching_file_and_pin() {
        let dir = tempfile::tempdir().unwrap();
        let cert = b"official-cert";
        let pin = sha256_hex(cert);
        let apk = signed_apk(cert, false);
        let path = dir.path().join("app-v1.apk");
        let sha = sha256_hex(&apk);
        // 다이제스트 기록 없는 파일은 재사용하지 않는다
        std::fs::write(&path, &apk).unwrap();
        assert!(load_cached(&path, None, &[&pin], 1 << 20).is_none());
        store_verified(&path, &apk, &sha).unwrap();
        assert_eq!(load_cached(&path, None, &[&pin], 1 << 20).unwrap().1, sha);
        assert!(load_cached(&path, Some(&sha), &[&pin], 1 << 20).is_some());
        // 서버의 현재 다이제스트가 다르면(새 빌드로 교체) 캐시를 쓰지 않는다
        assert!(load_cached(&path, Some(&"a".repeat(64)), &[&pin], 1 << 20).is_none());
        // 핀 불일치·파일 변조
        assert!(load_cached(&path, None, &[&"b".repeat(64)], 1 << 20).is_none());
        let mut tampered = apk.clone();
        tampered[5] ^= 1;
        std::fs::write(&path, &tampered).unwrap();
        assert!(load_cached(&path, None, &[&pin], 1 << 20).is_none());
    }

    #[test]
    fn offline_fallback_picks_newest_verified_cache() {
        let dir = tempfile::tempdir().unwrap();
        let cert = b"official-cert";
        let pin = sha256_hex(cert);
        let good = signed_apk(cert, false);
        let older = dir.path().join("app-v1.apk");
        store_verified(&older, &good, &sha256_hex(&good)).unwrap();
        // 더 최근이지만 검증 실패(기록 없음) — 건너뛰고 검증된 파일을 고른다
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(dir.path().join("app-v2.apk"), &good).unwrap();
        std::fs::write(dir.path().join("other.bin"), b"x").unwrap();
        let (path, _, sha) = newest_verified_cache(
            dir.path(),
            |n| n.starts_with("app-") && n.ends_with(".apk"),
            &[&pin],
            1 << 20,
        )
        .unwrap();
        assert_eq!(path, older);
        assert_eq!(sha, sha256_hex(&good));
        assert!(newest_verified_cache(dir.path(), |_| true, &[&"c".repeat(64)], 1 << 20).is_none());
    }

    /// 실제 공식 APK로 핀 값 확인(네트워크 필요 — 수동: XV_APK_DIR=<폴더> cargo test -- --ignored live_apk_pins)
    #[test]
    #[ignore]
    fn live_apk_pins() {
        let dir = std::env::var("XV_APK_DIR").expect("XV_APK_DIR");
        for (file, pins) in [
            ("magisk.apk", MAGISK_CERT_SHA256),
            ("smsie.apk", SMSIE_CERT_SHA256),
        ] {
            let bytes = std::fs::read(Path::new(&dir).join(file)).unwrap();
            println!("{file}: {:?}", signer_cert_sha256(&bytes).unwrap());
            check_pins(&bytes, pins).unwrap();
        }
    }
}
