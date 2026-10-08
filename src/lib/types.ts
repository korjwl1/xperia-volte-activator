// 도메인 타입 — .plans/03-data/mock-schema.md 참조
export type ApiResult<T> = { ok: true; value: T } | { ok: false; error: string };
export type Unsubscribe = () => void;
export type DeviceMode = "android" | "bootloader-fastboot" | "fastbootd" | "flashmode";

export type TriState = boolean | "unknown";

export interface ImsDiagnostic {
  status: "no-sim" | "sim-not-ready" | "query-failed" | "unsupported-format" | "conflicting-evidence" | "not-registered" | "registering" | "voice-unavailable" | "registered" | "wifi-only" | "cross-sim" | "other-network" | "transport-unknown";
  registration: "registered" | "registering" | "not-registered" | "unknown";
  voice: boolean | null;
  sms: boolean | null;
  transport: "cellular" | "wifi" | "other" | "unknown";
  technology: "lte" | "nr" | "iwlan" | "cross-sim" | "3g" | "unknown";
}

export interface CallCheck {
  slot: 1 | 2;
  outgoing: boolean;
  incoming: boolean;
  audio: boolean;
  afterReboot: boolean;
  afterIdle: boolean;
}

/** No raw dumps, device serials, subscriber identifiers or phone numbers. */
export interface CommunicationSnapshot {
  checkedAt: string;
  outcome: "observed" | "query-failed" | "disconnected";
  model: string;
  firmware: string;
  fingerprint: string;
  android: string;
  baseband: string;
  sims: SimInfo[];
  presets: { slot: 1 | 2; carrier: CarrierId; version: string; sha256: string }[];
}

export interface SimInfo {
  slot: 1 | 2;
  /** Subscription evidence only; missing or conflicting evidence remains unknown. */
  type: "physical" | "esim" | "unknown";
  carrier: string | null; // null = SIM 인식 안 됨 (state 참고)
  /** gsm.sim.state 원값: LOADED / ABSENT / PIN_REQUIRED / PUK_REQUIRED / NETWORK_LOCKED / NOT_READY / CARD_IO_ERROR … */
  state: string;
  /** on = 셀룰러 IMS 음성 준비 / wifi = Wi-Fi 통화 등록 / off = 미등록·음성 불가 / unknown = 판별 불가. LTE/NR 구분은 ims.technology, 실제 통화는 별도 확인. */
  volte: "on" | "wifi" | "off" | "unknown";
  ims?: ImsDiagnostic;
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
  /** ro.build.fingerprint — 업데이트 확인용(백엔드 실측, mock에는 없을 수 있음) */
  fingerprint?: string;
  baseband?: string;
  observedAtMs?: number;
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

export type WorkflowMode = "automatic" | "manual" | "update";
export const MANUAL_TASK_IDS = ["backup", "restore", "unlock", "relock", "root", "unroot", "volte", "verify"] as const;
export type ManualTask = (typeof MANUAL_TASK_IDS)[number];

/** 진행 기록 검증(domain/journal.ts)도 이 목록을 쓴다 — 타입과 검증 목록이 어긋나지 않게 한 곳에서 정의 */
export const STEP_KINDS = [
  "backup", "unlock", "setup", "root", "efs-preflight", "efs", "verify", "volte-props",
  "fw-download", "fw-flash", "fw-verify",
  "unroot", "relock", "final-verify", "restore", "dexopt",
] as const;
export type StepKind = (typeof STEP_KINDS)[number];

export const MANUAL_IDS = [
  "usb-debug", "su-grant", "magisk-patch", "oem-toggle", "mode-wait", "ims-check",
  "unlock-code", "firmware-select", "backup-notice", "flash-mode", "ims-precheck",
  "smsie-export", "smsie-import", "contacts-import",
] as const;
export type ManualId = (typeof MANUAL_IDS)[number];

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
/** backup_prepare 결과 — 저장 위치의 xva-<모델>-backup. existing이면 같은 폰의 기존 백업을 바뀐 파일만 갱신 */
export interface PreparedBackup {
  dir: string;
  existing: boolean;
}

export interface BackupSummary {
  deviceKey?: string | null;
  complete: boolean;
  files: number;
  bytes: number;
  dir: string;
  errors: string[];
  items: { id: string; status: string; files: number; bytes: number }[];
  omittedApps?: { package: string; reasons: string[]; removedFiles: number; removedBytes: number; cleanupPending: boolean }[];
  sourceMetadata?: { itemId: string; path: string; sha256: string; entries: number; directories: number; unavailableBirthTimes: number; payloadMismatches: number; complete: boolean; errors: string[]; captureContext: string }[];
}

/** 백업·복구 진행 이벤트 페이로드 — 'backup:progress' / 'restore:progress' */
export interface BackupProgress {
  itemId: string;
  phase: string;
  file?: string | null;
  filesDone: number;
  filesTotal: number;
  bytesDone: number;
  bytesTotal: number;
}

/** SMS Import/Export 수집 결과 — ready=false면 앱에서 아직 내보내지 않음 */
export interface SmsIeOutcome {
  cleanupWarning?: string | null;
  ready: boolean;
  summary: BackupSummary | null;
}

/** 복구 실행 결과 — 자동 복구 로그·실패 목록(실패가 있어도 나머지는 진행) */
export interface RestoreOutcome {
  logs: string[];
  failures: string[];
  /** 문자·통화 기록(smsie) 수동 복원이 남아 있음 — 수동 개입 단계로 진행 */
  smsiePending: boolean;
  /** 폰에서 연락처 가져오기가 남음 — 이미 계정 동기화로 백업 수만큼 있으면 false(단계 생략) */
  contactsPending: boolean;
}

/** fastboot getvar 결과 — unlocked·current-slot·slot-successful:a/b·max-download-size … */
export type FastbootVars = Record<string, string>;

/** 언락/리락 실행 결과 — getvar로 이중 확인한 값 */
export interface UnlockResult {
  unlocked: boolean;
}

/** 리락 게이트(§3-3) 사전 점검 결과 — 백엔드 relock_gate_check */
export interface RelockGate {
  ok: boolean;
  reasons: string[];
  checked: { partition: string; slot: string; ok: boolean; detail: string }[];
}

/** Magisk APK 확보 결과 — 백엔드 magisk_prepare */
export interface MagiskPrepared {
  version: string;
  apkPath: string;
  sha256: string;
}

/** 부트 패치 결과 — 백엔드 magisk_patch (ANDROID!·크기·해시 검증 통과분) */
export interface PatchResult {
  path: string;
  origSha256: string;
  patchedSha256: string;
  bytes: number;
  log: string[];
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
  /** 사용자가 통신 확인을 생략했는지 — 파일 기록 결과와 독립적인 선택 */
  communicationSkipped?: boolean;
  /** 세부 작업 체크포인트 — 이어서 진행 시 끝낸 세부 작업은 건너뛴다 */
  sub?: { list: string[]; done: number };
}

/** 작업 진행 기록 — 끊긴 작업을 같은 폰에서 이어서 진행 (journal.rs, 앱 데이터 폴더)
 *  언락 코드·IMEI는 넣지 않는다 (이어서 진행할 때 다시 입력) */
export interface RunJournal {
  /** Completed step whose explicit Next click is still pending. */
  awaitingNext?: string | null;
  backupOmissions?: { apps: NonNullable<BackupSummary["omittedApps"]>; pending: boolean };
  version: 1;
  model: string;
  productName: string;
  serialMasked: string;
  startedAt: string; // ISO
  updatedAt: string;
  config: VolteConfig;
  opts: { mode?: WorkflowMode; manualTask?: ManualTask; unroot: boolean; relock: boolean; restore: boolean; backupOnly?: boolean };
  backupPath: string;
  /** 실전 백업이 만든 백업 폴더(manifest.json 위치) — 복구·이어받기에 사용 */
  backupDir?: string;
  /** 실전 루팅 산출물의 경로 — 언루팅 입력으로 사용하지 않는다. */
  patchedImage?: string;
  /** 선택한 백업 항목 id */
  backupItems: string[];
  steps: PlanStep[];
  runSteps: RunStep[];
  cursor: number;
  firmware: FirmwareResult | null;
  firmwareDir: string;
  /** 최종 VoLTE 확인을 생략하고 마무리했는지 */
  imsUnverified?: boolean;
  /** 최종 단계에서 실제 기기 IMS 등록을 확인했는지 (이전 기록은 미확인) */
  imsVerified?: boolean;
  /** 사용자가 대상 슬롯 모두에서 실제 발신·수신·양방향 음성을 확인했는지 */
  callVerified?: boolean;
  communication?: { before: CommunicationSnapshot | null; latest: CommunicationSnapshot | null; calls: CallCheck[] };
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

/** Native EFS/NV results. Phone payloads and credentials remain in Rust. */
export interface EfsError { code: string; operation: string; message: string; status?: number; cleanup?: string[] }
export type EfsResult<T> = { ok: true; value: T } | { ok: false; error: string; details?: EfsError };
export interface EfsWarning { code: string; target: string; message: string }
export interface EfsConfiguration { port: string; presetRoot: string; snapshotRoot: string }
export interface EfsToolCheck { version: string; path: string; native: boolean; deviceExecution: boolean; rootExecution: boolean; fastbootExecution: boolean }
export interface EfsPreflight { log: string[]; errors: string[]; warnings: string[]; parameters: number[] }
export interface EfsUploadResult { errors: string[]; filesSeen: number; planned: number; skipped: number; warnings: EfsWarning[] }
export interface EfsVerifyReport { ok: boolean; files: number; matched: number; planned: number; skipped: number; missing: string[]; mismatches: string[]; warnings: EfsWarning[] }
export interface EfsSnapshotResult { path: string; filesSeen: number; warnings: EfsWarning[] }
export interface EfsProgress { operation: string; file: string; n: number; total: number }
export interface EfsLogEvent { cmd: string; line: string }

/** 수동 SIN 추출 결과 — path는 실제 패치·기록에 사용할 raw IMG 경로다. */
export interface FirmwareDirInfo {
  file: string;
  path: string;
  fingerprint: string;
  imageBytes: number;
}

/** Magisk 패치 입력 — 사전 준비 이미지·APK 해시와 현재 펌웨어 지문을 포함한다. */
export interface MagiskPatchRequest {
  serial: string;
  apkPath: string;
  imagePath: string;
  partition: "boot" | "init_boot";
  imageSha256: string;
  fingerprint: string;
  apkSha256: string;
}
