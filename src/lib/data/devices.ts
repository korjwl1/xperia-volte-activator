// 기종별 정보 — 원본 CLI root()의 파티션 기준 계승:
//   init_boot = Android 13 이상 출시 기기 (Xperia 1 V, 5 V 이후) / boot = 그 이전 기기
// ⚠ 확장 시 모델 코드 확인 필요. 표에 없는 기종은 null(판별 불가) — 추측하지 않는다.

const INIT_BOOT_PREFIXES = ["XQ-DQ", "XQ-DE", "XQ-EC"]; // 1 V, 5 V, 1 VI
// 1 IV, 5 IV, 10 IV, 1 II, 5 II, 1 III, 5 III, 10 V, 10 VI (원본 root(): "10 VI 이하는 boot")
const BOOT_PREFIXES = ["XQ-CT", "XQ-CQ", "XQ-CC", "XQ-AT", "XQ-AS", "XQ-BC", "XQ-BQ", "XQ-BE", "XQ-DC", "XQ-ES"];

/** 루팅/언루팅 대상 파티션 — 루팅과 언루팅이 같은 표를 쓴다 */
export function bootPartition(model: string): "init_boot" | "boot" | null {
  if (INIT_BOOT_PREFIXES.some((p) => model.startsWith(p))) return "init_boot";
  if (BOOT_PREFIXES.some((p) => model.startsWith(p))) return "boot";
  return null;
}

// ── 기종별 지원 범위 (카페 기종별 주의 사항 cafe.naver.com/x1smart/613331 등, 2026-10-03 조사) ──
// 하나의 패치 절차로 모든 기종을 처리할 수 없다. 표에 없는 기종은 "지원 미확인" — 추측하지 않는다.
export type SupportLevel = "일반" | "추가 조건" | "지원 미확인";
export interface ModelSupport {
  name: string;
  level: SupportLevel;
  notes: string[];
  source?: string;
}

const CAFE_MODEL_NOTES = "https://cafe.naver.com/x1smart/613331";
const IV_NOTES = [
  "KT·LG U+는 SoftBank 모뎀을 먼저 적용한 뒤 패치하는 절차가 안내되어 있습니다 — 이 앱은 모뎀 교체를 하지 않습니다",
  "모뎀을 섞은 상태에서 리락한 뒤 실패한 사례가 있습니다",
];
const III_NOTES = ["리락 후 VoLTE를 켜려면 Shizuku·Pixel IMS 설정이 필요하다는 안내가 있습니다 — 이 앱은 해당 설정을 하지 않습니다"];
const II_NOTES = ["패치 고정 작업(PDC)이 필요하다는 안내가 있습니다 — 이 앱의 절차만으로는 패치가 유지되지 않을 수 있습니다"];

const MODEL_SUPPORT: [string, ModelSupport][] = [
  ["XQ-DQ", { name: "Xperia 1 V", level: "일반", notes: [] }],
  ["XQ-DE", { name: "Xperia 5 V", level: "일반", notes: [] }],
  ["XQ-EC", { name: "Xperia 1 VI", level: "일반", notes: [] }],
  ["XQ-CT", { name: "Xperia 1 IV", level: "추가 조건", notes: IV_NOTES, source: CAFE_MODEL_NOTES }],
  ["XQ-CQ", { name: "Xperia 5 IV", level: "추가 조건", notes: IV_NOTES, source: CAFE_MODEL_NOTES }],
  ["XQ-BC", { name: "Xperia 1 III", level: "추가 조건", notes: III_NOTES, source: CAFE_MODEL_NOTES }],
  ["XQ-BQ", { name: "Xperia 5 III", level: "추가 조건", notes: III_NOTES, source: CAFE_MODEL_NOTES }],
  ["XQ-BE", { name: "Xperia PRO-I", level: "추가 조건", notes: III_NOTES, source: CAFE_MODEL_NOTES }],
  ["XQ-AT", { name: "Xperia 1 II", level: "지원 미확인", notes: II_NOTES, source: CAFE_MODEL_NOTES }],
  ["XQ-AS", { name: "Xperia 5 II", level: "지원 미확인", notes: II_NOTES, source: CAFE_MODEL_NOTES }],
];

/** 기종별 지원 범위 — 표에 없으면 지원 미확인 */
export function modelSupport(model: string): ModelSupport {
  const hit = MODEL_SUPPORT.find(([p]) => model.startsWith(p));
  return hit ? hit[1] : { name: model, level: "지원 미확인", notes: ["이 앱에서 확인된 패치 절차가 없는 기종입니다"] };
}

/** SIM detection never changes the user's targets. These restrictions concern missing model procedures only. */
export function patchProcedureProblem(model: string, carriers: readonly string[]): string | null {
  if (!carriers.length) return null;
  if (bootPartition(model) === null) {
    return "이 기종의 부트 파티션·패치 절차가 확인되지 않아 자동 패치를 지원하지 않습니다";
  }
  if (["XQ-AT", "XQ-AS"].some(prefix => model.startsWith(prefix))) {
    return "Mark II에 필요한 PDC 고정 작업을 이 앱에서 수행하지 못해 자동 패치를 지원하지 않습니다";
  }
  if (["XQ-CT", "XQ-CQ"].some(prefix => model.startsWith(prefix)) && carriers.some(c => c === "KT" || c === "LGU")) {
    return "Mark IV의 KT·LG U+ 패치에는 별도 모뎀 작업이 필요합니다. 이 앱은 모뎀 교체·선행 조건 검증을 지원하지 않아 자동 패치를 진행할 수 없습니다";
  }
  return null;
}
