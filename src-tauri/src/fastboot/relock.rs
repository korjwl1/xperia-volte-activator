//! 리락 게이트 (§3-3) — 부트 체인 파티션×슬롯의 플래시 이력이 순정 이미지로 끝나는지 판정.
//! 이력원: flash-history.jsonl(fastboot_flash가 deviceKey·상태(started/done/failed)까지 기록).
//! 순수 로직만 담는다(파일·해시 IO는 호출부 주입 — 단위 테스트 용이).

use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub device_key: String,
    pub partition: String, // 파티션+슬롯 (예: init_boot_a)
    pub sha256: String,
    pub status: String, // started | done | failed
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotCheck {
    pub partition: String,
    pub slot: String, // "_a" | "_b"
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GateResult {
    pub ok: bool,
    pub reasons: Vec<String>,
    pub checked: Vec<SlotCheck>,
}

/// 리락 게이트 허용 부트 체인 파티션(기저명 — 슬롯 제외)
const BOOT_CHAIN: &[&str] = &["boot", "init_boot"];

/// flash-history.jsonl 파싱 — 빈 줄·끊긴 줄·필수 필드 없는 줄은 무시(손상 이력은 판정에서 버린다)
pub fn parse_history(raw: &str) -> Vec<HistoryEntry> {
    let mut out = Vec::new();
    for line in raw.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        let (Some(device_key), Some(partition), Some(sha256), Some(status)) = (
            v["deviceKey"].as_str(),
            v["partition"].as_str(),
            v["sha256"].as_str(),
            v["status"].as_str(),
        ) else {
            continue;
        };
        out.push(HistoryEntry {
            device_key: device_key.to_string(),
            partition: partition.to_string(),
            sha256: sha256.to_string(),
            status: status.to_string(),
        });
    }
    out
}

/// 순정 증거 — 파일을 읽어 ANDROID! 매직 확인 후 해시. 패치 이미지(ANDROID!이지만 내용 다름)는
/// 해시 비교로 구별된다(호출부에서 stockPath는 firmware_fetch 결과만 전달).
pub fn stock_sha256(path: &Path) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("순정 이미지 읽기 실패: {e}"))?;
    if !data.starts_with(b"ANDROID!") {
        return Err("순정 이미지가 ANDROID! 부트 이미지 형식이 아닙니다".into());
    }
    Ok(hex::encode(Sha256::digest(&data)))
}

/// 판정 — {partition}×_a/_b의 마지막 done 이력이 stock_sha와 일치해야 통과.
/// 우리 이력에 없는 파티션·슬롯 = 이 도구가 건 적 없음 → 순정 유지로 간주(통과).
/// device_key 필터: None이면 전체 이력 대상(사전 점검 안내용), Some이면 해당 기기만.
pub fn verify(
    history: &[HistoryEntry],
    partition: &str,
    stock_sha: &str,
    device_key: Option<&str>,
) -> GateResult {
    let mut reasons = Vec::new();
    let mut checked = Vec::new();
    if !BOOT_CHAIN.contains(&partition) {
        return GateResult {
            ok: false,
            reasons: vec![format!("부트 체인 파티션(boot·init_boot)이 아닙니다: {partition}")],
            checked,
        };
    }
    for slot in ["_a", "_b"] {
        let part = format!("{partition}{slot}");
        let entries: Vec<&HistoryEntry> = history
            .iter()
            .filter(|h| {
                h.partition == part
                    && device_key.map(|k| h.device_key == k).unwrap_or(true)
            })
            .collect();
        if entries.is_empty() {
            checked.push(SlotCheck {
                partition: partition.to_string(),
                slot: slot.to_string(),
                ok: true,
                detail: "기록 없음 — 이 도구가 수정한 적 없음(순정 유지로 간주)".into(),
            });
            continue;
        }
        // 마지막 done 항목 — 그 이후 started/failed가 더 있으면 최근 시도가 실패한 것
        let last_done = entries.iter().rev().find(|h| h.status == "done");
        let after_last_done_problematic = match last_done {
            Some(done) => {
                let done_ptr = *done as *const HistoryEntry;
                let idx = entries
                    .iter()
                    .position(|h| std::ptr::eq(*h, done_ptr))
                    .expect("rev find 결과는 목록에 있음");
                entries[idx + 1..]
                    .iter()
                    .any(|h| h.status == "started" || h.status == "failed")
            }
            None => false,
        };
        let Some(done) = last_done else {
            reasons.push(format!("{part}: 완료된 기록이 없습니다 — 언루팅(순정 기록)을 먼저 진행해 주세요"));
            checked.push(SlotCheck {
                partition: partition.to_string(),
                slot: slot.to_string(),
                ok: false,
                detail: "완료(done) 기록 없음".into(),
            });
            continue;
        };
        if after_last_done_problematic {
            reasons.push(format!("{part}: 완료 이후 실패한 기록이 있습니다 — 마지막 상태가 불명확합니다"));
            checked.push(SlotCheck {
                partition: partition.to_string(),
                slot: slot.to_string(),
                ok: false,
                detail: "완료 후 실패 기록 존재".into(),
            });
            continue;
        }
        if done.sha256 == stock_sha {
            checked.push(SlotCheck {
                partition: partition.to_string(),
                slot: slot.to_string(),
                ok: true,
                detail: "마지막 기록이 순정 이미지와 일치".into(),
            });
        } else {
            reasons.push(format!(
                "{part}: 마지막 기록이 순정 이미지와 다릅니다(언루팅 필요) — sha256 {}… ≠ {}…",
                &done.sha256[..8.min(done.sha256.len())],
                &stock_sha[..8.min(stock_sha.len())],
            ));
            checked.push(SlotCheck {
                partition: partition.to_string(),
                slot: slot.to_string(),
                ok: false,
                detail: "마지막 기록이 순정과 불일치".into(),
            });
        }
    }
    let ok = reasons.is_empty();
    GateResult { ok, reasons, checked }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(partition: &str, sha: &str, status: &str) -> HistoryEntry {
        HistoryEntry {
            device_key: "dev1".into(),
            partition: partition.into(),
            sha256: sha.into(),
            status: status.into(),
        }
    }

    const STOCK: &str = "aa1111111111111111111111111111111111111111111111111111111111111aa";
    const PATCHED: &str = "bb2222222222222222222222222222222222222222222222222222222222222bb";

    #[test]
    fn no_history_passes_as_untouched() {
        let g = verify(&[], "init_boot", STOCK, Some("dev1"));
        assert!(g.ok);
        assert!(g.checked.iter().all(|c| c.ok));
    }

    #[test]
    fn both_slots_stock_done_passes() {
        let h = vec![
            entry("init_boot_a", PATCHED, "done"),
            entry("init_boot_b", PATCHED, "done"),
            entry("init_boot_a", STOCK, "done"),
            entry("init_boot_b", STOCK, "done"),
        ];
        assert!(verify(&h, "init_boot", STOCK, Some("dev1")).ok);
    }

    #[test]
    fn last_flash_patched_blocks() {
        // 루팅만 한 상태 — 마지막이 패치 이미지
        let h = vec![
            entry("init_boot_a", STOCK, "done"),
            entry("init_boot_b", STOCK, "done"),
            entry("init_boot_a", PATCHED, "done"),
            entry("init_boot_b", PATCHED, "done"),
        ];
        let g = verify(&h, "init_boot", STOCK, Some("dev1"));
        assert!(!g.ok);
        assert_eq!(g.reasons.len(), 2);
    }

    #[test]
    fn one_slot_only_done_blocks() {
        let h = vec![
            entry("init_boot_a", STOCK, "done"),
            entry("init_boot_b", STOCK, "failed"),
        ];
        let g = verify(&h, "init_boot", STOCK, Some("dev1"));
        assert!(!g.ok);
        assert!(g.reasons.iter().any(|r| r.contains("init_boot_b")));
    }

    #[test]
    fn done_then_failed_after_blocks() {
        // 언루팅 성공 뒤 무언가 실패한 시도 — 최종 상태 불명확
        let h = vec![
            entry("init_boot_a", STOCK, "done"),
            entry("init_boot_a", STOCK, "started"),
            entry("init_boot_a", STOCK, "failed"),
        ];
        let g = verify(&h, "init_boot", STOCK, Some("dev1"));
        assert!(!g.ok);
        assert!(g.reasons.iter().any(|r| r.contains("불명확")));
    }

    #[test]
    fn other_device_history_ignored() {
        let mut other = entry("init_boot_a", PATCHED, "done");
        other.device_key = "dev2".into();
        let h = vec![other, entry("init_boot_b", STOCK, "done")];
        let g = verify(&h, "init_boot", STOCK, Some("dev1"));
        assert!(g.ok); // dev2 이력은 무시, dev1은 _b만 순정 done — _a 기록 없음(미조작 간주)
    }

    #[test]
    fn non_boot_chain_partition_rejected() {
        assert!(!verify(&[], "userdata", STOCK, None).ok);
    }

    #[test]
    fn parsing_skips_broken_lines() {
        let raw = format!(
            "{{\"deviceKey\":\"d\",\"partition\":\"boot_a\",\"sha256\":\"x\",\"status\":\"done\"}}\n\n{{broken\n\"\"\n{{\"deviceKey\":\"d\",\"partition\":\"boot_b\",\"sha256\":\"y\",\"status\":\"done\"}}\n"
        );
        let h = parse_history(&raw);
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].partition, "boot_a");
    }

    #[test]
    fn stock_image_magic_enforced() {
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("stock.img");
        std::fs::write(&bad, b"NOTANDROID").unwrap();
        assert!(stock_sha256(&bad).is_err());
        let good = dir.path().join("stock2.img");
        std::fs::write(&good, b"ANDROID!payload").unwrap();
        let sha = stock_sha256(&good).unwrap();
        assert_eq!(sha.len(), 64);
    }

    /// 루팅→언루팅 왕복 시나리오 — 이력 순서 보존 판정
    #[test]
    fn root_then_unroot_scenario() {
        let mut h = vec![];
        for slot in ["_a", "_b"] {
            h.push(entry(&format!("init_boot{slot}"), STOCK, "started"));
            h.push(entry(&format!("init_boot{slot}"), PATCHED, "done")); // 루팅
        }
        assert!(!verify(&h, "init_boot", STOCK, Some("dev1")).ok);
        for slot in ["_a", "_b"] {
            h.push(entry(&format!("init_boot{slot}"), STOCK, "done")); // 언루팅
        }
        assert!(verify(&h, "init_boot", STOCK, Some("dev1")).ok);
    }
}
