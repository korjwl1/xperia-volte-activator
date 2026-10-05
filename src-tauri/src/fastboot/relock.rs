//! 리락 게이트 (§3-3) — 부트 체인 파티션×슬롯의 플래시 이력이 순정 이미지로 끝나는지 판정.
//! 이력원: flash-history.jsonl(fastboot_flash가 deviceKey·상태(started/done/failed)까지 기록).
//! 순수 로직만 담는다(파일·해시 IO는 호출부 주입 — 단위 테스트 용이).

use serde::Serialize;

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
const BOOT_CHAIN: &[&str] = &crate::boot_image::BOOT_PARTITIONS;

fn valid_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

/// 손상/미완료 줄을 버리면 이전 성공 이력이 최신 상태로 오인된다. 전체를 실패 처리한다.
pub fn parse_history(raw: &str) -> Result<Vec<HistoryEntry>, String> {
    let mut out = Vec::new();
    for (index, line) in raw.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let v = serde_json::from_str::<serde_json::Value>(line)
            .map_err(|_| format!("플래시 이력 {}번째 줄이 손상됐습니다", index + 1))?;
        let (Some(device_key), Some(partition), Some(sha256), Some(status)) = (
            v["deviceKey"].as_str(),
            v["partition"].as_str(),
            v["sha256"].as_str(),
            v["status"].as_str(),
        ) else {
            return Err(format!(
                "플래시 이력 {}번째 줄의 필수 값이 없습니다",
                index + 1
            ));
        };
        if !valid_hash(device_key)
            || !valid_hash(sha256)
            || !matches!(status, "started" | "done" | "failed")
            || partition.is_empty()
            || !partition
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        {
            return Err(format!(
                "플래시 이력 {}번째 줄의 값이 올바르지 않습니다",
                index + 1
            ));
        }
        out.push(HistoryEntry {
            device_key: device_key.to_string(),
            partition: partition.to_string(),
            sha256: sha256.to_string(),
            status: status.to_string(),
        });
    }
    Ok(out)
}

/// 해당 기기 양 슬롯의 최신 항목이 done이고 입력 이미지와 같은지 진단한다.
/// 호출부는 추출 출처와 현재 기기·모드를 별도로 검사한다. 전체 AVB 검증은 아니다.
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
            reasons: vec![format!(
                "부트 체인 파티션(boot·init_boot)이 아닙니다: {partition}"
            )],
            checked,
        };
    }
    let Some(device_key) = device_key.filter(|key| valid_hash(key)) else {
        return GateResult {
            ok: false,
            reasons: vec!["현재 기기의 식별 키가 필요합니다".into()],
            checked,
        };
    };
    if !valid_hash(stock_sha) {
        return GateResult {
            ok: false,
            reasons: vec!["이미지 해시가 올바르지 않습니다".into()],
            checked,
        };
    }
    // 다른 부트 파티션까지 건드렸다면 이 이미지 하나로 전체 부트 체인을 검증할 수 없다.
    let slots = [format!("{partition}_a"), format!("{partition}_b")];
    if history
        .iter()
        .any(|h| h.device_key == device_key && !slots.contains(&h.partition))
    {
        reasons
            .push("다른 파티션의 플래시 이력이 있습니다 — 전체 부트 체인 검증이 필요합니다".into());
    }
    let slot_check = |part: &str, ok: bool, detail: &str| SlotCheck {
        partition: partition.to_string(),
        slot: part[partition.len()..].to_string(),
        ok,
        detail: detail.into(),
    };
    for part in &slots {
        let last = history
            .iter()
            .rfind(|h| &h.partition == part && h.device_key == device_key);
        // 마지막 항목이 done이어야 한다 — 그 이후 started/failed가 있으면 최근 시도가 실패한 것
        match last {
            None => {
                reasons.push(format!(
                    "{part}: 플래시 기록이 없어 슬롯 상태를 확인할 수 없습니다"
                ));
                checked.push(slot_check(
                    part,
                    false,
                    "기록 없음 — 슬롯 상태를 확인할 수 없음",
                ));
            }
            Some(entry) if entry.status != "done" => {
                reasons.push(format!(
                    "{part}: 최신 시도가 완료되지 않아 최종 상태가 불명확합니다"
                ));
                checked.push(slot_check(part, false, "완료(done) 기록 없음"));
            }
            Some(done) if done.sha256 == stock_sha => {
                checked.push(slot_check(part, true, "마지막 기록이 순정 이미지와 일치"));
            }
            Some(done) => {
                reasons.push(format!(
                    "{part}: 마지막 기록이 순정 이미지와 다릅니다(언루팅 필요) — sha256 {}… ≠ {}…",
                    done.sha256.chars().take(8).collect::<String>(),
                    stock_sha.chars().take(8).collect::<String>(),
                ));
                checked.push(slot_check(part, false, "마지막 기록이 순정과 불일치"));
            }
        }
    }
    let ok = reasons.is_empty();
    GateResult {
        ok,
        reasons,
        checked,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(partition: &str, sha: &str, status: &str) -> HistoryEntry {
        HistoryEntry {
            device_key: KEY.into(),
            partition: partition.into(),
            sha256: sha.into(),
            status: status.into(),
        }
    }

    const KEY: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    const STOCK: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const PATCHED: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[test]
    fn missing_history_blocks() {
        let g = verify(&[], "init_boot", STOCK, Some(KEY));
        assert!(!g.ok);
        assert!(g.checked.iter().all(|c| !c.ok));
    }

    #[test]
    fn both_slots_stock_done_passes() {
        let h = vec![
            entry("init_boot_a", PATCHED, "done"),
            entry("init_boot_b", PATCHED, "done"),
            entry("init_boot_a", STOCK, "done"),
            entry("init_boot_b", STOCK, "done"),
        ];
        assert!(verify(&h, "init_boot", STOCK, Some(KEY)).ok);
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
        let g = verify(&h, "init_boot", STOCK, Some(KEY));
        assert!(!g.ok);
        assert_eq!(g.reasons.len(), 2);
    }

    #[test]
    fn one_slot_only_done_blocks() {
        let h = vec![
            entry("init_boot_a", STOCK, "done"),
            entry("init_boot_b", STOCK, "failed"),
        ];
        let g = verify(&h, "init_boot", STOCK, Some(KEY));
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
        let g = verify(&h, "init_boot", STOCK, Some(KEY));
        assert!(!g.ok);
        assert!(g.reasons.iter().any(|r| r.contains("불명확")));
    }

    #[test]
    fn other_device_history_ignored() {
        let mut other = entry("init_boot_a", PATCHED, "done");
        other.device_key = "d".repeat(64);
        let h = vec![other, entry("init_boot_b", STOCK, "done")];
        let g = verify(&h, "init_boot", STOCK, Some(KEY));
        assert!(!g.ok);
        assert!(!g.checked[0].ok);
        assert!(g.checked[1].ok);
    }

    #[test]
    fn non_boot_chain_partition_rejected() {
        assert!(!verify(&[], "userdata", STOCK, None).ok);
    }

    #[test]
    fn parsing_rejects_corrupt_and_incomplete_lines() {
        let good = format!(
            r#"{{"deviceKey":"{KEY}","partition":"boot_a","sha256":"{STOCK}","status":"done"}}"#
        );
        assert_eq!(parse_history(&format!("{good}\n\n")).unwrap().len(), 1);
        for bad in ["{broken", "{}", r#"{"status":"done"}"#] {
            assert!(parse_history(&format!("{good}\n{bad}")).is_err());
        }
    }

    /// 루팅→언루팅 왕복 시나리오 — 이력 순서 보존 판정
    #[test]
    fn root_then_unroot_scenario() {
        let mut h = vec![];
        for slot in ["_a", "_b"] {
            h.push(entry(&format!("init_boot{slot}"), STOCK, "started"));
            h.push(entry(&format!("init_boot{slot}"), PATCHED, "done")); // 루팅
        }
        assert!(!verify(&h, "init_boot", STOCK, Some(KEY)).ok);
        for slot in ["_a", "_b"] {
            h.push(entry(&format!("init_boot{slot}"), STOCK, "done")); // 언루팅
        }
        assert!(verify(&h, "init_boot", STOCK, Some(KEY)).ok);
    }
}
