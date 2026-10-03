// 도메인 타입 — .plans/03-data/mock-schema.md 참조
export type DeviceMode = "android" | "bootloader-fastboot" | "fastbootd" | "flashmode";

export type TriState = boolean | "unknown";

export interface SimInfo {
  slot: 1 | 2;
  type: "physical" | "esim";
  carrier: string | null; // null = SIM 인식 안 됨 (state 참고)
  /** gsm.sim.state 원값: LOADED / ABSENT / PIN_REQUIRED / PUK_REQUIRED / NETWORK_LOCKED / NOT_READY / CARD_IO_ERROR … */
  state: string;
  /** on = 셀룰러 IMS 음성(VoLTE) / wifi = Wi-Fi 통화로만 등록(VoLTE 아님) / off = 미등록 / unknown = 판별 불가 — *#*#4636#*#* IMS 상태와 같은 출처 */
  volte: "on" | "wifi" | "off" | "unknown";
  patchedWith?: string; // 어떤 통신사 프로파일이 적용됐는지 — DIAG 리드백(M5) 전까지 미제공
}

export interface UsbInfo {
  topology: string; // "루트 허브 직결" | "외부 허브 경유(1단계)" ...
  controller: string; // "Intel 칩셋(xHCI)" | "ASMedia ..." — §10-5
  linkSpeed: string; // "SuperSpeed (5Gbps)" | "High-Speed (480Mbps)"
}

export interface DeviceStatus {
  /** adb 연결 상태: device(준비) / unauthorized(USB 디버깅 허용 대기) / offline / usb(다중 USB 자리표시) */
  state: string;
  serial?: string; // 내부용 (UI에서는 serialMasked만 표시)
  serialMasked: string;
  model: string;
  productName: string;
  firmware: string;
  android: string;
  mode: DeviceMode;
  bootloader: "locked" | "unlocked" | "unknown";
  rooted: TriState;
  sims: SimInfo[];
  usb: UsbInfo;
  /** 언락 사전 조건 — null = 판별 불가 */
  prep: {
    developerOptions: boolean | null;
    usbDebugging: boolean | null;
    oemUnlockAllowed: boolean | null;
  };
}

export interface EnvCheckItem {
  id: string;
  label: string;
  state: "pass" | "warn" | "fail" | "info";
  detail: string;
  fixable: boolean;
}

/** 기기 연결 수단 점검 — 백엔드 `adb_status` 계약 (.plans/02-contracts/tauri-commands.md) */
export interface AdbStatus {
  available: boolean;
  /** adb-server: 실행 중인 adb 서버 경유 / usb-direct: USB 직접 연결 / none: 감지된 수단 없음 */
  mode: "adb-server" | "usb-direct" | "none";
  detail: string | null;
}

export type Profile = "clean-return" | "keep-root" | "unroot-only";

export type StepKind =
  | "backup" | "unlock" | "setup" | "root" | "efs-preflight" | "efs" | "verify" | "volte-props"
  | "fw-download" | "fw-flash" | "fw-verify"
  | "unroot" | "relock" | "final-verify" | "restore" | "dexopt";

export type ManualId =
  | "usb-debug" | "su-grant" | "magisk-patch" | "oem-toggle" | "mode-wait" | "ims-check"
  | "unlock-code" | "firmware-select" | "backup-notice" | "flash-mode" | "ims-precheck"
  | "smsie-export" | "smsie-import";

/** 수동 개입 모달 내용 — input이 있으면 입력 완료 전까지 [완료] 비활성 */
export interface ManualPrompt {
  id: ManualId;
  title: string;
  steps: string[];
  input?: "unlock-code" | "firmware";
}

export interface PlanStep {
  id: string;
  kind: StepKind;
  title: string;
  desc: string;
  optional: boolean;
  enabled: boolean; // 선택 단계의 토글 상태
  risk: "safe" | "warn" | "danger";
  wipe: boolean; // 데이터 초기화 발생
  estSec: number;
  /** 단계 시작 시 순서대로 거치는 수동 개입 */
  manual?: ManualId[];
}

export type CarrierId = "SKT" | "KT" | "LGU" | "LGU_V";

/** SIM 상태 표시 문구 — LOADED가 아닌 경우 */
/** 패치 대상 슬롯의 SIM 문제 — 없으면 null (SIM 없음·PIN 잠김·통신사 미확인을 구분) */
export function simIssue(sim: SimInfo | undefined): string | null {
  if (!sim || sim.carrier === null) return simStateLabel(sim?.state ?? "ABSENT");
  if (sim.carrier.trim() === "") return "통신사 확인 불가";
  return null;
}

export function simStateLabel(state: string): string {
  switch (state) {
    case "ABSENT": return "미삽입";
    case "PIN_REQUIRED": return "PIN 잠김";
    case "PUK_REQUIRED": return "PUK 잠김";
    case "NETWORK_LOCKED": return "네트워크 잠김";
    case "NOT_READY": return "SIM 준비 중";
    case "CARD_IO_ERROR": case "CARD_RESTRICTED": return "SIM 오류";
    default: return "SIM 인식 불가";
  }
}

/** SIM 슬롯별 패치 대상 — carrier null = 패치 안 함 (원본 CLI의 'N') */
export interface SimTarget {
  slot: 1 | 2;
  carrier: CarrierId | null;
}

export interface VolteConfig {
  /** EFS 프리셋은 단일(원본 CLI의 balance 프리셋) — 모드 선택 없음 */
  sims: SimTarget[];
  /** 펌웨어 목표 버전 — null 또는 설치된 버전 = 업데이트 안 함 (새 버전만 선택 가능) */
  firmware: string | null;
  /** 부트로더만 작업 — 모든 슬롯이 패치 안 함 + 업데이트 없음일 때만 (사용자 지시 2026-10-03) */
  bootloaderAction: "unlock" | "relock" | null;
}

/** LG U+ 선택 시 1 V / 5 V(XQ-DQ*, XQ-DE*)는 전용 프리셋(LGU_V)으로 자동 대체 */
export function resolveCarrier(carrier: CarrierId, model: string): CarrierId {
  if (carrier === "LGU" && (model.includes("XQ-DQ") || model.includes("XQ-DE"))) return "LGU_V";
  return carrier;
}

export const CARRIER_LABEL: Record<CarrierId, string> = {
  SKT: "SKT",
  KT: "KT",
  LGU: "LG U+",
  LGU_V: "LG U+ (1 V / 5 V 전용)",
};

export type BackupClass = "full" | "partial" | "none";

export interface BackupItem {
  id: string;
  label: string;
  cls: BackupClass;
  note?: string;
  checked: boolean;
  /** 고정 추정치(실측 불가 항목) — 없으면 실측값(storage_sizes)을 사용 */
  estBytes?: number;
}

export interface BackupGroup {
  id: string;
  label: string;
  desc: string;
  items: BackupItem[];
}

/** 앱 복구 분류 — data/appRules.ts 규칙 (모든 폰 공통)
 *  restored = 이 프로그램이 데이터를 복원(외부 데이터 존재) / relogin = 앱만 재설치, 다시 로그인 / lost = 미리 직접 옮기지 않으면 데이터 소실 */
export type AppRecovery = "restored" | "relogin" | "lost";

export interface AppItem {
  pkg: string;
  label: string;
  recovery: AppRecovery;
  note: string;
  /** /sdcard/Android/data/<pkg> 존재 */
  hasExternalData: boolean;
  /** ALLOW_BACKUP 플래그 — 구글 백업 자격(실제 백업 여부 아님) */
  allowBackup: boolean;
  /** 구글 백업에 실제 백업 기록이 있음 (dumpsys backup) */
  googleBackedUp: boolean;
}

/** 순정 펌웨어 부트 이미지 자동 다운로드 결과 — 백엔드 firmware_fetch */
export interface FirmwareResult {
  partition: string;
  path: string;
  version: string;
  fingerprint: string;
  imageBytes: number;
  downloadedBytes: number;
}

/** 서버 펌웨어 버전 목록 — 백엔드 firmware_versions (설치된 버전 이상만) */
export interface FirmwareVersions {
  model: string;
  installed: string;
  supported: boolean;
  versions: { version: string; android: string }[];
}

/** 설정 백업 개요 — 백엔드 settings_overview */
export interface SettingsOverview {
  systemCount: number;
  secureCount: number;
  globalCount: number;
  restoreItems: { namespace: string; key: string; label: string; value: string }[];
  batteryExemptApps: number;
}

/** 백업 실행 결과 — 백엔드 backup_run (계약 .plans/02-contracts) */
export interface BackupSummary {
  complete: boolean;
  files: number;
  bytes: number;
  dir: string;
  errors: string[];
  items: { id: string; status: string; files: number; bytes: number }[];
}

/** 백업·복구 진행 이벤트 페이로드 — 'backup:progress' / 'restore:progress' */
export interface BackupProgress {
  itemId: string;
  phase: string;
  file?: string;
  filesDone: number;
  filesTotal: number;
  bytesDone: number;
  bytesTotal: number;
}

/** SMS Import/Export 수집 결과 — ready=false면 앱에서 아직 내보내지 않음 */
export interface SmsIeOutcome {
  ready: boolean;
  summary: BackupSummary | null;
}

export type RunStatus =
  | "pending" | "running" | "done" | "failed" | "skipped" | "manual-wait";

export interface RunStep {
  id: string;
  title: string;
  status: RunStatus;
  progress: number; // 0..1
  logs: string[];
  /** 완료한 수동 개입 수 (PlanStep.manual 기준) */
  manualDone: number;
  /** 세부 작업 체크포인트 — 이어서 진행 시 끝낸 세부 작업은 건너뛴다 */
  sub?: { list: string[]; done: number };
}

/** 작업 진행 기록 — 끊긴 작업을 같은 폰에서 이어서 진행 (journal.rs, 앱 데이터 폴더)
 *  언락 코드·IMEI는 넣지 않는다 (이어서 진행할 때 다시 입력) */
export interface RunJournal {
  version: 1;
  model: string;
  productName: string;
  serialMasked: string;
  startedAt: string; // ISO
  updatedAt: string;
  config: VolteConfig;
  opts: { unroot: boolean; relock: boolean; restore: boolean };
  backupPath: string;
  /** 실전 백업이 만든 백업 폴더(manifest.json 위치) — 복구·이어받기에 사용 */
  backupDir?: string;
  /** 선택한 백업 항목 id */
  backupItems: string[];
  steps: PlanStep[];
  runSteps: RunStep[];
  cursor: number;
  firmware: FirmwareResult | null;
  firmwareDir: string;
  /** 최종 VoLTE 확인을 생략하고 마무리했는지 */
  imsUnverified?: boolean;
  /** 작업 시작 때의 SIM 구성 — 이어서 진행할 때 바뀌었으면 통신 확인을 다시 */
  sims?: { slot: 1 | 2; carrier: string | null; state: string }[];
  /** 명시적으로 멈춘 경우의 사유 (없으면 진행 중 앱 종료·연결 끊김으로 본다) */
  stop: { stepId: string; stepTitle: string; reason: string; at: string } | null;
}

export const RISK_LABEL: Record<PlanStep["risk"], string> = {
  safe: "안전",
  warn: "주의",
  danger: "위험",
};

