use crate::apk_verify::{self, ReleaseAsset};
use serde::{Deserialize, Serialize};
use std::{
    io::{Cursor, Read},
    path::Path,
};

const MAX: u64 = 256 * 1024 * 1024;
// Bootstrap: official rc3 asset digest 25657bc4...a0a0a1, 2026-10-08.
// APK v2 signer DER fingerprint, independently extracted with a Node length-prefix parser.
const RESUKISU_PINS: &[&str] =
    &["d3469712b6214462764a1d8d3e5cbe1d6819a0b629791b9f4101867821f1df64"];
const RESUKISU_REPO: &str = "Baka-SU/BakaSU";

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Prepared {
    pub id: String,
    pub version: String,
    pub path: String,
    pub sha256: String,
    pub module_id: Option<String>,
    pub external: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub tag: String,
    pub prerelease: bool,
    pub published_at: String,
}

pub const MODULES: &[(&str, &str)] = &[
    ("overlayfs", "RipperHybrid/Meta-Overlayfsx"),
    ("neozygisk", "JingMatrix/NeoZygisk"),
    ("rezygisk", "PerformanC/ReZygisk"),
    ("zygisk-next", "LSPosed/ZygiskNext"),
    ("zygisk-assistant", "snake-4/Zygisk-Assistant"),
    ("play-integrity-fork", "osm0sis/PlayIntegrityFork"),
    ("integrity-box", "MeowDump/Integrity-Box"),
    ("tricky-store", "5ec1cff/TrickyStore"),
    ("tricky-addon", "KOWX712/Tricky-Addon-Update-Target-List"),
    ("hma", "frknkrc44/HMA-OSS"),
    ("shamiko", "LSPosed/LSPosed.github.io"),
];
pub const EXTERNAL: &[&str] = &["bootloop-protector", "play-store-fix"];
fn safe_token(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 128
        && v.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        && v != "."
        && v != ".."
}
fn json(url: &str) -> Result<serde_json::Value, String> {
    let mut bytes = vec![];
    ureq::get(url)
        .set("User-Agent", "Xperia-VoLTE-Activator")
        .timeout(std::time::Duration::from_secs(30))
        .call()
        .map_err(|e| format!("릴리스 조회 실패: {e}"))?
        .into_reader()
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err("릴리스 목록 크기 초과".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "릴리스 응답 형식 오류".into())
}
#[tauri::command]
pub async fn resukisu_releases() -> Result<Vec<Release>, String> {
    crate::tasks::blocking("ReSukiSU versions", || {
        let mut out = vec![];
        for page in 1..=4 {
            let v = json(&format!(
                "https://api.github.com/repos/{RESUKISU_REPO}/releases?per_page=50&page={page}"
            ))?;
            let list = v.as_array().ok_or("릴리스 목록 형식 오류")?;
            for r in list.iter().filter(|r| r["draft"] == false) {
                let tag = r["tag_name"]
                    .as_str()
                    .filter(|t| safe_token(t))
                    .ok_or("릴리스 태그 형식 오류")?;
                if r["assets"].as_array().is_some_and(|a| {
                    a.iter()
                        .any(|a| a["name"].as_str().is_some_and(manager_name))
                }) {
                    out.push(Release {
                        tag: tag.into(),
                        prerelease: r["prerelease"].as_bool().ok_or("릴리스 상태 없음")?,
                        published_at: r["published_at"].as_str().unwrap_or_default().into(),
                    });
                }
            }
            if list.len() < 50 {
                return Ok(out);
            }
        }
        Err("릴리스 목록이 200개를 넘습니다 — 페이지 범위 재검토 필요".into())
    })
    .await
}
fn manager_name(name: &str) -> bool {
    safe_token(name) && name.starts_with("ReSukiSU_") && name.ends_with("-arm64-v8a-release.apk")
}
fn select(
    v: &serde_json::Value,
    repo: &str,
    manager: bool,
) -> Result<(String, ReleaseAsset), String> {
    let tag = v["tag_name"]
        .as_str()
        .filter(|t| safe_token(t))
        .ok_or("릴리스 태그 오류")?;
    if v["draft"] != false {
        return Err("초안 릴리스 사용 불가".into());
    }
    let assets = v["assets"].as_array().ok_or("릴리스 자산 없음")?;
    let matches: Vec<_> = assets
        .iter()
        .filter(|a| {
            a["name"].as_str().is_some_and(|n| {
                safe_token(n)
                    && if manager {
                        manager_name(n)
                    } else {
                        n.ends_with(".zip") && !n.to_ascii_lowercase().contains("debug")
                    }
            })
        })
        .collect();
    if matches.len() != 1 {
        return Err("릴리스 자산이 없거나 여러 개입니다 — 임의 선택하지 않습니다".into());
    }
    let mut a = matches[0].clone();
    let name = a["name"].as_str().unwrap().to_owned();
    let url = format!("https://github.com/{repo}/releases/download/{tag}/{name}");
    if a["browser_download_url"] != url {
        return Err("릴리스 다운로드 출처가 다릅니다".into());
    }
    // This older official asset predates GitHub's digest field. Pin only this exact release.
    // Bootstrap bytes fetched from this official HTTPS URL on 2026-10-08; no generic hash bypass.
    if repo == "snake-4/Zygisk-Assistant"
        && tag == "v2.1.4"
        && name == "Zygisk-Assistant-v2.1.4-1013f8a-release.zip"
        && a["digest"].is_null()
    {
        a["digest"] = serde_json::json!(
            "sha256:9eca30a269dc676a66f67a9339185dee55cccd47dc1fc8eeea5416124626d67a"
        );
    }
    Ok((
        tag.into(),
        apk_verify::asset_from_json(&a, &name, &url, MAX)?,
    ))
}
pub fn module_archive(bytes: &[u8]) -> Result<String, String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| "모듈 ZIP 형식 오류")?;
    if zip.len() > 8192 {
        return Err("모듈 ZIP 항목 수 초과".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut expanded = 0u64;
    let mut prop = None;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().trim_end_matches('/').to_owned();
        if !seen.insert(name.to_ascii_lowercase())
            || entry.enclosed_name().is_none()
            || name.contains('\\')
            || name.contains(':')
            || entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000)
        {
            return Err("모듈 ZIP의 경로·중복·링크가 안전하지 않습니다".into());
        }
        expanded = expanded.checked_add(entry.size()).ok_or("모듈 크기 초과")?;
        if expanded > 512 * 1024 * 1024 {
            return Err("모듈 압축 해제 크기 초과".into());
        }
        if name == "module.prop" {
            if entry.size() > 64 * 1024 {
                return Err("module.prop 크기 초과".into());
            }
            let mut text = String::new();
            entry
                .read_to_string(&mut text)
                .map_err(|_| "module.prop 읽기/CRC 오류")?;
            let ids: Vec<_> = text
                .lines()
                .filter_map(|l| l.trim().strip_prefix("id="))
                .collect();
            if ids.len() != 1 || !safe_token(ids[0]) {
                return Err("module.prop id 형식 오류".into());
            }
            prop = Some(ids[0].to_owned());
        } else {
            std::io::copy(&mut entry, &mut std::io::sink())
                .map_err(|_| "모듈 ZIP 읽기/CRC 오류")?;
        }
    }
    prop.ok_or_else(|| "ZIP 루트에 module.prop가 없습니다".into())
}
fn validate(bytes: &[u8], manager: bool) -> Result<Option<String>, String> {
    if manager {
        apk_verify::check_pins(bytes, RESUKISU_PINS)?;
        // Payload remains inside the verified APK. Never rely on sporadic ksud release assets.
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
        if zip
            .by_name("lib/arm64-v8a/libksud.so")
            .map_err(|_| "APK에 arm64 ksud가 없습니다")?
            .size()
            == 0
        {
            return Err("ksud가 비었습니다".into());
        }
        Ok(None)
    } else {
        module_archive(bytes).map(Some)
    }
}
fn persist(
    dir: &Path,
    id: &str,
    version: String,
    bytes: Vec<u8>,
    manager: bool,
    external: bool,
) -> Result<Prepared, String> {
    let module_id = validate(&bytes, manager)?;
    let sha256 = apk_verify::sha256_hex(&bytes);
    let path = dir
        .join("root-packages")
        .join(format!("{sha256}.{}", if manager { "apk" } else { "zip" }));
    crate::storage::atomic_write(&path, &bytes)?;
    let p = Prepared {
        id: id.into(),
        version,
        path: path.to_string_lossy().into(),
        sha256,
        module_id,
        external,
    };
    super::save(&path.with_extension("json"), &p)?;
    Ok(p)
}
#[tauri::command]
pub async fn root_package_prepare(id: String, tag: Option<String>) -> Result<Prepared, String> {
    crate::tasks::blocking("Root package preparation", move || {
        let dir = super::data_dir()?;
        if EXTERNAL.contains(&id.as_str()) {
            if tag.is_some() {
                return Err("동봉 모듈 버전은 배포 manifest로 고정됩니다".into());
            }
            let (bytes, hash) = bundled(&id)?;
            if apk_verify::sha256_hex(bytes) != hash {
                return Err("동봉 모듈 해시 불일치".into());
            }
            return persist(
                &dir,
                &id,
                "bundled-20261001".into(),
                bytes.to_vec(),
                false,
                true,
            );
        }
        let manager = id == "resukisu";
        let repo = if manager {
            RESUKISU_REPO
        } else {
            MODULES
                .iter()
                .find(|(k, _)| *k == id)
                .ok_or("지원하지 않는 패키지")?
                .1
        };
        let endpoint = if manager {
            let tag = tag
                .as_deref()
                .filter(|t| safe_token(t))
                .ok_or("ReSukiSU 버전을 명시적으로 선택하세요")?;
            format!("tags/{tag}")
        } else {
            if tag.is_some() {
                return Err("모듈은 최신 stable 릴리스를 조회합니다".into());
            }
            "latest".into()
        };
        let (version, asset) = select(
            &json(&format!(
                "https://api.github.com/repos/{repo}/releases/{endpoint}"
            ))?,
            repo,
            manager,
        )?;
        let mut bytes = Vec::new();
        ureq::get(&asset.url)
            .timeout(std::time::Duration::from_secs(300))
            .call()
            .map_err(|e| e.to_string())?
            .into_reader()
            .take(MAX + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        apk_verify::verify_download(&bytes, &asset)?;
        persist(&dir, &id, version, bytes, manager, false)
    })
    .await
}
fn bundled(id: &str) -> Result<(&'static [u8], &'static str), String> {
    match id {
        "bootloop-protector" => Ok((
            include_bytes!(
                "../../assets/root/AshReXcue_Bootloop_Protector_9.9_KO_SonyUserCommunity.zip"
            ),
            "8d6445c90ecfa237b8c7a09a06f8169bad9035a22858e99fc0741e877668f408",
        )),
        "play-store-fix" => Ok((
            include_bytes!(
                "../../assets/root/SonyUserCommunity_PlayStoreFix_v3.4_Magisk-KernelSU.zip"
            ),
            "3a1caa209914221350baac783d6da645eb60ec5e64f680fbc745e912b0ed6c34",
        )),
        _ => Err("동봉 패키지 없음".into()),
    }
}
/// 카페 HMA 프리셋을 폰 Download에 넣는다(2026-10-09 사용자 요청) — PC에 저장해 옮기던 수고를 없앤다.
/// 사용자는 HMA 앱에서 이 파일을 가져오기만 하면 된다. 같은 이름이 있으면 덮어쓴다(동봉 원본과 해시가 같은 파일).
#[tauri::command]
pub async fn root_preset_push(serial: String) -> Result<String, String> {
    super::write_gate(&serial, true)?;
    let operation = crate::device_io::WriteOperation::acquire()?;
    crate::tasks::blocking("HMA preset push", move || {
        let _operation = operation;
        let bytes: &[u8] = include_bytes!("../../assets/root/HMA-OSS_SonyUserCommunity_2026-10-01.json");
        if apk_verify::sha256_hex(bytes) != "99f730a53ba3474b0581bb28622844ece43dd4cdeb479c6ce13b53d6af897dfb" {
            return Err("HMA preset 해시 불일치".into());
        }
        const NAME: &str = "HMA-OSS_SonyUserCommunity_2026-10-01.json";
        let remote = format!("/storage/emulated/0/Download/{NAME}");
        crate::adb::with_first_device(&Some(serial), |dev| {
            // Download 폴더는 모듈 설치 뒤 앱 소유(drwxrws---)로 바뀌어 shell이 직접 못 쓴다(2026-10-09 실측).
            // shell이 쓸 수 있는 임시 폴더에 올린 뒤 루트로 복사한다 — 복사된 파일은 미디어 그룹이라 HMA 앱이 읽는다.
            let tmp = "/data/local/tmp/xvolte-hma.json";
            dev.push(&mut std::io::Cursor::new(bytes), &tmp)
                .map_err(|e| format!("HMA 프리셋 전송 실패: {e}"))?;
            let out = crate::device_io::shell_write(
                dev,
                &crate::device_io::su_command(&format!("cp {tmp} '{remote}' && rm -f {tmp} && test -f '{remote}'")),
            );
            let _ = crate::device_io::shell(dev, &format!("rm -f {tmp}"));
            out.map_err(|e| format!("HMA 프리셋을 Download로 복사하지 못했습니다: {e}"))?;
            Ok(remote)
        })
    })
    .await
}

#[tauri::command]
pub async fn root_preset_export(dest: String) -> Result<String, String> {
    crate::tasks::blocking("HMA preset export", move || {
        let dir = Path::new(&dest);
        if !dir.is_absolute() || !dir.is_dir() {
            return Err("저장할 절대 폴더 경로가 필요합니다".into());
        }
        let bytes = include_bytes!("../../assets/root/HMA-OSS_SonyUserCommunity_2026-10-01.json");
        if apk_verify::sha256_hex(bytes)
            != "99f730a53ba3474b0581bb28622844ece43dd4cdeb479c6ce13b53d6af897dfb"
        {
            return Err("HMA preset 해시 불일치".into());
        }
        let path = dir.join("HMA-OSS_SonyUserCommunity_2026-10-01.json");
        // User-selected directory; never overwrite an existing personal preset.
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| format!("프리셋 저장 실패: {e}"))?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        Ok(path.to_string_lossy().into())
    })
    .await
}
pub fn load(dir: &Path, hash: &str) -> Result<(Prepared, Vec<u8>), String> {
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("패키지 해시 오류".into());
    }
    let hash = hash.to_ascii_lowercase();
    let root = dir.join("root-packages");
    let raw = crate::storage::read_bounded(&root.join(format!("{hash}.json")), 64 * 1024)?
        .ok_or("준비한 패키지 기록 없음")?;
    let p: Prepared = serde_json::from_slice(&raw).map_err(|_| "패키지 기록 손상")?;
    let manager = p.id == "resukisu";
    if !manager && !MODULES.iter().any(|(id, _)| *id == p.id) && !EXTERNAL.contains(&p.id.as_str())
    {
        return Err("패키지 ID 오류".into());
    }
    let path = root.join(format!("{hash}.{}", if manager { "apk" } else { "zip" }));
    let bytes =
        crate::storage::read_bounded(&path, MAX as usize)?.ok_or("준비한 패키지 파일 없음")?;
    if p.sha256 != hash
        || apk_verify::sha256_hex(&bytes) != hash
        || validate(&bytes, manager)? != p.module_id
    {
        return Err("준비한 패키지 내용 변경".into());
    }
    Ok((p, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    pub fn zip_bytes(id: &str) -> Vec<u8> {
        use std::io::Write;
        let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
        z.start_file("module.prop", zip::write::SimpleFileOptions::default())
            .unwrap();
        write!(z, "id={id}\nname=Test\nversion=1\n").unwrap();
        z.finish().unwrap().into_inner()
    }
    #[test]
    fn archive_requires_unique_safe_module_id_and_cache_rechecks_content() {
        assert_eq!(
            module_archive(&zip_bytes("test.module")).unwrap(),
            "test.module"
        );
        assert!(module_archive(&zip_bytes("../evil")).is_err());
        assert!(module_archive(&zip_bytes("one\nid=two")).is_err());
        let dir = tempfile::tempdir().unwrap();
        let p = persist(
            dir.path(),
            "bootloop-protector",
            "user-supplied".into(),
            zip_bytes("overlayfs"),
            false,
            true,
        )
        .unwrap();
        assert!(load(dir.path(), &p.sha256).is_ok());
        std::fs::write(&p.path, zip_bytes("different")).unwrap();
        assert!(load(dir.path(), &p.sha256).is_err());
    }
    #[test]
    fn ambiguous_assets_missing_digest_redirects_and_unpinned_managers_fail() {
        let v = serde_json::json!({"tag_name":"v1","draft":false,"assets":[{"name":"test.zip","browser_download_url":"https://evil/test.zip","digest":format!("sha256:{}","a".repeat(64)),"size":1}]});
        assert!(select(&v, "owner/repo", false).is_err());
        assert!(validate(&zip_bytes("fake"), true).is_err());
        assert!(!safe_token("../v1"));
        assert!(!manager_name("ReSukiSU_debug.apk"));
    }
    #[test]
    fn overlayfs_uses_official_release_and_rejects_missing_digest_or_foreign_source() {
        let repo = MODULES.iter().find(|(id, _)| *id == "overlayfs").unwrap().1;
        assert!(!EXTERNAL.contains(&"overlayfs"));
        assert!(bundled("overlayfs").is_err());
        let hash = "043e01d944ab40327b64aeba1e8a88c8c616c36f1f6ba7c69216cf418a4832ea";
        let mut release = serde_json::json!({
            "tag_name": "v1.3.4", "draft": false,
            "assets": [{
                "name": "Meta-Overlayfsx_v1.3.4_13400.zip",
                "browser_download_url": "https://github.com/RipperHybrid/Meta-Overlayfsx/releases/download/v1.3.4/Meta-Overlayfsx_v1.3.4_13400.zip",
                "digest": format!("sha256:{hash}"), "size": 569836
            }]
        });
        let (tag, asset) = select(&release, repo, false).unwrap();
        assert_eq!(tag, "v1.3.4");
        assert_eq!(asset.sha256, hash);
        release["assets"][0]["digest"] = serde_json::Value::Null;
        assert!(select(&release, repo, false).is_err());
        release["assets"][0]["digest"] = serde_json::json!(format!("sha256:{hash}"));
        release["assets"][0]["browser_download_url"] =
            serde_json::json!("https://example.com/overlayfs.zip");
        assert!(select(&release, repo, false).is_err());
    }
    #[test]
    fn bundled_assets_match_pins_and_are_valid_modules() {
        for id in EXTERNAL {
            let (bytes, hash) = bundled(id).unwrap();
            assert_eq!(apk_verify::sha256_hex(bytes), hash);
            assert!(module_archive(bytes).is_ok());
        }
        let bytes = include_bytes!("../../assets/root/HMA-OSS_SonyUserCommunity_2026-10-01.json");
        assert_eq!(
            apk_verify::sha256_hex(bytes),
            "99f730a53ba3474b0581bb28622844ece43dd4cdeb479c6ce13b53d6af897dfb"
        );
        assert!(serde_json::from_slice::<serde_json::Value>(bytes).is_ok());
    }
}
