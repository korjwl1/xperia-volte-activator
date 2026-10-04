//! 순정 펌웨어에서 부트 이미지(init_boot / boot)만 부분 다운로드 — Sony 배포 서버(app.swup.update.sony.net)
//!
//! 흐름: 기기 정보(모델·빌드·지문) → 지원 기종 표에서 식별값 → match/v2(서버 최신 버전 = 기기 버전인지)
//!       → software-service(APP_SW 파일) → 청크 목록 → ZIP 끝부분만 Range로 받아 목록(central directory) 해석
//!       → update.xml 지문이 기기와 완전히 같은지 확인 → `<partition>_*.sin`만 Range로 받아 inflate·CRC 확인
//!       → tar에서 `.000`(ANDROID! 이미지) 추출 → 앱 데이터 폴더에 `<partition>.img` 저장
//! 전체 펌웨어(XQ-DQ44 약 4.8 GB) 대신 init_boot 약 1.7 MB만 받는다. 펌웨어를 재배포하지 않는다(사용자 PC에만 저장).
//! 조사 근거: tasks/research-unlock-firmware.md (비공식 API — 변경 시 실패하며, 프론트는 수동 폴더 선택으로 폴백)
//! PC 쓰기: 앱 데이터 폴더의 firmware/ 캐시만 (사용자 승인 2026-10-03)

use crate::adb::{parse_getprop, shell, with_first_device};
use crate::app_paths::data_dir as app_data_dir;
use crate::tasks::guarded;
use serde::Serialize;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

const API: &str = "https://app.swup.update.sony.net/ess-distribution/public/api";

/// 지원 기종 식별값 — 기종별 식별값 조회 API가 2025-11 폐쇄되어 직접 관리 (출처: Sony product/v2 + XperiCheck)
struct DeviceIds {
    model: &'static str,
    cdf_id: &'static str,
    product_code: &'static str,
    product_id: &'static str,
    model_id: &'static str,
    hw_variant_id: &'static str,
}

const DEVICES: &[DeviceIds] = &[DeviceIds {
    model: "XQ-DQ44", // Xperia 1 V (일본 SIM-free, XQ-DQ44_Customized_JP)
    cdf_id: "25853834",
    product_code: "43041034",
    product_id: "d2e6c91c-1b3d-419f-bcb6-02bb299059c3",
    model_id: "d6f8b033-8742-4e07-87d4-8c99c0f4239c",
    hw_variant_id: "a7be8ce7-629d-4e09-bc8f-675431394e09",
}];

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FirmwareOut {
    partition: String,
    /// 저장된 이미지 전체 경로
    path: String,
    version: String,
    /// 펌웨어 update.xml 지문 (= 기기 ro.build.fingerprint)
    fingerprint: String,
    /// 이미지 크기
    image_bytes: u64,
    /// 실제로 받은 바이트 (목록 + update.xml + .sin)
    downloaded_bytes: u64,
}

// ── HTTP ──

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(60))
        .build()
}

fn get_text(a: &ureq::Agent, url: &str) -> Result<String, String> {
    a.get(url)
        .call()
        .map_err(|e| format!("Sony 서버 요청 실패: {e}"))?
        .into_string()
        .map_err(|e| format!("Sony 서버 응답 읽기 실패: {e}"))
}

/// 여러 청크로 나뉜 ZIP을 하나의 파일처럼 Range로 읽는다
struct ChunkedFile {
    agent: ureq::Agent,
    chunks: Vec<(u64, String)>, // (크기, URL)
    downloaded: u64,
}

/// 범위 읽기 추상 — HTTP 조각(펌웨어)과 로컬 파일(APK 등)에서 같은 ZIP 코드를 쓴다.
pub(crate) trait RangeRead {
    fn total(&self) -> Result<u64, String>;
    fn read_at(&mut self, offset: u64, len: u64) -> Result<Vec<u8>, String>;
}

impl RangeRead for ChunkedFile {
    fn total(&self) -> Result<u64, String> {
        self.total_impl()
    }
    fn read_at(&mut self, offset: u64, len: u64) -> Result<Vec<u8>, String> {
        self.read_at_impl(offset, len)
    }
}

impl ChunkedFile {
    fn total_impl(&self) -> Result<u64, String> {
        self.chunks.iter().try_fold(0u64, |sum, (size, _)| {
            sum.checked_add(*size)
                .ok_or("펌웨어 크기 합계가 올바르지 않습니다".into())
        })
    }

    fn read_at_impl(&mut self, mut offset: u64, mut len: u64) -> Result<Vec<u8>, String> {
        if len > MAX_ENTRY
            || offset
                .checked_add(len)
                .is_none_or(|end| end > self.total().unwrap_or(0))
        {
            return Err("펌웨어 읽기 범위/크기가 올바르지 않습니다".into());
        }
        let mut out = Vec::with_capacity(len as usize);
        let mut base = 0u64;
        for (size, url) in &self.chunks {
            if len == 0 {
                break;
            }
            if offset < base + size {
                let start = offset - base;
                let take = len.min(size - start);
                let resp = self
                    .agent
                    .get(url)
                    .set("Range", &format!("bytes={}-{}", start, start + take - 1))
                    .call()
                    .map_err(|e| format!("펌웨어 조각 요청 실패: {e}"))?;
                let expected = format!("bytes {start}-{}/{}", start + take - 1, size);
                if resp.status() != 206 || resp.header("Content-Range") != Some(expected.as_str()) {
                    return Err("Sony 서버가 요청한 파일 범위를 반환하지 않았습니다".into());
                }
                let mut buf = Vec::with_capacity(take as usize);
                resp.into_reader()
                    .take(take + 1)
                    .read_to_end(&mut buf)
                    .map_err(|e| format!("펌웨어 조각 읽기 실패: {e}"))?;
                if buf.len() as u64 != take {
                    return Err("펌웨어 조각 크기가 맞지 않습니다".into());
                }
                self.downloaded += take;
                out.extend_from_slice(&buf);
                offset += take;
                len -= take;
            }
            base += size;
        }
        if len != 0 {
            return Err("펌웨어 파일 범위를 벗어났습니다".into());
        }
        Ok(out)
    }
}

// ── XML ──

fn child_text<'a>(n: roxmltree::Node<'a, 'a>, tag: &str) -> Option<&'a str> {
    n.children()
        .find(|c| c.has_tag_name(tag))
        .and_then(|c| c.text())
}

fn link_href<'a>(n: roxmltree::Node<'a, 'a>, rel: &str) -> Option<&'a str> {
    n.children()
        .find(|c| c.has_tag_name("link") && c.attribute("rel") == Some(rel))
        .and_then(|c| c.attribute("href"))
}

fn match_url(ids: &DeviceIds) -> String {
    format!(
        "{API}/device-service/match/v2?CDFId={}&HwSetupKey=Default&HWVariantId={}&ModelObjectId={}&ProductObjectId={}&SecurityStateType=COMMERCIAL&SonyProductCode={}",
        ids.cdf_id, ids.hw_variant_id, ids.model_id, ids.product_id, ids.product_code
    )
}

/// Sony 펌웨어 버전 비교 ("67.2.A.3.178") — 점으로 나눠 숫자는 숫자로, 문자는 문자로 비교
pub(crate) fn cmp_version(a: &str, b: &str) -> std::cmp::Ordering {
    let pa: Vec<&str> = a.split('.').collect();
    let pb: Vec<&str> = b.split('.').collect();
    for i in 0..pa.len().max(pb.len()) {
        let (x, y) = (
            pa.get(i).copied().unwrap_or(""),
            pb.get(i).copied().unwrap_or(""),
        );
        let o = match (x.parse::<u64>(), y.parse::<u64>()) {
            (Ok(m), Ok(n)) => m.cmp(&n),
            _ => x.cmp(y),
        };
        if o != std::cmp::Ordering::Equal {
            return o;
        }
    }
    std::cmp::Ordering::Equal
}

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FwVersionOut {
    version: String,
    android: String,
}

/// match/v2 응답의 버전 목록 (중복 제거, 최신순)
fn parse_versions(xml: &str) -> Result<Vec<FwVersionOut>, String> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| format!("Sony 응답 해석 실패: {e}"))?;
    let mut out: Vec<FwVersionOut> = vec![];
    for n in doc
        .descendants()
        .filter(|n| n.has_tag_name("software-device-service-info"))
    {
        let v = child_text(n, "software-version").unwrap_or("").to_string();
        if !v.is_empty() && !out.iter().any(|o| o.version == v) {
            out.push(FwVersionOut {
                version: v,
                android: child_text(n, "android-version").unwrap_or("").to_string(),
            });
        }
    }
    out.sort_by(|a, b| cmp_version(&b.version, &a.version));
    Ok(out)
}

/// match/v2 응답에서 기기 버전과 같은 TARGET 소프트웨어의 링크
fn pick_software(xml: &str, version: &str) -> Result<String, String> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| format!("Sony 응답 해석 실패: {e}"))?;
    let infos: Vec<_> = doc
        .descendants()
        .filter(|n| n.has_tag_name("software-device-service-info"))
        .collect();
    let mut server_versions = vec![];
    for n in &infos {
        let v = child_text(*n, "software-version").unwrap_or("");
        server_versions.push(v.to_string());
        if v == version {
            if let Some(h) = link_href(*n, "self") {
                return Ok(h.to_string());
            }
        }
    }
    server_versions.sort();
    server_versions.dedup();
    Err(format!(
        "서버에 이 기기 버전({version})의 펌웨어가 없습니다 — 서버 제공 버전: {}. 휴대폰을 최신 버전으로 업데이트한 뒤 다시 시도하거나 펌웨어 폴더를 직접 지정해 주세요",
        server_versions.join(", ")
    ))
}

/// software-service 응답에서 APP_SW 파일의 청크 정보 링크
fn pick_app_sw(xml: &str) -> Result<String, String> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| format!("Sony 응답 해석 실패: {e}"))?;
    doc.descendants()
        .filter(|n| n.has_tag_name("file-resource"))
        .find(|n| child_text(*n, "file-key") == Some("APP_SW"))
        .and_then(|n| link_href(n, "file-chunk-info"))
        .map(|s| s.to_string())
        .ok_or("펌웨어 파일 정보를 찾을 수 없습니다".into())
}

fn parse_chunks(xml: &str) -> Result<Vec<(u64, String)>, String> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| format!("Sony 응답 해석 실패: {e}"))?;
    // 조각 하나라도 해석하지 못하면 뒤쪽 오프셋이 모두 밀리므로 건너뛰지 않고 실패한다
    let mut chunks: Vec<(u64, u64, String)> = doc
        .descendants()
        .filter(|n| n.has_tag_name("file-chunk"))
        .map(|n| {
            let num = n.attribute("number")?.parse().ok()?;
            let size = child_text(n, "size")?.trim().parse().ok()?;
            Some((num, size, link_href(n, "download")?.to_string()))
        })
        .collect::<Option<_>>()
        .ok_or("펌웨어 조각 정보가 올바르지 않습니다")?;
    chunks.sort_by_key(|c| c.0);
    let Some(first) = chunks.first().map(|c| c.0) else {
        return Err("펌웨어 조각 목록이 비어 있습니다".into());
    };
    // 번호는 빠짐·중복 없이 연속이어야 한다
    if chunks
        .iter()
        .enumerate()
        .any(|(i, c)| first.checked_add(i as u64) != Some(c.0))
    {
        return Err("펌웨어 조각 번호가 연속되지 않습니다".into());
    }
    Ok(chunks.into_iter().map(|(_, s, u)| (s, u)).collect())
}

// ── ZIP (zip64) ──

#[derive(Debug, Clone)]
struct ZipEntry {
    name: String,
    method: u16,
    crc32: u32,
    comp_size: u64,
    uncomp_size: u64,
    local_offset: u64,
}

// 범위 밖 읽기는 panic 대신 오류 (손상된 응답·파일 방어 — release는 panic=abort라 앱 전체가 종료됨)
const ZIP_BAD: &str = "펌웨어 ZIP 구조가 올바르지 않습니다";
fn u16le(b: &[u8], i: usize) -> Result<u16, String> {
    b.get(i..i.checked_add(2).ok_or(ZIP_BAD)?)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
        .ok_or_else(|| ZIP_BAD.into())
}
fn u32le(b: &[u8], i: usize) -> Result<u32, String> {
    b.get(i..i.checked_add(4).ok_or(ZIP_BAD)?)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(|| ZIP_BAD.into())
}
fn u64le(b: &[u8], i: usize) -> Result<u64, String> {
    b.get(i..i.checked_add(8).ok_or(ZIP_BAD)?)
        .map(|s| u64::from_le_bytes(s.try_into().expect("8바이트")))
        .ok_or_else(|| ZIP_BAD.into())
}
/// 한 항목으로 받을 최대 크기 (update.xml·부트 이미지 .sin — 정상은 수십 MB 이하)
const MAX_ENTRY: u64 = 256 * 1024 * 1024;

/// 파일 끝부분(tail)에서 central directory 위치를 찾는다 → (cd_offset, cd_size)
fn find_central_directory(
    tail: &[u8],
    tail_start: u64,
    f: &mut dyn RangeRead,
) -> Result<(u64, u64), String> {
    let eocd = (0..tail.len().saturating_sub(21))
        .rev()
        .find(|&i| u32le(tail, i).ok() == Some(0x0605_4b50))
        .ok_or("펌웨어 ZIP 끝 정보를 찾을 수 없습니다")?;
    let cd_size = u32le(tail, eocd + 12)? as u64;
    let cd_offset = u32le(tail, eocd + 16)? as u64;
    if cd_offset != 0xFFFF_FFFF && cd_size != 0xFFFF_FFFF {
        return Ok((cd_offset, cd_size));
    }
    // zip64: EOCD 바로 앞 locator(20바이트) → zip64 EOCD
    if eocd < 20 || u32le(tail, eocd - 20)? != 0x0706_4b50 {
        return Err("펌웨어 ZIP64 정보를 찾을 수 없습니다".into());
    }
    let z64_off = u64le(tail, eocd - 20 + 8)?;
    let rec = if z64_off >= tail_start {
        tail.get((z64_off - tail_start) as usize..)
            .ok_or(ZIP_BAD)?
            .to_vec()
    } else {
        f.read_at(z64_off, 56)?
    };
    if u32le(&rec, 0)? != 0x0606_4b50 {
        return Err("펌웨어 ZIP64 레코드가 올바르지 않습니다".into());
    }
    Ok((u64le(&rec, 48)?, u64le(&rec, 40)?))
}

fn parse_central_directory(cd: &[u8]) -> Result<Vec<ZipEntry>, String> {
    let mut out = vec![];
    let mut i = 0;
    while i + 46 <= cd.len() && u32le(cd, i)? == 0x0201_4b50 {
        let method = u16le(cd, i + 10)?;
        let crc32 = u32le(cd, i + 16)?;
        let mut comp_size = u32le(cd, i + 20)? as u64;
        let mut uncomp_size = u32le(cd, i + 24)? as u64;
        let name_len = u16le(cd, i + 28)? as usize;
        let extra_len = u16le(cd, i + 30)? as usize;
        let comment_len = u16le(cd, i + 32)? as usize;
        let mut local_offset = u32le(cd, i + 42)? as u64;
        let name =
            String::from_utf8_lossy(cd.get(i + 46..i + 46 + name_len).ok_or(ZIP_BAD)?).to_string();
        // zip64 확장 필드(0x0001): 0xFFFFFFFF인 값만 순서대로 들어 있다
        let mut e = i + 46 + name_len;
        let extra_end = e + extra_len;
        if extra_end > cd.len() {
            return Err(ZIP_BAD.into());
        }
        while e + 4 <= extra_end {
            let id = u16le(cd, e)?;
            let size = u16le(cd, e + 2)? as usize;
            if e + 4 + size > extra_end {
                return Err(ZIP_BAD.into());
            }
            if id == 0x0001 {
                let field = &cd[e + 4..e + 4 + size];
                let mut p = 0;
                if uncomp_size == 0xFFFF_FFFF {
                    uncomp_size = u64le(field, p)?;
                    p += 8;
                }
                if comp_size == 0xFFFF_FFFF {
                    comp_size = u64le(field, p)?;
                    p += 8;
                }
                if local_offset == 0xFFFF_FFFF {
                    local_offset = u64le(field, p)?;
                }
            }
            e += 4 + size;
        }
        if extra_end + comment_len > cd.len() {
            return Err(ZIP_BAD.into());
        }
        out.push(ZipEntry {
            name,
            method,
            crc32,
            comp_size,
            uncomp_size,
            local_offset,
        });
        i = extra_end + comment_len;
    }
    if out.is_empty() {
        return Err("펌웨어 ZIP 목록이 비어 있습니다".into());
    }
    Ok(out)
}

/// 항목 하나만 받아 압축 해제 + CRC 확인
fn read_entry(f: &mut dyn RangeRead, e: &ZipEntry) -> Result<Vec<u8>, String> {
    let header = f.read_at(e.local_offset, 30)?;
    if e.comp_size > MAX_ENTRY || e.uncomp_size > MAX_ENTRY {
        return Err(format!("{} 항목 크기가 비정상적입니다", e.name));
    }
    if u32le(&header, 0)? != 0x0403_4b50 {
        return Err(format!("{} 항목 헤더가 올바르지 않습니다", e.name));
    }
    let data_off = e
        .local_offset
        .checked_add(30 + u16le(&header, 26)? as u64 + u16le(&header, 28)? as u64)
        .ok_or(ZIP_BAD)?;
    let raw = f.read_at(data_off, e.comp_size)?;
    let data = match e.method {
        0 => raw,
        8 => {
            let mut out = Vec::with_capacity(e.uncomp_size as usize);
            flate2::read::DeflateDecoder::new(&raw[..])
                .take(e.uncomp_size + 1)
                .read_to_end(&mut out)
                .map_err(|err| format!("{} 압축 해제 실패: {err}", e.name))?;
            out
        }
        m => return Err(format!("{} 지원하지 않는 압축 방식({m})", e.name)),
    };
    if data.len() as u64 != e.uncomp_size || crc32fast::hash(&data) != e.crc32 {
        return Err(format!("{} 무결성 검사(CRC) 실패", e.name));
    }
    Ok(data)
}

/// 메모리에 이미 읽어 검증한 ZIP(APK) — 해시를 잰 바로 그 바이트를 파싱해 바꿔치기 틈이 없다
pub(crate) struct MemZip<'a>(pub(crate) &'a [u8]);

impl RangeRead for MemZip<'_> {
    fn total(&self) -> Result<u64, String> {
        Ok(self.0.len() as u64)
    }

    fn read_at(&mut self, offset: u64, len: u64) -> Result<Vec<u8>, String> {
        let end = offset
            .checked_add(len)
            .filter(|end| len <= MAX_ENTRY && *end <= self.0.len() as u64)
            .ok_or("ZIP 읽기 범위/크기가 올바르지 않습니다")?;
        Ok(self.0[offset as usize..end as usize].to_vec())
    }
}

/// ZIP 끝부분(최대 128 KiB) → central directory 목록. 원격(Sony 서버)·로컬(APK) 공용.
fn read_central_directory(f: &mut dyn RangeRead) -> Result<Vec<ZipEntry>, String> {
    let total = f.total()?;
    let tail_len = total.min(128 * 1024);
    let tail_start = total - tail_len;
    let tail = f.read_at(tail_start, tail_len)?;
    let (cd_off, cd_size) = find_central_directory(&tail, tail_start, f)?;
    cd_off
        .checked_add(cd_size)
        .filter(|end| *end <= total)
        .ok_or(ZIP_BAD)?; // 범위 검증 — 값 자체는 아래 슬라이스에 사용
    if cd_off >= tail_start {
        let start = (cd_off - tail_start) as usize;
        parse_central_directory(&tail[start..start + cd_size as usize])
    } else {
        parse_central_directory(&f.read_at(cd_off, cd_size)?)
    }
}

/// 조건에 맞는 항목이 정확히 하나여야 한다 — 여러 개면 어느 것을 쓸지 추측하지 않는다
fn unique_entry<'a>(
    entries: &'a [ZipEntry],
    what: &str,
    pred: impl Fn(&ZipEntry) -> bool,
) -> Result<&'a ZipEntry, String> {
    let mut matching = entries.iter().filter(|e| pred(e));
    match (matching.next(), matching.next()) {
        (Some(entry), None) => Ok(entry),
        (None, _) => Err(format!("펌웨어에 {what}이(가) 없습니다")),
        (Some(_), Some(_)) => Err(format!(
            "펌웨어에 {what}이(가) 여러 개 있습니다 — 어느 것을 쓸지 정할 수 없어 중단합니다. 펌웨어 폴더를 직접 지정해 주세요"
        )),
    }
}

/// ZIP에서 지정한 이름의 항목들을 읽는다(전부 필수) — tail → central directory → 항목
pub(crate) fn zip_extract_named(
    f: &mut dyn RangeRead,
    names: &[&str],
) -> Result<Vec<(String, Vec<u8>)>, String> {
    let entries = read_central_directory(f)?;
    let mut out = Vec::with_capacity(names.len());
    let mut aggregate = 0u64;
    for name in names {
        let matching: Vec<_> = entries.iter().filter(|e| e.name == *name).collect();
        if matching.len() != 1 {
            return Err(format!("ZIP의 {name} 항목이 없거나 중복됐습니다"));
        }
        aggregate = aggregate
            .checked_add(matching[0].uncomp_size)
            .filter(|total| *total <= MAX_ENTRY)
            .ok_or("APK 페이로드 합계가 크기 상한을 초과했습니다")?;
    }
    for name in names {
        let e = entries
            .iter()
            .find(|e| e.name == *name)
            .ok_or_else(|| format!("ZIP에 {name} 항목이 없습니다"))?;
        out.push((e.name.clone(), read_entry(f, e)?));
    }
    Ok(out)
}

/// update.xml의 지문(<FINGERPRINT>)
fn update_xml_fingerprint(xml: &str) -> Option<String> {
    let doc = roxmltree::Document::parse(xml).ok()?;
    doc.descendants()
        .find(|n| n.tag_name().name().eq_ignore_ascii_case("FINGERPRINT"))
        .and_then(|n| n.text())
        .map(|s| s.trim().to_string())
}

/// `.sin`(tar: 서명 .cms + 원본 이미지 .000)에서 이미지 추출 — ANDROID! 매직 확인
fn extract_sin_image(sin: &[u8]) -> Result<Vec<u8>, String> {
    let mut ar = tar::Archive::new(sin);
    for entry in ar.entries().map_err(|e| format!(".sin 해석 실패: {e}"))? {
        let mut entry = entry.map_err(|e| format!(".sin 해석 실패: {e}"))?;
        let path = entry
            .path()
            .map_err(|e| format!(".sin 해석 실패: {e}"))?
            .to_string_lossy()
            .to_string();
        if path.ends_with(".000") {
            let mut img = vec![];
            if entry.size() > MAX_ENTRY {
                return Err("부트 이미지 크기가 상한을 넘습니다".into());
            }
            (&mut entry)
                .take(MAX_ENTRY + 1)
                .read_to_end(&mut img)
                .map_err(|e| format!(".sin 해석 실패: {e}"))?;
            if img.len() as u64 > MAX_ENTRY {
                return Err("부트 이미지 크기가 상한을 넘습니다".into());
            }
            if !img.starts_with(b"ANDROID!") {
                return Err("부트 이미지 형식(ANDROID!)이 아닙니다".into());
            }
            return Ok(img);
        }
    }
    Err(".sin 안에 이미지(.000)가 없습니다".into())
}

fn sin_name_matches(name: &str, partition: &str) -> bool {
    let base = name.rsplit('/').next().unwrap_or(name);
    base.starts_with(&format!("{partition}_")) && base.ends_with(".sin")
}

// ── 명령 ──

/// 저장 공간 부족 오류 접두어 — 프론트가 "다른 저장 위치 선택"으로 안내
pub(crate) const NO_SPACE: &str = "NO_SPACE|";

/// 저장 폴더: 지정한 폴더(사용자 선택) 또는 앱 데이터 폴더
fn firmware_dir(dest: &Option<String>, model: &str, version: &str) -> Result<PathBuf, String> {
    let base = match dest.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
        Some(d) => PathBuf::from(d),
        None => app_data_dir()
            .ok_or("앱 데이터 폴더를 확인할 수 없습니다")?
            .join("firmware"),
    };
    Ok(base.join(format!("{model}_{version}")))
}

/// 저장 전 여유 공간 확인 — 이미지 크기 + 여유분(16 MiB)
fn ensure_space(dir: &Path, need: u64) -> Result<(), String> {
    let need = need.saturating_add(16 * 1024 * 1024);
    let free = fs2::available_space(dir).map_err(|e| format!("여유 공간 조회 실패: {e}"))?;
    if free < need {
        return Err(format!(
            "{NO_SPACE}저장 공간이 부족합니다 — 필요 {:.0} MB, 남은 공간 {:.0} MB ({})",
            need as f64 / 1048576.0,
            free as f64 / 1048576.0,
            dir.display()
        ));
    }
    Ok(())
}

fn fetch_work(
    serial: Option<String>,
    partition: String,
    target: Option<String>,
    dest: Option<String>,
) -> Result<FirmwareOut, String> {
    if partition != "init_boot" && partition != "boot" {
        return Err("지원하지 않는 파티션입니다".into());
    }
    let raw = with_first_device(&serial, |dev| shell(dev, "getprop"))?;
    let p = parse_getprop(&raw);
    let get = |k: &str| p.get(k).cloned().unwrap_or_default();
    let (model, installed, fingerprint) = (
        get("ro.product.model"),
        get("ro.build.id"),
        get("ro.build.fingerprint"),
    );
    // 대상 버전: 지정이 없으면 설치된 버전. 업데이트 대상은 설치된 버전보다 새 버전만 (다운그레이드 불가)
    let version = target
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| installed.clone());
    let is_update = version != installed;
    if is_update && cmp_version(&version, &installed) != std::cmp::Ordering::Greater {
        return Err(format!(
            "설치된 버전({installed})보다 새 버전만 받을 수 있습니다"
        ));
    }

    let ids = DEVICES
        .iter()
        .find(|d| d.model == model)
        .ok_or_else(|| format!("자동 다운로드를 아직 지원하지 않는 기종입니다({model}) — 펌웨어 폴더를 직접 지정해 주세요"))?;

    let agent = agent();
    let sw_url = pick_software(&get_text(&agent, &match_url(ids))?, &version)?;
    let chunk_url = pick_app_sw(&get_text(&agent, &sw_url)?)?;
    let chunks = parse_chunks(&get_text(&agent, &chunk_url)?)?;
    let mut f = ChunkedFile {
        agent,
        chunks,
        downloaded: 0,
    };

    // ZIP 목록: 끝 128 KiB → central directory
    let entries = read_central_directory(&mut f)?;

    // 부트 이미지 .sin — 정확히 하나여야 한다(폴더 지정 경로와 같은 규칙)
    let sin = unique_entry(&entries, &format!("{partition} 이미지"), |e| {
        sin_name_matches(&e.name, &partition)
    })?
    .clone();
    let sin_dir = sin
        .name
        .rsplit_once('/')
        .map_or("", |(dir, _)| dir)
        .to_string();
    // 지문 대조용 update.xml — .sin과 같은 폴더의 것을 쓴다(다른 폴더의 것과 섞지 않는다)
    let ux = unique_entry(&entries, "update.xml", |e| {
        e.name
            .rsplit_once('/')
            .map_or(("", e.name.as_str()), |(d, n)| (d, n))
            == (sin_dir.as_str(), "update.xml")
    })?
    .clone();

    // 지문 확인 — 설치된 버전이면 기기와 완전히 일치, 업데이트 대상이면 같은 기기·지역(지문 앞부분) + 대상 버전
    let ux_text = String::from_utf8_lossy(&read_entry(&mut f, &ux)?).to_string();
    let fw_fp = update_xml_fingerprint(&ux_text).ok_or("펌웨어 지문을 읽을 수 없습니다")?;
    let same_device = fw_fp.split(':').next() == fingerprint.split(':').next();
    let ok = if is_update {
        same_device && fw_fp.contains(&format!("/{version}/"))
    } else {
        fw_fp == fingerprint
    };
    if !ok {
        return Err("펌웨어 지문이 기기와 맞지 않습니다 — 다른 지역/버전 펌웨어이므로 사용할 수 없습니다. 펌웨어 폴더를 직접 지정해 주세요".into());
    }

    // 받기 전에 크기 상한·저장 위치·공간부터 확인 (이미지 크기 ≈ .sin 크기)
    if sin.uncomp_size > MAX_ENTRY {
        return Err("부트 이미지 크기가 상한을 넘습니다".into());
    }
    let dir = firmware_dir(&dest, &model, &version)?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("펌웨어 저장 폴더 생성 실패: {e}"))?;
    ensure_space(&dir, sin.uncomp_size)?;

    let img = extract_sin_image(&read_entry(&mut f, &sin)?)?;
    // 폴더 지정 경로와 같은 이미지 검사(최소 크기 포함)
    crate::boot_image::validate(&img)?;
    let path: PathBuf = dir.join(format!("{partition}.img"));
    crate::storage::atomic_write(&path, &img).map_err(|e| format!("부트 이미지 저장 실패: {e}"))?;
    eprintln!(
        "[rust] firmware {partition} {version}: {} bytes 저장 (다운로드 {} bytes)",
        img.len(),
        f.downloaded
    );

    Ok(FirmwareOut {
        partition,
        path: path.to_string_lossy().to_string(),
        version,
        fingerprint: fw_fp,
        image_bytes: img.len() as u64,
        downloaded_bytes: f.downloaded,
    })
}

/// 순정 펌웨어(설치된 버전 또는 지정한 새 버전)에서 부트 이미지만 받아 저장 (기본: 앱 데이터 폴더, dest 지정 시 그 폴더)
#[tauri::command]
pub async fn firmware_fetch(
    serial: Option<String>,
    partition: String,
    version: Option<String>,
    dest: Option<String>,
) -> Result<FirmwareOut, String> {
    guarded(Duration::from_secs(300), move || {
        fetch_work(serial, partition, version, dest)
    })
    .await
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FirmwareVersionsOut {
    model: String,
    installed: String,
    /// 서버 버전 조회를 지원하는 기종인지
    supported: bool,
    /// 설치된 버전 이상만 (최신순) — 다운그레이드는 제공하지 않음
    versions: Vec<FwVersionOut>,
}

fn versions_work(serial: Option<String>) -> Result<FirmwareVersionsOut, String> {
    let raw = with_first_device(&serial, |dev| {
        shell(dev, "getprop ro.product.model; getprop ro.build.id")
    })?;
    let mut lines = raw.lines().map(|l| l.trim().to_string());
    let model = lines.next().unwrap_or_default();
    let installed = lines.next().unwrap_or_default();
    let Some(ids) = DEVICES.iter().find(|d| d.model == model) else {
        return Ok(FirmwareVersionsOut {
            model,
            installed,
            supported: false,
            versions: vec![],
        });
    };
    let versions = parse_versions(&get_text(&agent(), &match_url(ids))?)?
        .into_iter()
        .filter(|v| cmp_version(&v.version, &installed) != std::cmp::Ordering::Less)
        .collect();
    Ok(FirmwareVersionsOut {
        model,
        installed,
        supported: true,
        versions,
    })
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FirmwareDirOut {
    /// 찾은 .sin 파일 이름
    file: String,
    path: String,
    fingerprint: String,
    /// 추출한 이미지 크기
    image_bytes: u64,
}

/// 수동 SIN과 update.xml을 검사하고 패치·기록에 사용할 raw IMG를 PC 캐시에 추출한다.
fn dir_check_work(dir: &str, partition: &str, cache: &Path) -> Result<FirmwareDirOut, String> {
    if partition != "init_boot" && partition != "boot" {
        return Err("지원하지 않는 파티션입니다".into());
    }
    let root = Path::new(dir);
    if !root.is_dir() {
        return Err("폴더를 찾을 수 없습니다".into());
    }
    // XperiFirm은 폴더 안에 바로 풀어 두지만, 한 단계 아래 폴더도 확인
    let mut candidates = vec![];
    for entry in std::fs::read_dir(root).map_err(|e| format!("폴더 읽기 실패: {e}"))? {
        let entry = entry.map_err(|e| format!("폴더 항목 읽기 실패: {e}"))?;
        let p = entry.path();
        if p.is_dir() {
            let sub = std::fs::read_dir(&p).map_err(|e| e.to_string())?;
            for entry in sub {
                candidates.push(entry.map_err(|e| e.to_string())?.path());
            }
        } else {
            candidates.push(p);
        }
    }
    let matching: Vec<_> = candidates
        .into_iter()
        .filter(|p| p.is_file())
        .filter(|p| {
            sin_name_matches(
                &p.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
                partition,
            )
        })
        .collect();
    if matching.len() != 1 {
        return Err(format!(
            "{partition} SIN 파일이 없거나 여러 개입니다 — 펌웨어 폴더 하나를 지정해 주세요"
        ));
    }
    let sin = &matching[0];
    let xml = crate::storage::read_bounded(
        &sin.parent().ok_or("SIN 부모 경로 없음")?.join("update.xml"),
        1024 * 1024,
    )?
    .ok_or("펌웨어 폴더에 update.xml이 없습니다 — 기기와 버전을 대조할 수 없습니다")?;
    let xml = String::from_utf8(xml).map_err(|e| e.to_string())?;
    let fingerprint = update_xml_fingerprint(&xml)
        .filter(|fp| !fp.is_empty())
        .ok_or("펌웨어 지문이 없습니다")?;
    let data =
        crate::storage::read_bounded(sin, MAX_ENTRY as usize)?.ok_or("SIN 파일이 없습니다")?;
    let img = extract_sin_image(&data)?;
    crate::boot_image::validate(&img)?;
    let path = cache.join(format!(
        "{partition}-{}.img",
        crate::boot_image::sha256(&img)
    ));
    crate::storage::atomic_write(&path, &img)?;
    Ok(FirmwareDirOut {
        file: sin
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        image_bytes: img.len() as u64,
        path: path.to_string_lossy().to_string(),
        fingerprint,
    })
}

/// 직접 지정한 펌웨어 폴더 검사·PC 캐시 추출(기기 쓰기 없음).
#[tauri::command]
pub async fn firmware_dir_check(dir: String, partition: String) -> Result<FirmwareDirOut, String> {
    let cache = crate::app_paths::data_dir()
        .ok_or("앱 데이터 폴더가 없습니다")?
        .join("firmware/manual");
    crate::tasks::blocking("수동 펌웨어 추출", move || {
        dir_check_work(&dir, &partition, &cache)
    })
    .await
}

/// 서버에 있는 펌웨어 버전 (읽기 전용 조회)
#[tauri::command]
pub async fn firmware_versions(serial: Option<String>) -> Result<FirmwareVersionsOut, String> {
    guarded(Duration::from_secs(30), move || versions_work(serial)).await
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// 테스트용 stored 방식 ZIP 빌더 — 로컬 헤더 + 데이터 + central directory + EOCD
    pub(crate) fn build_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        fn le16(v: u16) -> [u8; 2] {
            v.to_le_bytes()
        }
        fn le32(v: u32) -> [u8; 4] {
            v.to_le_bytes()
        }
        let mut out = Vec::new();
        let mut central = Vec::new();
        let mut offsets = Vec::new();
        for (name, data) in entries {
            offsets.push(out.len() as u32);
            let crc = crc32fast::hash(data);
            out.extend_from_slice(b"PK\x03\x04");
            out.extend_from_slice(&le16(20)); // version needed
            out.extend_from_slice(&le16(0)); // flags
            out.extend_from_slice(&le16(0)); // method: stored
            out.extend_from_slice(&le16(0)); // time
            out.extend_from_slice(&le16(0)); // date
            out.extend_from_slice(&le32(crc));
            out.extend_from_slice(&le32(data.len() as u32));
            out.extend_from_slice(&le32(data.len() as u32));
            out.extend_from_slice(&le16(name.len() as u16));
            out.extend_from_slice(&le16(0)); // extra len
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(data);
        }
        let cd_offset = out.len() as u32;
        for ((name, data), off) in entries.iter().zip(&offsets) {
            let crc = crc32fast::hash(data);
            central.extend_from_slice(b"PK\x01\x02");
            central.extend_from_slice(&le16(20)); // version made by
            central.extend_from_slice(&le16(20)); // version needed
            central.extend_from_slice(&le16(0)); // flags
            central.extend_from_slice(&le16(0)); // method
            central.extend_from_slice(&le16(0)); // time
            central.extend_from_slice(&le16(0)); // date
            central.extend_from_slice(&le32(crc));
            central.extend_from_slice(&le32(data.len() as u32));
            central.extend_from_slice(&le32(data.len() as u32));
            central.extend_from_slice(&le16(name.len() as u16));
            central.extend_from_slice(&le16(0)); // extra
            central.extend_from_slice(&le16(0)); // comment
            central.extend_from_slice(&le16(0)); // disk start
            central.extend_from_slice(&le16(0)); // internal attrs
            central.extend_from_slice(&le32(0)); // external attrs
            central.extend_from_slice(&le32(*off));
            central.extend_from_slice(name.as_bytes());
        }
        out.extend_from_slice(&central);
        let cd_size = central.len() as u32;
        out.extend_from_slice(b"PK\x05\x06");
        out.extend_from_slice(&le16(0)); // disk
        out.extend_from_slice(&le16(0)); // cd disk
        out.extend_from_slice(&le16(entries.len() as u16));
        out.extend_from_slice(&le16(entries.len() as u16));
        out.extend_from_slice(&le32(cd_size));
        out.extend_from_slice(&le32(cd_offset));
        out.extend_from_slice(&le16(0)); // comment len
        out
    }

    #[test]
    fn duplicate_and_excessive_zip_payloads_fail_before_extraction() {
        let duplicate = build_zip(&[("a", b"one"), ("a", b"two")]);
        assert!(zip_extract_named(&mut MemZip(&duplicate), &["a"]).is_err());
        let mut bytes = build_zip(&[("a", b"one"), ("b", b"two")]);
        for offset in 0..bytes.len() - 4 {
            if bytes[offset..offset + 4] == *b"PK\x01\x02" {
                bytes[offset + 24..offset + 28]
                    .copy_from_slice(&(150u32 * 1024 * 1024).to_le_bytes());
            }
        }
        let error = zip_extract_named(&mut MemZip(&bytes), &["a", "b"]).unwrap_err();
        assert!(error.contains("APK"));
    }

    #[test]
    fn manual_sin_returns_a_raw_image_and_requires_unambiguous_firmware_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let firmware = dir.path().join("download");
        std::fs::create_dir_all(&firmware).unwrap();
        let cache = dir.path().join("cache");
        let mut image = vec![0x41; 4096];
        image[..8].copy_from_slice(b"ANDROID!");
        let mut header = tar::Header::new_gnu();
        header.set_size(image.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        let mut tar = tar::Builder::new(Vec::new());
        tar.append_data(&mut header, "init_boot.000", &image[..])
            .unwrap();
        let sin = firmware.join("init_boot_X-FLASH-ALL-test.sin");
        std::fs::write(&sin, tar.into_inner().unwrap()).unwrap();
        assert!(dir_check_work(dir.path().to_str().unwrap(), "init_boot", &cache).is_err());
        std::fs::write(
            firmware.join("update.xml"),
            "<UPDATE><FINGERPRINT>Sony/current</FINGERPRINT></UPDATE>",
        )
        .unwrap();
        let result = dir_check_work(dir.path().to_str().unwrap(), "init_boot", &cache).unwrap();
        assert!(result.path.ends_with(".img"));
        assert_ne!(std::path::Path::new(&result.path), sin);
        assert_eq!(result.fingerprint, "Sony/current");
        assert_eq!(std::fs::read(&result.path).unwrap(), image);
        std::fs::copy(&sin, firmware.join("init_boot_X-FLASH-ALL-other.sin")).unwrap();
        assert!(dir_check_work(dir.path().to_str().unwrap(), "init_boot", &cache).is_err());
    }

    #[test]
    fn memory_zip_extract_named_roundtrip() {
        let apk = build_zip(&[
            ("lib/arm64-v8a/libbusybox.so", b"busybox-bytes"),
            ("assets/boot_patch.sh", b"#!/script"),
            ("unrelated.txt", b"skip me"),
        ]);
        let mut z = MemZip(&apk);
        let got = zip_extract_named(
            &mut z,
            &["lib/arm64-v8a/libbusybox.so", "assets/boot_patch.sh"],
        )
        .unwrap();
        assert_eq!(got[0].1, b"busybox-bytes");
        assert_eq!(got[1].1, b"#!/script");
        // 누락 항목은 오류
        let mut z2 = MemZip(&apk);
        assert!(zip_extract_named(&mut z2, &["lib/arm64-v8a/libmagisk.so"]).is_err());
    }

    #[test]
    fn range_responses_require_exact_status_header_and_body() {
        use std::io::{Read, Write};
        for (status, range, body, valid) in [
            ("206 Partial Content", "bytes 1-2/4", "bc", true),
            ("200 OK", "bytes 1-2/4", "bc", false),
            ("206 Partial Content", "bytes 0-1/4", "bc", false),
            ("206 Partial Content", "bytes 1-2/4", "b", false),
            ("206 Partial Content", "bytes 1-2/4", "bcd", false),
        ] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}/chunk", listener.local_addr().unwrap());
            let thread = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut request = vec![];
                let mut buffer = [0; 512];
                while !request.ends_with(b"\r\n\r\n") {
                    let n = stream.read(&mut buffer).unwrap();
                    assert!(n > 0 && request.len() < 8192);
                    request.extend_from_slice(&buffer[..n]);
                }
                assert!(String::from_utf8(request)
                    .unwrap()
                    .to_lowercase()
                    .contains("range: bytes=1-2"));
                write!(stream, "HTTP/1.1 {status}\r\nContent-Range: {range}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            });
            let mut file = ChunkedFile {
                agent: agent(),
                chunks: vec![(4, url)],
                downloaded: 0,
            };
            let result = file.read_at(1, 2);
            assert_eq!(result.is_ok(), valid, "{status} / {range} / {body}");
            if valid {
                assert_eq!(result.unwrap(), b"bc");
            }
            thread.join().unwrap();
        }
    }

    #[test]
    fn excessive_and_overflowing_ranges_fail_before_network_io() {
        let mut file = ChunkedFile {
            agent: agent(),
            chunks: vec![(4, "invalid".into())],
            downloaded: 0,
        };
        for (offset, length) in [(0, MAX_ENTRY + 1), (u64::MAX, 2), (3, 2)] {
            assert!(file.read_at(offset, length).is_err());
        }
        file.chunks = vec![(u64::MAX, "invalid".into()), (1, "invalid".into())];
        assert!(file.total().is_err());
    }

    #[test]
    fn corrupt_central_directory_is_error_not_panic() {
        // 46바이트 헤더에 name_len=1, 실제 이름 바이트 없음
        let mut cd = vec![0u8; 46];
        cd[0..4].copy_from_slice(&0x0201_4b50u32.to_le_bytes());
        cd[28] = 1;
        assert!(parse_central_directory(&cd).is_err());
        // extra 길이가 버퍼를 넘음
        let mut cd2 = vec![0u8; 47];
        cd2[0..4].copy_from_slice(&0x0201_4b50u32.to_le_bytes());
        cd2[28] = 1;
        cd2[30] = 200;
        assert!(parse_central_directory(&cd2).is_err());
    }

    #[test]
    fn dir_check_rejects_bad_folders() {
        let tmp = std::env::temp_dir().join("xvolte_dircheck_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        assert!(
            dir_check_work(tmp.to_str().unwrap(), "init_boot", &tmp.join("cache"))
                .unwrap_err()
                .contains("SIN 파일이 없거나 여러 개")
        );
        std::fs::write(tmp.join("init_boot_X-FLASH-ALL-TEST.sin"), b"not a tar").unwrap();
        assert!(dir_check_work(tmp.to_str().unwrap(), "init_boot", &tmp.join("cache")).is_err());
        assert!(dir_check_work("Z:\\없는 폴더", "init_boot", &tmp.join("cache")).is_err());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn space_check_and_dest_dir() {
        let tmp = std::env::temp_dir();
        assert!(ensure_space(&tmp, 1024).is_ok());
        let e = ensure_space(&tmp, u64::MAX / 2).unwrap_err();
        assert!(e.starts_with(NO_SPACE));
        let d = firmware_dir(
            &Some(tmp.to_string_lossy().into()),
            "XQ-DQ44",
            "67.2.A.3.178",
        )
        .unwrap();
        assert!(d.starts_with(&tmp) && d.ends_with("XQ-DQ44_67.2.A.3.178"));
    }

    #[test]
    fn version_compare_and_parse() {
        use std::cmp::Ordering::*;
        assert_eq!(cmp_version("67.2.A.3.178", "67.1.A.2.315"), Greater);
        assert_eq!(cmp_version("67.2.A.3.178", "67.2.A.3.178"), Equal);
        assert_eq!(cmp_version("67.2.A.3.99", "67.2.A.3.178"), Less);
        let xml = "<r><software-device-service-info><software-version>67.1.A.2.315</software-version><android-version>14</android-version></software-device-service-info><software-device-service-info><software-version>67.2.A.3.178</software-version><android-version>15</android-version></software-device-service-info><software-device-service-info><software-version>67.2.A.3.178</software-version><android-version>15</android-version></software-device-service-info></r>";
        let v = parse_versions(xml).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(
            v[0],
            FwVersionOut {
                version: "67.2.A.3.178".into(),
                android: "15".into()
            }
        );
    }

    #[test]
    fn sin_name_matching() {
        assert!(sin_name_matches(
            "init_boot_X-FLASH-ALL-25B1.sin",
            "init_boot"
        ));
        assert!(sin_name_matches("boot/boot_X-FLASH-ALL-25B1.sin", "boot"));
        assert!(!sin_name_matches("init_boot_X-FLASH-ALL-25B1.sin", "boot"));
        assert!(!sin_name_matches(
            "vendor_boot_X-FLASH-ALL-25B1.sin",
            "boot"
        ));
    }

    #[test]
    fn pick_software_matches_version() {
        let xml = r#"<match-response><device-service-infos>
<software-device-service-info><link rel="self" href="https://x/a"/><software-version>67.2.A.3.178</software-version></software-device-service-info>
<software-device-service-info><link rel="self" href="https://x/b"/><software-version>67.1.A.2.315</software-version></software-device-service-info>
</device-service-infos></match-response>"#;
        assert_eq!(pick_software(xml, "67.1.A.2.315").unwrap(), "https://x/b");
        assert!(pick_software(xml, "60.0.A.0.1")
            .unwrap_err()
            .contains("67.2.A.3.178"));
    }

    #[test]
    fn chunk_list_must_be_complete_and_contiguous() {
        let chunk = |n: &str, size: &str| {
            format!(
                r#"<file-chunk number="{n}"><size>{size}</size><link rel="download" href="https://x/{n}"/></file-chunk>"#
            )
        };
        let doc = |body: String| format!("<file-chunk-info>{body}</file-chunk-info>");
        let ok = parse_chunks(&doc(chunk("2", "20") + &chunk("1", "10"))).unwrap();
        assert_eq!(
            ok,
            vec![(10, "https://x/1".into()), (20, "https://x/2".into())]
        );
        // 빠진 번호·중복·해석 불가 조각은 건너뛰지 않고 실패
        assert!(parse_chunks(&doc(chunk("1", "10") + &chunk("3", "30"))).is_err());
        assert!(parse_chunks(&doc(chunk("1", "10") + &chunk("1", "10"))).is_err());
        assert!(parse_chunks(&doc(chunk("1", "10") + &chunk("2", "x"))).is_err());
        assert!(parse_chunks(&doc(String::new())).is_err());
    }

    /// 실제 Sony 서버에서 XQ-DQ44 init_boot를 부분 다운로드 (네트워크 필요 — 수동 실행: cargo test -- --ignored live_)
    #[test]
    #[ignore]
    fn live_partial_download_xq_dq44() {
        let ids = &DEVICES[0];
        let agent = agent();
        let sw =
            pick_software(&get_text(&agent, &match_url(ids)).unwrap(), "67.2.A.3.178").unwrap();
        let chunks = parse_chunks(
            &get_text(
                &agent,
                &pick_app_sw(&get_text(&agent, &sw).unwrap()).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        let mut f = ChunkedFile {
            agent,
            chunks,
            downloaded: 0,
        };
        let total = f.total().unwrap();
        let tail_start = total - 128 * 1024;
        let tail = f.read_at(tail_start, 128 * 1024).unwrap();
        let (off, size) = find_central_directory(&tail, tail_start, &mut f).unwrap();
        let cd = f.read_at(off, size).unwrap();
        let entries = parse_central_directory(&cd).unwrap();
        let ux = entries
            .iter()
            .find(|e| e.name.ends_with("update.xml"))
            .unwrap()
            .clone();
        let fp =
            update_xml_fingerprint(&String::from_utf8_lossy(&read_entry(&mut f, &ux).unwrap()))
                .unwrap();
        assert!(
            fp.starts_with("Sony/XQ-DQ44/XQ-DQ44:15/67.2.A.3.178/"),
            "{fp}"
        );
        let sin = entries
            .iter()
            .find(|e| sin_name_matches(&e.name, "init_boot"))
            .unwrap()
            .clone();
        let img = extract_sin_image(&read_entry(&mut f, &sin).unwrap()).unwrap();
        assert_eq!(img.len(), 8 * 1024 * 1024);
        eprintln!(
            "entries={} downloaded={} bytes img={} fp={fp}",
            entries.len(),
            f.downloaded,
            img.len()
        );
        // 수동 검증용: XVOLTE_SAVE_IMG=<경로> 지정 시 이미지 저장
        if let Ok(p) = std::env::var("XVOLTE_SAVE_IMG") {
            std::fs::write(&p, &img).unwrap();
        }
    }
}
