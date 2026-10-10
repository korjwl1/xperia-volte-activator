// 기종 레지스트리 — 모델별 사실(부트 파티션·개발 포트·PDC·LGU_V·지원 범위·주의)의 단일 공급원.
//   init_boot = Android 13 이상 출시 기기 (Xperia 1 V, 5 V 이후) / boot = 그 이전 기기 (원본 CLI root() 계승)
// ⚠ 새 기기 추가 = 아래 MODELS 표에 한 줄. prefix는 ro.product.model 접두사. 표에 없는 기종은 null(판별 불가) — 추측하지 않는다.
//   docs/devices.md의 "새 기기 추가" 안내와 함께 유지한다.

export type SupportLevel = "일반" | "추가 조건" | "지원 미확인";
export interface ModelSupport {
  name: string;
  level: SupportLevel;
  notes: string[];
  source?: string;
}

// ── 기종별 참고 사항 (Hanabi beta11 소스 + 자동화툴 사용자 보고, 2026-10-05 재검토) ──
// 외부 후속 작업은 안내하며 일반 EFS 경로를 차단하지 않는다.
const IV_NOTES = [
  "KT·LG U+에서 자동화툴 적용 뒤 통화가 안 되어 PDC·모뎀 작업으로 대응한 보고가 있습니다. 펌웨어에 따라 EFS만으로 성공한 보고도 있습니다",
  "1·5 IV는 국내 일반 EFS 패치가 안 먹혀 소프트뱅크(SoftBank) MBN을 강제로 올린 뒤 우회한 사례가 보고됐습니다(일부 통신사 한정). 이 앱은 MBN 강제·모뎀 교체를 자동화하지 않습니다 — 필요하면 외부 도구로 직접 진행해야 합니다",
  "이 앱은 Mark IV 개발 포트 보완을 포함하지만 PDC·모뎀 교체는 수행하지 않습니다",
  "모뎀을 섞은 상태에서 리락한 뒤 실패한 사례가 있습니다",
];
const III_NOTES = ["리락 뒤 VoLTE 설정을 위해 Shizuku·Pixel IMS를 사용하는 안내가 있습니다 — 이 앱과 Hanabi 도구는 해당 앱 설정을 하지 않습니다"];
const II_NOTES = ["Hanabi는 자동화툴의 EFS 포트 개방 뒤 PDC 고정 작업을 별도로 수동 진행하도록 안내합니다 — 이 앱도 고정 작업은 수행하지 않습니다"];
const UNKNOWN_NOTE = "이 앱에서 확인된 패치 절차가 없는 기종입니다. 지원 대상은 심프리(XQ-*) 모델입니다 — 일본 통신사판(도코모·au·소프트뱅크)은 대개 부트로더 언락이 막혀 있어 패치가 불가능합니다(도코모 임시 루트 등 일부 예외 제외)";

/** 주의 노출 조건 — IV는 KT/LGU 선택일 때만, III/PRO-I는 리락 선택일 때만 (II는 조건 없이 통신사 선택 시) */
type NoteGate = "iv" | "iii";
interface ModelEntry {
  /** ro.product.model 접두사 */
  prefix: string;
  partition: "init_boot" | "boot";
  /** Mark IV 개발 포트(persist.usb.eng). 실제 setprop은 src-tauri/src/efs/diag.rs에 있으며 두 접두사 목록을 함께 유지한다. */
  diagEngineering?: boolean;
  /** EFS 후 PDC 수동 고정이 필요한 기종(안내만) */
  manualPdc?: boolean;
  /** LG U+ 선택 시 전용 LGU_V 프리셋을 쓰는 기종 */
  lguV?: boolean;
  /** 지원 범위·주의. 없으면 "지원 미확인"(부트 파티션은 알아도 패치 절차 미검증) */
  support?: { name: string; level: SupportLevel; notes: string[]; noteGate?: NoteGate; source?: string };
}

const IV_SRC = "https://cafe.naver.com/x1smart/614559";
const III_SRC = "https://cafe.naver.com/x1smart/615748";
const II_SRC = "https://cafe.naver.com/x1smart/615332";

const MODELS: ModelEntry[] = [
  { prefix: "XQ-DQ", partition: "init_boot", lguV: true, support: { name: "Xperia 1 V", level: "일반", notes: [] } },
  { prefix: "XQ-DE", partition: "init_boot", lguV: true, support: { name: "Xperia 5 V", level: "일반", notes: [] } },
  { prefix: "XQ-EC", partition: "init_boot", support: { name: "Xperia 1 VI", level: "일반", notes: [] } },
  { prefix: "XQ-CT", partition: "boot", diagEngineering: true, support: { name: "Xperia 1 IV", level: "추가 조건", notes: IV_NOTES, noteGate: "iv", source: IV_SRC } },
  { prefix: "XQ-CQ", partition: "boot", diagEngineering: true, support: { name: "Xperia 5 IV", level: "추가 조건", notes: IV_NOTES, noteGate: "iv", source: IV_SRC } },
  { prefix: "XQ-BC", partition: "boot", support: { name: "Xperia 1 III", level: "추가 조건", notes: III_NOTES, noteGate: "iii", source: III_SRC } },
  { prefix: "XQ-BQ", partition: "boot", support: { name: "Xperia 5 III", level: "추가 조건", notes: III_NOTES, noteGate: "iii", source: III_SRC } },
  { prefix: "XQ-BE", partition: "boot", support: { name: "Xperia PRO-I", level: "추가 조건", notes: III_NOTES, noteGate: "iii", source: "https://cafe.naver.com/x1smart/613331" } },
  { prefix: "XQ-AT", partition: "boot", manualPdc: true, support: { name: "Xperia 1 II", level: "추가 조건", notes: II_NOTES, source: II_SRC } },
  { prefix: "XQ-AS", partition: "boot", manualPdc: true, support: { name: "Xperia 5 II", level: "추가 조건", notes: II_NOTES, source: II_SRC } },
  // 10 시리즈: 부트 파티션은 알지만 패치 절차 미검증 — support 없이 "지원 미확인"으로 떨어진다 (원본 root(): "10 VI 이하는 boot")
  { prefix: "XQ-CC", partition: "boot" }, // 10 IV
  { prefix: "XQ-DC", partition: "boot" }, // 10 V
  { prefix: "XQ-ES", partition: "boot" }, // 10 VI
];

function modelEntry(model: string): ModelEntry | undefined {
  return MODELS.find((m) => model.startsWith(m.prefix));
}

/** 루팅/언루팅 대상 파티션 — 루팅과 언루팅이 같은 표를 쓴다. 표 밖이면 null(추측 안 함) */
export function bootPartition(model: string): "init_boot" | "boot" | null {
  return modelEntry(model)?.partition ?? null;
}

/** LG U+ 선택 시 전용 LGU_V 프리셋을 쓰는 기종인지 (1 V / 5 V).
 *  모델 코드 어디에든 접두사가 포함되면 참(기존 resolveCarrier의 includes 판정을 그대로 계승). */
export function usesLguVPreset(model: string): boolean {
  return MODELS.some((m) => m.lguV && model.includes(m.prefix));
}

/** 기종별 지원 범위 — 표에 support가 없으면 지원 미확인 */
export function modelSupport(model: string, carriers?: readonly string[], relock = true): ModelSupport {
  const s = modelEntry(model)?.support;
  if (!s) return { name: model, level: "지원 미확인", notes: [UNKNOWN_NOTE] };
  const relevant = carriers === undefined || carriers.length > 0;
  const hide =
    !relevant ||
    (s.noteGate === "iv" && carriers !== undefined && !carriers.some((c) => c === "KT" || c === "LGU")) ||
    (s.noteGate === "iii" && !relock);
  const notes = hide ? [] : s.notes;
  return { name: s.name, level: notes.length ? s.level : "일반", notes, ...(s.source ? { source: s.source } : {}) };
}

/** 인식된 기종과 사용자가 고른 작업으로만 절차를 맞춘다. SIM 감지나 펌웨어 최신 여부는 입력이 아니다. */
export function deviceWorkflow(model: string, carriers: readonly string[], relock: boolean) {
  const e = modelEntry(model);
  return {
    partition: e?.partition ?? null,
    diagEngineering: !!e?.diagEngineering,
    manualPdc: carriers.length > 0 && !!e?.manualPdc,
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
