// 기종별 정보 — 원본 CLI root()의 파티션 기준 계승:
//   init_boot = Android 13 이상 출시 기기 (Xperia 1 V, 5 V 이후) / boot = 그 이전 기기
// ⚠ 확장 시 모델 코드 확인 필요. 표에 없는 기종은 null(판별 불가) — 추측하지 않는다.

const INIT_BOOT_PREFIXES = ["XQ-DQ", "XQ-DE", "XQ-EC"]; // 1 V, 5 V, 1 VI
// 1 IV, 5 IV, 10 IV, 1 II, 5 II, 1 III, 5 III, 10 V, 10 VI (원본 root(): "10 VI 이하는 boot")
const BOOT_PREFIXES = ["XQ-CT", "XQ-CQ", "XQ-CC", "XQ-AT", "XQ-AS", "XQ-BC", "XQ-BQ", "XQ-DC", "XQ-ES"];

/** 루팅/언루팅 대상 파티션 — 루팅과 언루팅이 같은 표를 쓴다 */
export function bootPartition(model: string): "init_boot" | "boot" | null {
  if (INIT_BOOT_PREFIXES.some((p) => model.startsWith(p))) return "init_boot";
  if (BOOT_PREFIXES.some((p) => model.startsWith(p))) return "boot";
  return null;
}
