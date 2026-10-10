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

// ── 기종별 참고 사항 (Hanabi beta11 소스 + 자동화툴 사용자 보고, 2026-10-05 재검토) ──
// 외부 후속 작업은 안내하며 일반 EFS 경로를 차단하지 않는다. 표 밖 파티션은 추측하지 않는다.
export type SupportLevel = "일반" | "추가 조건" | "지원 미확인";
export interface ModelSupport {
  name: string;
  level: SupportLevel;
  notes: string[];
  source?: string;
}

const IV_NOTES = [
  "KT·LG U+에서 자동화툴 적용 뒤 통화가 안 되어 PDC·모뎀 작업으로 대응한 보고가 있습니다. 펌웨어에 따라 EFS만으로 성공한 보고도 있습니다",
  "1·5 IV는 국내 일반 EFS 패치가 안 먹혀 소프트뱅크(SoftBank) MBN을 강제로 올린 뒤 우회한 사례가 보고됐습니다(일부 통신사 한정). 이 앱은 MBN 강제·모뎀 교체를 자동화하지 않습니다 — 필요하면 외부 도구로 직접 진행해야 합니다",
  "이 앱은 Mark IV 개발 포트 보완을 포함하지만 PDC·모뎀 교체는 수행하지 않습니다",
  "모뎀을 섞은 상태에서 리락한 뒤 실패한 사례가 있습니다",
];
const III_NOTES = ["리락 뒤 VoLTE 설정을 위해 Shizuku·Pixel IMS를 사용하는 안내가 있습니다 — 이 앱과 Hanabi 도구는 해당 앱 설정을 하지 않습니다"];
const II_NOTES = ["Hanabi는 자동화툴의 EFS 포트 개방 뒤 PDC 고정 작업을 별도로 수동 진행하도록 안내합니다 — 이 앱도 고정 작업은 수행하지 않습니다"];

const MODEL_SUPPORT: [string, ModelSupport][] = [
  ["XQ-DQ", { name: "Xperia 1 V", level: "일반", notes: [] }],
  ["XQ-DE", { name: "Xperia 5 V", level: "일반", notes: [] }],
  ["XQ-EC", { name: "Xperia 1 VI", level: "일반", notes: [] }],
  ["XQ-CT", { name: "Xperia 1 IV", level: "추가 조건", notes: IV_NOTES, source: "https://cafe.naver.com/x1smart/614559" }],
  ["XQ-CQ", { name: "Xperia 5 IV", level: "추가 조건", notes: IV_NOTES, source: "https://cafe.naver.com/x1smart/614559" }],
  ["XQ-BC", { name: "Xperia 1 III", level: "추가 조건", notes: III_NOTES, source: "https://cafe.naver.com/x1smart/615748" }],
  ["XQ-BQ", { name: "Xperia 5 III", level: "추가 조건", notes: III_NOTES, source: "https://cafe.naver.com/x1smart/615748" }],
  ["XQ-BE", { name: "Xperia PRO-I", level: "추가 조건", notes: III_NOTES, source: "https://cafe.naver.com/x1smart/613331" }],
  ["XQ-AT", { name: "Xperia 1 II", level: "추가 조건", notes: II_NOTES, source: "https://cafe.naver.com/x1smart/615332" }],
  ["XQ-AS", { name: "Xperia 5 II", level: "추가 조건", notes: II_NOTES, source: "https://cafe.naver.com/x1smart/615332" }],
];

/** 기종별 지원 범위 — 표에 없으면 지원 미확인 */
export function modelSupport(model: string, carriers?: readonly string[], relock = true): ModelSupport {
  const hit = MODEL_SUPPORT.find(([p]) => model.startsWith(p));
  if (!hit) return { name: model, level: "지원 미확인", notes: ["이 앱에서 확인된 패치 절차가 없는 기종입니다. 지원 대상은 심프리(XQ-*) 모델입니다 — 일본 통신사판(도코모·au·소프트뱅크)은 대개 부트로더 언락이 막혀 있어 패치가 불가능합니다(도코모 임시 루트 등 일부 예외 제외)"] };
  const relevant = carriers === undefined || carriers.length > 0;
  const notes = !relevant
    || (hit[1].notes === IV_NOTES && carriers !== undefined && !carriers.some(c => c === "KT" || c === "LGU"))
    || (hit[1].notes === III_NOTES && !relock) ? [] : hit[1].notes;
  return { ...hit[1], notes, level: notes.length ? hit[1].level : "일반" };
}

/** 인식된 기종과 사용자가 고른 작업으로만 절차를 맞춘다. SIM 감지나 펌웨어 최신 여부는 입력이 아니다. */
export function deviceWorkflow(model: string, carriers: readonly string[], relock: boolean) {
  const diagEngineering = model.startsWith("XQ-CT") || model.startsWith("XQ-CQ");
  return {
    partition: bootPartition(model),
    diagEngineering,
    manualPdc: carriers.length > 0 && (model.startsWith("XQ-AT") || model.startsWith("XQ-AS")),
    support: modelSupport(model, carriers, relock),
  };
}

/** 외부 PDC/모뎀 작업과 SIM 감지는 사용자 선택을 차단하지 않는다. 부트 파티션만 확정해야 한다. */
export function patchProcedureProblem(model: string, carriers: readonly string[]): string | null {
  if (!carriers.length) return null;
  if (bootPartition(model) === null) {
    return "이 기종의 부트 파티션·패치 절차가 확인되지 않아 자동 패치를 지원하지 않습니다";
  }
  return null;
}
