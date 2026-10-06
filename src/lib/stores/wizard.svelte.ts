// 위자드 상태 머신 + 실행 시뮬레이션 러너 (mock)
import type { CallCheck, CommunicationSnapshot, FirmwareDirInfo, AppItem, FirmwareResult, FirmwareVersions, SettingsOverview, SimInfo, BackupGroup, DeviceStatus, EnvCheckItem, ManualId, ManualPrompt, PlanStep, RunJournal, RunStep, VolteConfig } from "$lib/types";
import { api } from "$lib/api";
import { LINKS, maskSecret } from "$lib/data/links";
import { deviceWorkflow, patchProcedureProblem } from "$lib/data/devices";
import { mockBackupGroups } from "$lib/mock/apps";
import { bootloaderOnly, buildPlan, stepHazard, updateTarget, type PlanOptions } from "$lib/domain/plan";
import { firmwareUpdateProblems } from "$lib/domain/verify";
import { SIMULATED_RUN, REAL_STEPS } from "$lib/data/runMode";
import { buildFeatureProblem, executionPlanProblem, hasLiveActions, liveStepEnabled } from "$lib/domain/execution";
import { EFS_PRESET_MODE, EFS_PRESET_VERSION, efsPreset } from "$lib/data/efsPresets";
import type { BackupProgress, BackupSummary, EfsConfiguration } from "$lib/types";
import { AsyncQueue } from "$lib/domain/asyncQueue";
import { decodeJournal } from "$lib/domain/journal";
import { waitUntil } from "$lib/domain/waitUntil";
import { callsComplete, cellularReady, imsLabel, newCallCheck, type CallItem } from "$lib/domain/communication";

/** 목 모드 언락 코드 예시값 — 16자리 16진수 형식만 맞춘 가짜 값(실전 fastboot에서는 채우지 않음) */
const MOCK_UNLOCK_CODE = "0x1234567890ABCDEF";

export type WizardView = "device" | "warning" | "step1" | "step2" | "step3" | "step4";

export const MACRO_STEPS = [
  { id: 1, view: "step1" as const, label: "사전 옵션 선택" },
  { id: 2, view: "step2" as const, label: "작업 옵션 선택" },
  { id: 3, view: "step3" as const, label: "VoLTE 패치 진행" },
  { id: 4, view: "step4" as const, label: "점검 및 마무리" },
] as const;

type LoadState = "idle" | "loading" | "done" | "failed";

const MANUAL_TEXT: Record<ManualId, Omit<ManualPrompt, "id">> = {
  // 백업 직전 안내 — 전용 화면(BackupNotice)으로 표시, 동의 체크 후 진행
  "backup-notice": {
    title: "백업 전 확인",
    steps: [],
  },
  "unlock-code": {
    title: "언락 코드 받기",
    input: "unlock-code",
    steps: [
      "브라우저로 열린 Sony 언락 코드 발급 페이지에서 기기 모델을 선택합니다 (쿠키 팝업은 Accept)",
      "IMEI 칸에 아래 [IMEI 복사]로 복사한 IMEI 1을 붙여넣습니다",
      "동의 체크 2개 → Submit → 보안 확인(캡차) → 화면에 표시된 언락 코드를 복사해 아래에 붙여넣습니다",
    ],
  },
  // 자동 다운로드가 실패했을 때만 표시 — 원인(공간 부족/다운로드 실패)별 안내는 화면에서 구성
  "firmware-select": {
    title: "순정 펌웨어 준비",
    input: "firmware",
    steps: [],
  },
  "su-grant": {
    title: "루트 권한 승인",
    steps: ["폰 화면에 Magisk 루트 권한 요청이 뜨면 '허용' 선택"],
  },
  "oem-toggle": {
    title: "언락 조건 확인",
    steps: [
      "개발자 옵션: 설정 > 휴대전화 정보에서 빌드번호를 개발자 옵션이 활성화될 때까지 연속으로 터치",
      "설정 > 시스템 > 개발자 옵션에서 OEM 잠금 해제와 USB 디버깅 활성화",
      "켠 뒤 [다시 확인]을 누르면 폰에서 바로 확인합니다",
    ],
  },
  "flash-mode": {
    title: "플래시 모드 진입",
    steps: [
      "폰 전원을 완전히 끕니다 (USB 연결 해제)",
      "볼륨 아래 버튼을 누른 채 USB 케이블을 연결합니다 — 알림 LED가 초록색이면 플래시 모드입니다",
      "자동 감지되면 펌웨어 기록이 시작됩니다 (사용자 데이터는 유지됩니다)",
    ],
  },
  "mode-wait": {
    title: "부트로더 모드 진입 대기",
    steps: ["앱이 폰을 부트로더 모드로 재부팅합니다 (파란색 LED)", "USB 연결을 유지해 주세요", "부트로더 모드가 감지되면 자동으로 다음 단계로 진행됩니다"],
  },
  "usb-debug": {
    title: "USB 디버깅 승인 대기",
    steps: ["재부팅 후 초기 설정을 마치고 개발자 옵션 활성화", "USB 디버깅 켜기", "PC 연결 시 폰 화면에서 '허용' 선택 — 연결이 확인되면 자동으로 진행됩니다"],
  },
  "magisk-patch": {
    title: "Magisk 부트 패치 (폰 조작)",
    steps: [
      "Magisk 앱 설치와 시스템 이미지 전송은 자동으로 진행됩니다",
      "폰의 Magisk 앱 → Magisk 영역의 설치 → 파일 선택 및 패치 → 전송된 img 파일 선택",
      "패치가 완료되면 자동으로 감지됩니다",
    ],
  },
  "ims-precheck": {
    title: "언루팅·리락 전 통신 확인",
    steps: [
      "재부팅 후 통신사 신호가 잡히면 앱이 VoLTE(IMS) 등록을 확인합니다",
      "다른 전화로 실제로 걸고 받아 통화가 되는지 확인해 주세요 — 리락한 뒤에는 다시 고치려면 초기화가 한 번 더 필요합니다",
      "통화가 안 되면 [다시 패치]로 VoLTE 적용부터 다시 진행할 수 있습니다",
      "SIM이 없거나 지금 확인하지 않을 경우 통신 확인을 생략하고 계속할 수 있습니다. 파일 기록 결과는 유지됩니다",
    ],
  },
  "ims-check": {
    title: "최종 VoLTE 확인",
    steps: [
      "폰이 완전히 부팅되고 통신사 신호를 잡을 때까지 기다립니다 (2~3분)",
      "앱이 셀룰러 IMS 음성 등록 상태를 확인합니다. Wi-Fi를 잠시 끄고 대상 슬롯으로 발신·수신과 양쪽 목소리를 확인했다면 각각 체크해 주세요",
      "SIM이 없거나 통화 확인을 하지 않을 경우 [통신 확인 없이 마무리]를 누르세요 — 파일 기록 결과와 통신 확인 결과는 따로 표시됩니다",
    ],
  },
  "smsie-export": {
    title: "문자·통화 기록 내보내기 (폰 조작)",
    steps: [
      "SMS Import/Export 앱을 설치·실행하고 내장 저장소에 volte_sms_backup 폴더를 미리 만듭니다",
      "폰에서 Export Messages를 누릅니다 → 내장 저장소 → volte_sms_backup 폴더 선택 → 저장",
      "문자 내보내기 성공 안내를 확인한 뒤 Export Call Log를 누릅니다 → 같은 volte_sms_backup 폴더 선택 → 저장",
      "문자는 messages-날짜.zip, 통화 기록은 calls-날짜.json으로 저장됩니다. 날짜·중복 번호가 달라도 감지합니다",
      "저장이 끝나면 앱이 파일을 감지해 PC로 수집·검사하고 자동으로 다음 단계로 넘어갑니다",
    ],
  },
  "contacts-import": {
    title: "연락처 가져오기 (폰 조작)",
    steps: [
      "백업한 연락처 파일(contacts-restore.vcf)을 폰의 내장 저장소에 올려 두었습니다",
      "폰의 연락처 앱을 열고 메뉴(⋮ 또는 ☰) → 설정 → '가져오기'를 누릅니다",
      "'.vcf 파일'을 고르고 내장 저장소에서 contacts-restore.vcf를 선택합니다",
      "저장할 곳을 묻으면 쓰실 Google 계정(또는 기기)을 고릅니다",
      "가져오기가 끝나면 [확인하고 진행]을 누르세요 — 폰의 연락처 수가 백업과 맞는지 확인합니다",
    ],
  },
  "smsie-import": {
    title: "문자·통화 기록 복원 (폰 조작)",
    steps: [
      "먼저 폰을 비행기 모드로 전환해 주세요 (기본 문자 앱이 바뀌는 동안 수신 문자가 유실되지 않게)",
      "PC 백업 파일을 검증한 뒤 내장 저장소의 volte_sms_backup 폴더에 올려 두고 앱을 엽니다",
      "Import Messages → 내장 저장소 → volte_sms_backup → messages로 시작하는 .zip 파일 선택",
      "Import Call Log → 내장 저장소 → volte_sms_backup → calls로 시작하는 .json 파일 선택",
      "선택한 항목 모두의 가져오기 성공 안내를 확인한 뒤 [확인하고 진행]을 누릅니다 — 기본 문자 앱을 원래대로 돌립니다",
    ],
  },
};

const defaultVolteConfig = (): VolteConfig => ({
  sims: [
    { slot: 1, carrier: null },
    { slot: 2, carrier: null },
  ],
  firmware: null,
  bootloaderAction: null,
});

/** 백업 항목 id → 화면 라벨 (mock/apps.ts 정의와 동일 — 로그·체크포인트 표기용) */
const BACKUP_ITEM_LABEL: Record<string, string> = Object.fromEntries(
  mockBackupGroups.flatMap((group) => group.items.map((item) => [item.id, item.label])),
);

/** 단계별 메모리 로그 상한 — 오래 걸리는 실전 단계에서도 무한히 늘지 않게 (진행 기록에는 최근 300줄만 저장) */
const STEP_LOG_LIMIT = 2000;

/** 문자열의 SHA-256 hex (bytes를 주면 앞쪽 n바이트만) */
async function sha256Hex(text: string, bytes?: number): Promise<string> {
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text)));
  return [...digest.slice(0, bytes ?? digest.length)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

/** 백업·복구 진행 이벤트 → 단계 진행률 (항목 순서 기준, 끝나기 전에는 99%까지).
 *  한 항목 안의 단계(목록 확인 → 원본 속성 → 복사)와 재개 전 PC 검사에 구간을 따로 줘서 뒤로 가지 않게 한다.
 *  mode: "run" 새 백업·복구 / "resume" 재개(앞 10%는 PC 검사) / "verify" PC 검사만. 계산할 수 없는 이벤트는 NaN(현재 값 유지) */
export function itemProgress(p: BackupProgress, itemOrder: string[], mode: "run" | "resume" | "verify" = "run"): number {
  const ratio = p.bytesTotal > 0 ? p.bytesDone / p.bytesTotal : p.filesTotal > 0 ? p.filesDone / p.filesTotal : 0;
  const idx = itemOrder.indexOf(p.itemId);
  if (idx < 0) return NaN; // 예: 격리 tar 검사(quarantine) — 항목 순서에 없음
  const n = Math.max(1, itemOrder.length);
  if (mode === "verify") return Math.min(0.99, (idx + ratio) / n);
  const verifyShare = mode === "resume" ? 0.1 : 0;
  if (p.phase === "verify") return Math.min(0.99, (verifyShare * (idx + ratio)) / n);
  const within = p.phase === "scan" ? 0
    : p.phase === "metadata" ? 0.1 * ratio
    : p.phase === "copy" ? 0.1 + 0.9 * ratio
    : ratio;
  return Math.min(0.99, verifyShare + ((1 - verifyShare) * (idx + within)) / n);
}

/** 진행 카드에 상시 표시할 현재 작업 — 항목 이름 [끝난 파일 수 / 전체 파일 수] */
export function transferStatusText(p: BackupProgress): string {
  const label = BACKUP_ITEM_LABEL[p.itemId] ?? (p.itemId === "quarantine" ? "격리 파일" : p.itemId);
  if (p.phase === "scan") return `${label} 목록 확인 중${p.file ? ` — ${p.file}` : ""}`;
  if (["done", "partial", "pending"].includes(p.phase)) return "";
  const phase = p.phase === "metadata" ? " 원본 속성" : p.phase === "verify" ? " PC 검사" : "";
  const count = p.filesTotal > 0 ? ` [${p.filesDone.toLocaleString()} / ${p.filesTotal.toLocaleString()}]` : "";
  return `${label}${phase}${count}`;
}

/** 진행률은 같은 실행 안에서 뒤로 가지 않는다 — 계산할 수 없는 이벤트는 무시 */
function advance(current: number, next: number): number {
  return Number.isFinite(next) ? Math.max(current, next) : current;
}

/** 실전 엔진 단계 — 세대마다 한 번 시작(start), 수동 개입을 마치고 재진입하면 resume. danger: 기기 쓰기(창 닫기 차단) */
interface EngineStep {
  start: () => Promise<void>;
  resume?: () => void;
  danger: boolean;
}

export class Wizard {
  view = $state<WizardView>("device");
  device: DeviceStatus | null = $state(null);
  env: EnvCheckItem[] = $state([]);
  envLoading = $state(false);
  private envReq = 0;

  /** PC 환경 점검(읽기 전용) — 첫 화면 진입·[다시 확인] 때. 늦게 온 이전 응답은 버린다 */
  async loadEnv() {
    const req = ++this.envReq;
    this.envLoading = true;
    try {
      const items = await api.envCheck();
      // 조회 실패(null)는 이전 결과를 유지한다 — [다시 확인] 실패로 안내가 사라져 설치된 것처럼 보이지 않게
      if (req === this.envReq && items) this.env = items;
    } finally {
      if (req === this.envReq) this.envLoading = false;
    }
  }

  /** VoLTE 적용(EFS)에 필요한 DIAG 드라이버가 없다고 확인된 경우만 — 확인 불가는 단정하지 않는다 */
  get diagDriverMissing(): EnvCheckItem | null {
    return this.env.find((e) => e.id === "diag-driver" && (e.state === "warn" || e.state === "fail")) ?? null;
  }
  volteConfig = $state<VolteConfig>(defaultVolteConfig());

  /** 패치할 슬롯이 하나라도 있는지 */
  get hasPatchTarget(): boolean {
    return this.volteConfig.sims.some((s) => s.carrier !== null);
  }

  /** 펌웨어 업데이트 대상 버전 (없으면 null) */
  get updateVersion(): string | null {
    return updateTarget(this.device, this.volteConfig);
  }

  /** 부트로더만 작업(언락만/리락만) — 유효할 때만 값 */
  get bootloaderOnly(): "unlock" | "relock" | null {
    return bootloaderOnly(this.device, this.volteConfig);
  }

  /** 1단계 [다음] 활성 조건 — VoLTE 패치, 펌웨어 업데이트, 부트로더만 작업 중 하나 이상 */
  get hasAnyTask(): boolean {
    return !!this.opts.backupOnly || this.hasPatchTarget || this.updateVersion !== null || this.bootloaderOnly !== null;
  }

  // ── 펌웨어 버전 (1단계 사전 옵션) ──
  fwVersions: FirmwareVersions | null = $state(null);
  fwVersionsState = $state<LoadState>("idle");
  private fwVersionsFor: string | null = null;

  /** 1단계 진입 시 기기당 1회 서버 버전 조회 */
  ensureFirmwareVersions() {
    const d = this.device;
    if (!d) return;
    const key = d.serial ?? d.serialMasked;
    if (this.fwVersionsFor === key) return;
    this.fwVersionsFor = key;
    this.fwVersionsState = "loading";
    void api.firmwareVersions(d.serial).then((v) => {
      if (this.fwVersionsFor !== key) return;
      this.fwVersions = v;
      this.fwVersionsState = v ? "done" : "failed";
      // 일시적 실패는 다음 진입 때 다시 조회
      if (!v) this.fwVersionsFor = null;
    });
  }

  /** 선택·입력값이 속한 기기 — 다른 기기로 [작업 시작]하면 초기화 */
  private sessionFor: string | null = null;

  /** 1페이지 [작업 시작] — 기기를 고정하고, 이전과 다른 기기면 선택·입력값을 새로 시작 */
  startSession() {
    const d = this.device;
    if (!d) return;
    const key = `${d.model}|${d.serial ?? d.serialMasked}`;
    if (this.sessionFor !== key) {
      // 이전 기기의 실전 백업이 남아 있으면 백엔드에도 취소 전달
      if (this.runSteps.length > 0) void api.backupCancel(this.backupRunId);
      this.resetSession();
      this.sessionFor = key;
    }
    this.opts = { ...this.opts, backupOnly: false };
    this.view = "warning";
  }

  async startBackupSession() {
    this.startSession();
    this.opts = { unroot: false, relock: false, restore: false, backupOnly: true };
    this.volteConfig = defaultVolteConfig();
    this.optionsFor = null;
    this.journalReadBlocked = true;
    this.pendingJournal = null;
    this.view = "step2";
    const gen=this.runGen;
    const pending=await this.checkJournal();
    if (gen!==this.runGen) return;
    if (!pending) {
      this.backupDir = "";
      this.backupSummary = null;
    }
  }

  /** 실행 진행 상태 초기화 — 새 실행·재개·세션 변경·처음으로 공통 (실행 결과물·입력값은 유지) */
  private resetExecution() {
    void api.efsCancel();
    this.efsRunConfiguration = null;
    this.backupIncompleteAccepted = false;
    this.transferStatus = "";
    this.runGen++;
    this.pause();
    this.stopWatch();
    this.cursor = 0;
    this.finished = false;
    this.awaitingNext = null;
    this.backupOmissionNotice = false;
    this.backupOmittedApps = [];
    this.manualCurrent = null;
    this.manualChecking = false;
    this.manualCheckError = "";
    this.manualSetupState = "idle";
    this.smsieFilesReady = false;
    this.smsieRestoreDetails = "";
    this.backupNoticeAck = false;
    this.callChecks = [];
    this.communicationReq++;
    this.communicationLoading = false;
    this.communicationError = "";
    this.communicationLatest = null;
    this.imsSims = [];
    this.imsVerified = false;
    this.callVerified = false;
    this.imsUnverified = false;
    this.oemUnknownAck = false;
    this.stepError = "";
    this.stopInfo = null;
    this.usbError = false;
    this.erroredOnce = false;
    this.efsFailedOnce = false;
    this.engineRan.clear();
  }

  /** 기기 세션 초기화 — 실행 상태·결과물·실행 중 입력값 전부 (2단계 선택값은 ensureOptions가 기기별로 관리) */
  private resetSession() {
    this.resetExecution();
    // 기기 쓰기가 아직 끝나지 않았으면 PC 보호는 그 작업이 끝날 때 해제된다(dispatchEngine)
    this.communicationBefore = null;
    if (this.dangerBusy === 0) this.setGuard(false);
    this.steps = [];
    this.runSteps = [];
    this.backupDir = "";
    this.patchedImage = "";
    this.backupSummary = null;
    this.backupDeleteState = "idle";
    this.backupDeleteError = "";
    this.imsUnverified = false;
    this.usbErrorCount = 0;
    this.simulateUsbError = false;
    this.simulateEfsFail = false;
    this.journalKey = null;
    this.journalError = "";
    this.journalReadBlocked = false;
    this.journalMismatch = false;
    this.journalStarted = "";
    this.journalSims = undefined;
    this.pendingJournal = null;
    this.volteConfig = defaultVolteConfig();
    this.omdAck = false;
    this.riskAck = false;
    this.unlockCode = "";
    this.imei1 = null;
    this.imeiState = "idle";
    this.firmware = null;
    this.firmwareState = "idle";
    this.firmwareError = "";
    this.firmwareFail = null;
    this.firmwareDest = "";
    this.firmwareDir = "";
    this.firmwareDirInfo = null;
    this.firmwareDirState = "idle";
    this.firmwareDirError = "";
  }

  // 경고 페이지 동의 (뒤로 왔다 다시 와도 유지, 처음으로 가면 초기화)
  omdAck = $state(false);
  riskAck = $state(false);

  // ── 2단계(작업 옵션 선택) — 이전/다음 이동 시 유지, 다른 기기거나 처음으로 가면 초기화 ──
  groups: BackupGroup[] = $state([]); // 백업 항목 (항목 단위 checked)
  opts = $state<PlanOptions>({ unroot: false, relock: false, restore: true });
  get backupLive(): boolean { return REAL_STEPS.backup || this.opts.backupOnly === true; }
  get executionFlags() { return this.opts.backupOnly ? { backup: true, restore: false, fastboot: false, root: false, verify: false, efs: false } : REAL_STEPS; }
  backupPath = $state("");
  sizes: Record<string, number> | null = $state(null); // storage_sizes 실측
  sizesState = $state<LoadState>("idle");
  appClasses: AppItem[] | null = $state(null);
  appClassesState = $state<LoadState>("idle");
  settingsInfo: SettingsOverview | null = $state(null);
  settingsInfoState = $state<LoadState>("idle");
  private optionsFor: string | null = null; // 선택값/실측을 준비한 기기

  get anyBackupChecked(): boolean {
    return this.groups.some((g) => g.items.some((i) => i.checked));
  }

  /** 실행 순서 — 미리보기와 실제 실행이 같은 결과를 쓴다 */
  get plan(): PlanStep[] {
    return buildPlan(this.device, this.volteConfig, this.opts, this.anyBackupChecked);
  }

  /** 2단계 진입 시: 기기가 바뀌었을 때만 기본 선택 생성 + 실측(앱 목록 → 용량 순차) */
  ensureOptions(measure = true) {
    const d = this.device;
    if (!d) return;
    // 작업 종류(부트로더만 작업 여부)가 바뀌면 기본 선택을 다시 만든다
    const key = `${d.serial ?? d.serialMasked}|${this.bootloaderOnly ?? ""}|${!!this.opts.backupOnly}`;
    if (this.optionsFor === key) return;
    this.optionsFor = key;
    // 초기화 경로(잠긴 기기의 언락, 부트로더만 언락/리락)가 있으면 백업 기본 전체 선택 — 패치 흐름의 리락은 기본 해제
    const backupOnly = this.opts.backupOnly === true;
    const defaultOn = backupOnly || (this.hasPatchTarget && d.bootloader === "locked") || this.bootloaderOnly !== null;
    this.groups = mockBackupGroups.map((g) => ({
      ...g,
      items: g.items.map((i) => ({ ...i, checked: defaultOn })),
    }));
    this.opts = { unroot: false, relock: false, restore: defaultOn && !backupOnly, backupOnly };
    if (measure) void this.loadMeasurements(key, d.serial);
  }

  private async loadMeasurements(key: string, serial?: string) {
    // USB 직접 연결은 한 번에 한 작업만 가능 — 순차 실행
    this.appClasses = null;
    this.appClassesState = "loading";
    this.sizes = null;
    this.sizesState = "loading";
    this.settingsInfo = null;
    this.settingsInfoState = "loading";
    const apps = await api.appClasses(serial);
    if (this.optionsFor !== key) return;
    this.appClasses = apps;
    this.appClassesState = apps ? "done" : "failed";
    const info = await api.settingsOverview(serial);
    if (this.optionsFor !== key) return;
    this.settingsInfo = info;
    this.settingsInfoState = info ? "done" : "failed";
    const sizes = await api.storageSizes(serial);
    if (this.optionsFor !== key) return;
    this.sizes = sizes;
    this.sizesState = sizes ? "done" : "failed";
  }

  /** 루팅/언루팅 대상 파티션 (원본 CLI 기준 표, 미등록 기종은 null) */
  get partition(): "init_boot" | "boot" | null {
    return this.workflow.partition;
  }

  get workflow() {
    return deviceWorkflow(
      this.device?.model ?? "",
      this.volteConfig.sims.flatMap(s => s.carrier ? [s.carrier] : []),
      this.opts.relock || this.bootloaderOnly === "relock",
    );
  }

  // ── 실행 상태 ──
  steps: PlanStep[] = $state([]);
  runSteps: RunStep[] = $state([]);
  running = $state(false);
  finished = $state(false);
  awaitingNext = $state<string | null>(null);
  /** 백업·복구 중 현재 작업 표시(예: "APK 파일 [12 / 92]") — 작업이 끝나면 비운다 */
  transferStatus = $state("");
  backupOmissionNotice = $state(false);
  backupOmittedApps = $state<NonNullable<BackupSummary["omittedApps"]>>([]);
  get nextStepTitle(): string { return this.runSteps[this.cursor]?.title ?? "완료"; }
  manualCurrent: ManualPrompt | null = $state(null);
  /** 실전 백업 결과(완결 게이트·복구에 사용) — this.backupLive 전환 시에만 채워짐 */
  backupSummary: BackupSummary | null = $state(null);
  /** 실전 백업이 만든 폴더(manifest 위치) — 재시도 이어받기·복구·journal에 저장 */
  backupDir = $state("");
  /** 완료 화면 [백업 파일 삭제] 상태 */
  backupDeleteState = $state<"idle" | "deleting" | "deleted" | "failed">("idle");
  backupDeleteError = $state("");
  /** 진행 중인 실전 백업의 실행 식별값 — 취소를 이 실행에만 보낸다(이전 실행의 취소가 다음 실행에 남지 않게) */
  private backupRunId: string | undefined;
  /** 실전 루팅이 만든 패치 이미지 경로 — 언루팅(순정 재기록)·journal에 저장 */
  patchedImage = $state("");
  // 실행 중 입력값 — 언락 코드는 UI/로그에 마스킹해서만 표시
  unlockCode = $state("");
  firmwareDir = $state(""); // 수동 지정(폴백)
  imei1: string | null = $state(null); // 메모리에만 — UI는 마스킹, 로그에 남기지 않음
  imeiState = $state<LoadState>("idle");
  firmware: FirmwareResult | null = $state(null);
  firmwareState = $state<LoadState>("idle");
  firmwareError = $state("");
  /** 자동 다운로드 실패 원인 — space: 저장 공간 부족(다른 위치 선택) / download: 받기 실패(XperiFirm 폴더 지정) */
  firmwareFail = $state<"space" | "download" | null>(null);
  firmwareDest = $state(""); // 공간 부족 시 사용자가 고른 저장 위치
  usbError = $state(false);
  usbErrorCount = $state(0);
  simulateUsbError = $state(false);
  private erroredOnce = false;
  /** 실행 세대 — 중단·처음으로·새 실행·재개 때 증가. 비동기 완료 시 같은 세대인지 확인 */
  private runGen = 0;
  /** 단계 id → 실전 엔진을 시작한 세대 (재시도·재개로 세대가 바뀌면 다시 시작) */
  private readonly engineRan = new Map<string, number>();
  /** 첫 사전 검사에서 고정. 실행 중 PC 설정 변경이 다른 COM/프리셋으로 작업을 돌리지 않게 한다. */
  private efsRunConfiguration: EfsConfiguration | null = null;
  /** 아직 끝나지 않은 엔진·게이트·자동 다운로드 수 — 0이 될 때까지 [이어서]로 다시 진행하지 않는다 */
  busy = $state(0);
  /** 그중 기기 쓰기 엔진 수 — 창 닫기·PC 보호 해제를 막는다 */
  private dangerBusy = $state(0);
  private timer: ReturnType<typeof setInterval> | undefined;
  private cursor = 0;

  get macroStepIdx(): number {
    return MACRO_STEPS.findIndex((s) => s.view === this.view);
  }

  /** 2단계 [실행] 확정 — 현재 계획으로 실행 시작 */
  get journalBlocked() {return this.journalReadBlocked || this.pendingJournal!==null;}

  launch() {
    if (this.journalReadBlocked || this.pendingJournal || this.plan.length === 0) return;
    this.steps = this.plan;
    this.view = "step3";
    this.prepareRun();
    this.journalStarted = new Date().toISOString();
    this.journalSims = this.simSnapshot();
    void this.journalKeyReady()
      .then(() => this.persist(true))
      .catch(() => this.log(this.runSteps[this.cursor], "[경고] 진행 기록을 저장하지 못했습니다 — 중간에 끊기면 이어서 진행할 수 없습니다"));
    void this.startCheckedRun();
  }

  // ── 작업 진행 기록 (끊긴 작업 이어서 진행) ───────────────────
  /** 경고 페이지 다음에 보여줄, 같은 폰의 끝나지 않은 작업 */
  pendingJournal: RunJournal | null = $state(null);
  private journalKey: string | null = null;
  private journalStarted = "";
  private lastJournalSave = 0;
  private journalSims: RunJournal["sims"] = undefined;
  private readonly journalWrites = new AsyncQueue();
  private readonly guardWrites = new AsyncQueue();

  private simSnapshot(): RunJournal["sims"] {
    return this.device?.sims.map((s) => ({ slot: s.slot, carrier: s.carrier, state: s.state }));
  }

  /** 작업 시작 때와 지금의 SIM 구성이 다른지 (기록이 없으면 다르지 않은 것으로) */
  simChangedSince(j: RunJournal): boolean {
    const now = this.simSnapshot();
    if (!j.sims || !now) return false;
    return j.sims.some((a) => {
      const b = now.find((x) => x.slot === a.slot);
      return !b || b.carrier !== a.carrier || b.state !== a.state;
    });
  }
  private stopInfo: RunJournal["stop"] = null;

  /** 기기 식별 키 — 모델+시리얼의 SHA-256 앞 16바이트 (파일 이름에 시리얼을 그대로 쓰지 않음) */
  private async journalKeyReady(): Promise<string | null> {
    const d = this.device;
    const gen = this.runGen;
    if (!d?.serial) return (this.journalKey = null);
    const key = await sha256Hex(`${d.model}|${d.serial}`, 16);
    if (gen !== this.runGen || this.device?.serial !== d.serial) return null;
    this.journalKey = key;
    return key;
  }

  /** 경고 페이지 [다음] — 같은 폰의 끝나지 않은 작업이 있으면 pendingJournal에 두고 true */
  async checkJournal(): Promise<boolean> {
    const gen = this.runGen;
    const key = await this.journalKeyReady();
    if (!key) { if (this.opts.backupOnly) {this.journalReadBlocked=true;this.journalError="기기 식별 정보를 읽지 못했습니다 — 연결 상태를 확인해 주세요";return true;} return false;}
    let loaded;
    try { loaded = await api.journalLoad(key); }
    catch (error) { loaded = { ok: false as const, error: String(error) }; }
    if (gen !== this.runGen) return true;
    this.journalReadBlocked = !loaded.ok;
    this.journalMismatch = false;
    this.journalError = loaded.ok ? "" : `진행 기록을 읽지 못했습니다 — ${loaded.error}. 다시 확인해 주세요`;
    if (!loaded.ok) return true;
    const raw = loaded.value;
    if (raw === null) return false;
    const journal = decodeJournal(raw);
    if (!journal) {
      // 읽을 수 없는 기록(손상·이전 형식)은 새 실행이 덮어쓰기 전에 보관해 둔다 (백업 폴더 경로 등 복구 단서 보존)
      const archived = await this.archiveJournal(key, "discarded");
      return gen !== this.runGen || !archived;
    }
    if (journal.model !== this.device?.model || journal.serialMasked !== this.device?.serialMasked) {
      this.journalReadBlocked = true;
      this.journalMismatch = true;
      this.journalError = "진행 기록의 기기 정보가 현재 기기와 다릅니다 — 기록을 덮어쓰지 않습니다";
      return true;
    }
    this.pendingJournal = journal;
    return true;
  }

  /** 백업을 지우면 되돌릴 수 없는 데이터가 남는지 — 초기화 단계가 끝났는데 복구가 실제로 끝나지 않은 경우.
   *  초기화가 없었으면 폰 데이터가 그대로라 해당 없음. 백업은 실전·복구는 시뮬레이션이면 실제로 복원된 것이 아니다 */
  get backupStillNeeded(): boolean {
    const wiped = this.runSteps.some((r) => r.status === "done" && this.steps.find((s) => s.id === r.id)?.wipe);
    if (!wiped) return false;
    const restore = this.runSteps.find((r) => r.id === "restore");
    return !(restore?.status === "done" && (REAL_STEPS.restore || !this.backupLive));
  }

  /** 완료 화면 [백업 파일 삭제] — 사용자 확인 후에만. 이 실행이 만든 백업 폴더(backupDir)만 지운다
   *  (사용자가 고른 상위 저장 위치 backupPath는 절대 넘기지 않는다). 목 모드는 표시만 바뀐다 */
  async deleteBackup() {
    if (this.backupDeleteState === "deleting" || this.backupDeleteState === "deleted") return;
    const dir = this.backupDir;
    if (this.backupLive && !dir) {
      this.backupDeleteState = "failed";
      this.backupDeleteError = "삭제할 백업 폴더 정보가 없습니다";
      return;
    }
    this.backupDeleteState = "deleting";
    this.backupDeleteError = "";
    const gen = this.runGen;
    const r = await api.backupDelete(dir, this.opts.backupOnly);
    // 삭제 중 [처음으로]를 누르면 결과를 다음 세션에 남기지 않는다
    if (gen !== this.runGen) return;
    if (r.ok) {
      this.backupDeleteState = "deleted";
    } else {
      this.backupDeleteState = "failed";
      this.backupDeleteError = r.error;
    }
  }

  /** [새로 시작] — 이전 기록은 discarded로 보관하고 1단계부터 */
  async discardJournal() {
    const gen = this.runGen;
    const key = this.journalKey;
    if (key && !(await this.archiveJournal(key, "discarded"))) return;
    if (gen !== this.runGen) return;
    this.pendingJournal = null;
    this.journalMismatch = false;
    this.backupDir = "";
    this.patchedImage = "";
    this.backupSummary = null;
    this.view = this.opts.backupOnly ? "step2" : "step1";
  }

  /** [이어서 진행] — 선택했던 옵션·진행 상황을 되살리고 실행 화면으로 (자동 시작하지 않음) */
  resumeJournal() {
    const j = this.pendingJournal;
    if (!j) return;
    if (!j.opts.backupOnly && (!this.omdAck || !this.riskAck)) {
      this.view = "warning";
      this.journalError = "이전 작업을 이어서 진행하려면 위험 안내의 두 항목을 먼저 확인해 주세요";
      this.pendingJournal = null;
      return;
    }
    this.resetExecution();
    this.volteConfig = { ...defaultVolteConfig(), ...j.config };
    this.ensureOptions(false);
    for (const g of this.groups) for (const i of g.items) i.checked = j.backupItems.includes(i.id);
    this.opts = { ...j.opts };
    this.backupPath = j.backupPath;
    this.backupDir = j.backupDir ?? "";
    this.patchedImage = j.patchedImage ?? "";
    this.backupSummary = null; // 이어서 진행 시 백업 결과는 단계 재검증으로 다시 채운다
    this.backupOmittedApps = j.backupOmissions?.apps ?? [];
    this.backupOmissionNotice = j.backupOmissions?.pending ?? false;
    this.firmware = j.firmware;
    this.firmwareState = j.firmware ? "done" : "idle";
    this.firmwareDir = j.firmwareDir;
    this.imsUnverified = j.imsUnverified ?? false;
    this.imsVerified = j.imsVerified ?? false;
    this.callVerified = j.callVerified ?? false;
    this.communicationBefore = j.communication?.before ?? null;
    this.communicationLatest = j.communication?.latest ?? null;
    this.callChecks = j.communication?.calls ?? [];
    // 직접 지정한 폴더는 그사이 바뀌었을 수 있으므로 다시 검사
    this.firmwareDirInfo = null;
    if (j.firmwareDir) void this.setFirmwareDir(j.firmwareDir);
    this.steps = j.steps;
    // 끊긴 단계는 처음부터 다시 — 실제 구현은 단계 시작 시 기기 상태를 먼저 확인해 이미 끝난 작업은 건너뛴다
    const runSteps: RunStep[] = j.runSteps.map((st) =>
      st.status === "done" || st.status === "skipped"
        ? st
        : {
            ...st,
            status: "pending",
            // 끝낸 세부 작업까지는 진행된 것으로 — 실제 구현은 기기 상태로 한 번 더 확인 후 건너뜀
            progress: st.sub && st.sub.list.length > 0 ? st.sub.done / st.sub.list.length : 0,
            manualDone: 0,
          },
    );
    // 언락 코드는 기록하지 않으므로, 언락이 아직이면 사전 준비(언락 코드 입력)를 다시
    const prep = runSteps.find((st) => st.id === "prep");
    const unlockLeft = runSteps.some((st) => st.id === "unlock" && st.status !== "done");
    if (prep && unlockLeft && j.steps.find((st) => st.id === "prep")?.manual?.includes("unlock-code")) {
      prep.status = "pending";
      prep.progress = 0;
      prep.manualDone = 0;
    }
    // SIM을 바꿨으면 이전 통신 확인 결과를 재사용하지 않는다
    const simChanged = this.simChangedSince(j);
    const lastCommunication = j.communication?.latest;
    const firmwareChanged = lastCommunication && this.device && ["model", "firmware", "fingerprint", "baseband"].some(key => {
      const field = key as "model" | "firmware" | "fingerprint" | "baseband";
      return this.device?.[field] && this.device[field] !== lastCommunication[field];
    });
    const legacyCommunication = j.imsVerified === undefined || j.callVerified === undefined || j.communication === undefined;
    if (simChanged || firmwareChanged || legacyCommunication) {
      this.imsVerified = false;
      this.callVerified = false;
      this.callChecks = [];
      this.communicationLatest = null;
      for (const st of runSteps) {
        if ((st.id === "final-verify" || ((simChanged || firmwareChanged) && st.id === "comm-check")) && st.status === "done") {
          st.status = "pending";
          st.progress = 0;
          st.manualDone = 0;
          st.communicationSkipped = false;
          this.log(st, simChanged || firmwareChanged ? "[재개] SIM·펌웨어 구성이 달라 통신 확인을 다시 진행합니다 — 패치 대상 선택은 유지합니다" : "[재개] 이전 기록에 최종 통신 확인 결과가 없어 다시 확인합니다");
        }
      }
    }
    this.journalSims = j.sims;
    const first = runSteps.findIndex((st) => st.status !== "done" && st.status !== "skipped");
    if (first >= 0) {
      const sub = runSteps[first].sub;
      this.log(
        runSteps[first],
        sub && sub.done > 0
          ? `[재개] 이전 진행 기록을 불러왔습니다 — '${sub.list[sub.done - 1]}'까지 끝났으므로 '${sub.list[sub.done] ?? "마무리"}'부터 진행합니다`
          : "[재개] 이전 진행 기록을 불러왔습니다 — 이 단계부터 다시 진행합니다",
      );
    }
    this.runSteps = runSteps;
    for (const step of runSteps) if (step.status === "pending") this.clearRuntimeManuals(step.id);
    this.cursor = first >= 0 ? first : runSteps.length;
    const previous = runSteps[this.cursor - 1];
    this.awaitingNext = first > 0 && previous && ["done", "skipped"].includes(previous.status) ? previous.id : null;
    this.journalStarted = j.startedAt;
    this.pendingJournal = null;
    this.view = "step3";
    void this.persist(true);
  }

  /** 진행 기록 저장 — 상태 변화 시 즉시, 진행률·로그는 2초 간격 */
  private persist(force = false): Promise<boolean> {
    const key = this.journalKey;
    const d = this.device;
    if (!key || !d || this.runSteps.length === 0 || this.journalReadBlocked) return Promise.resolve(false);
    const now = Date.now();
    if (!force && now - this.lastJournalSave < 2000) return Promise.resolve(false);
    this.lastJournalSave = now;
    const j: RunJournal = {
      version: 1,
      model: d.model,
      productName: d.productName,
      serialMasked: d.serialMasked,
      startedAt: this.journalStarted || new Date().toISOString(),
      updatedAt: new Date().toISOString(),
      config: $state.snapshot(this.volteConfig),
      opts: { ...this.opts },
      backupPath: this.backupPath,
      backupDir: this.backupDir || undefined,
      patchedImage: this.patchedImage || undefined,
      backupItems: this.groups.flatMap((g) => g.items.filter((i) => i.checked).map((i) => i.id)),
      steps: $state.snapshot(this.steps),
      // 로그 전체를 깊은 복사하지 않도록 최근 300줄만 잘라 붙인다
      runSteps: this.runSteps.map(({ logs, ...rest }) => ({ ...$state.snapshot(rest), logs: logs.slice(-300) })),
      cursor: this.cursor,
      awaitingNext: this.awaitingNext,
      backupOmissions: { apps: $state.snapshot(this.backupOmittedApps), pending: this.backupOmissionNotice },
      firmware: this.firmware ? { ...this.firmware } : null,
      firmwareDir: this.firmwareDir,
      imsUnverified: this.imsUnverified,
      imsVerified: this.imsVerified,
      callVerified: this.callVerified,
      communication: { before: this.communicationBefore ? $state.snapshot(this.communicationBefore) : null, latest: this.communicationLatest ? $state.snapshot(this.communicationLatest) : null, calls: $state.snapshot(this.callChecks) },
      sims: this.journalSims,
      stop: this.stopInfo,
    };
    const data = JSON.stringify(j);
    const completed = this.finished;
    const gen = this.runGen;
    return this.journalWrites.push(async () => {
      let saved = false;
      try {
        saved = await api.journalSave(key, data);
        saved = saved && (!completed || await api.journalArchive(key, "done"));
      } catch { saved = false; }
      if (gen === this.runGen && key === this.journalKey) {
        this.journalError = saved ? "" : "진행 기록을 저장하지 못했습니다 — 앱을 종료하면 현재 단계부터 이어서 진행할 수 없을 수 있습니다";
      }
      // Post-completion checks update the completed record, never create a resumable run.
      return saved;
    });
  }

  journalError = $state("");
  journalMismatch = $state(false);
  private journalReadBlocked = $state(false);
  get simulationControlsVisible(): boolean { return SIMULATED_RUN && !hasLiveActions(this.executionFlags); }

  private async archiveJournal(key: string, tag: "discarded" | "done"): Promise<boolean> {
    const gen = this.runGen;
    let archived = false;
    try { archived = await this.journalWrites.push(() => api.journalArchive(key, tag)); } catch { /* preserve */ }
    if (gen === this.runGen && key === this.journalKey) {
      this.journalReadBlocked = !archived;
      this.journalError = archived ? "" : "기존 진행 기록을 보관하지 못했습니다 — 새 기록으로 덮어쓰지 않습니다. 다시 시도해 주세요";
    }
    return archived;
  }

  // ── 작업 중 PC 보호 (절전·Windows 종료 방지) — 실행 중·폰 확인 대기 중에는 켜고, 끝나거나 멈추면 끈다
  private guardOn = false;
  private setGuard(on: boolean) {
    if (this.guardOn === on) return;
    this.guardOn = on;
    void this.guardWrites.push(() => api.runGuard(on, "Xperia VoLTE 작업 진행 중 — 끝날 때까지 PC를 끄지 마세요")).then((ok) => {
      if (ok || !on || !this.guardOn) return;
      // 켜기 실패 — 상태를 되돌려 다음 시작 때 다시 시도하고, 사용자에게 알림
      this.guardOn = false;
      this.log(this.runSteps[this.cursor], "[경고] PC 절전·종료 방지를 켜지 못했습니다 — 작업이 끝날 때까지 PC가 잠들거나 꺼지지 않게 해 주세요");
    });
  }

  /** Windows 종료 요청 (guard.rs 이벤트) — query: 종료 보류 중 기록 저장 / end: 그래도 종료됨 */
  onSessionEnd(kind: string) {
    if (!this.runUnfinished) return;
    const cur = this.runSteps[this.cursor];
    if (kind === "end" && cur && !this.stopInfo) {
      this.stopInfo = { stepId: cur.id, stepTitle: cur.title, reason: "Windows 종료로 프로그램이 종료되었습니다", at: new Date().toISOString() };
      this.log(cur, `[종료] ${this.stopInfo.reason}`);
    } else if (kind === "query" && cur) {
      this.log(cur, "[경고] Windows 종료 요청을 보류했습니다 — 작업이 끝날 때까지 PC를 끄지 마세요");
    }
    void this.persist(true);
  }

  /** 끝나지 않은 실행이 있는지 — 창을 닫을 때 확인 */
  get runUnfinished(): boolean {
    return this.view === "step3" && this.runSteps.length > 0 && !this.finished;
  }

  /** 지금 진행 중인 단계가 되돌리기 어려운 작업인지 (창 닫기 경고용) — 중단 후에도 기기 쓰기가 끝나지 않았으면 포함 */
  get runInDanger(): boolean {
    if (this.dangerBusy > 0) return true;
    const cur = this.runSteps[this.cursor];
    const step = cur && cur.status === "running" ? this.steps.find((s) => s.id === cur.id) : undefined;
    // 확인 모달과 같은 기준(초기화·기록·모뎀 설정 수정) + 기존 danger 단계
    return !!step && (step.risk === "danger" || stepHazard(step) !== null);
  }

  /** 창 닫기 — 멈춘 사유를 남기고 기록을 확실히 저장 (사용자가 이미 중단한 경우 그 사유 유지) */
  async closeForExit(): Promise<boolean> {
    if (this.runInDanger) return false;
    const cur = this.runSteps[this.cursor];
    const wasWaiting = cur?.status === "manual-wait";
    this.stopRun(false);
    if (!this.stopInfo && cur) {
      this.stopInfo = {
        stepId: cur.id,
        stepTitle: cur.title,
        reason: wasWaiting ? "폰 확인을 기다리는 중에 프로그램을 종료했습니다" : "작업 중에 프로그램을 종료했습니다",
        at: new Date().toISOString(),
      };
      this.log(cur, `[종료] ${this.stopInfo.reason}`);
    }
    try {
      return await this.persist(true);
    } finally {
      this.setGuard(false);
    }
  }

  /** 명시적으로 멈춘 사유를 기록 — 기기 쓰기가 아직 진행 중이면 PC 보호는 그 작업이 끝날 때 해제한다 */
  private markStop(reason: string) {
    const cur = this.runSteps[this.cursor];
    this.stopInfo = cur ? { stepId: cur.id, stepTitle: cur.title, reason, at: new Date().toISOString() } : null;
    if (this.dangerBusy === 0) this.setGuard(false);
    void this.persist(true);
  }

  /** 단계 로그 추가 — 단계별 상한을 넘으면 오래된 줄부터 버린다 */
  private log(step: RunStep | undefined, line: string) {
    if (!step) return;
    step.logs.push(line);
    if (step.logs.length > STEP_LOG_LIMIT) step.logs.splice(0, step.logs.length - STEP_LOG_LIMIT);
  }

  // ── 실행 시뮬레이션 ───────────────────
  prepareRun() {
    this.resetExecution();
    this.patchedImage = "";
    this.runSteps = this.steps
      .filter((s) => s.enabled)
      .map((s) => ({ id: s.id, title: s.title, status: "pending", progress: 0, logs: [], manualDone: 0 }));
  }

  begin() {
    if (this.running || this.usbError || this.stepError || this.awaitingNext || this.backupOmissionNotice) return;
    if (this.firmwareDirState === "loading") return;
    if (this.journalReadBlocked) return this.failStep(this.journalError);
    const problem = executionPlanProblem(this.runSteps.map(s => s.id), this.executionFlags);
    if (problem) return this.failStep(problem);
    this.running = true;
    this.stopInfo = null;
    this.setGuard(true);
    this.timer = setInterval(() => this.tick(), 140);
  }

  /** [실행]/[이어서] 버튼 — 이전 엔진·게이트·다운로드가 끝나기 전에는 다시 진행하지 않는다
   *  (엔진 내부의 진행은 begin()을 직접 부른다) */
  resumeRun() {
    if (this.awaitingNext) return;
    if (!this.readyToResume()) return;
    void this.startCheckedRun();
  }

  nextStep() {
    if (!this.awaitingNext || this.backupOmissionNotice || this.stepError || this.usbError || !this.readyToResume()) return;
    this.awaitingNext = null;
    void this.persist(true);
    void this.startCheckedRun();
  }

  acknowledgeBackupOmissions() {
    this.backupOmissionNotice = false;
    void this.persist(true);
  }

  private async startCheckedRun() {
    const gen = this.runGen;
    const planProblem = executionPlanProblem(this.runSteps.map(s => s.id), this.executionFlags);
    if (planProblem) return this.failStep(planProblem);
    if (hasLiveActions(this.executionFlags)) {
      const capabilities = await this.track(api.engineCapabilities());
      if (gen !== this.runGen) return;
      if (!capabilities.ok) return this.failStep(`빌드 기능 확인 실패: ${capabilities.error}`);
      const problem = buildFeatureProblem(this.runSteps.map(s => s.id), capabilities.value);
      if (problem) return this.failStep(problem);
    }
    if (gen === this.runGen) this.begin();
  }

  private readyToResume(): boolean {
    if (this.firmwareDirState === "loading") return false;
    if (this.busy > 0) {
      this.log(this.runSteps[this.cursor], "[대기] 이전 작업이 아직 끝나지 않았습니다 — 끝난 뒤 다시 눌러 주세요");
      return false;
    }
    return true;
  }

  pause() {
    this.running = false;
    if (this.timer) clearInterval(this.timer);
    this.timer = undefined;
  }

  /** 진행 중 작업 수를 세며 기다린다 — 끝나면 busy에서 뺀다 */
  private track<T>(work: Promise<T>): Promise<T> {
    this.busy++;
    return work.finally(() => this.busy--);
  }

  private dispatchEngine(work: () => Promise<void>, danger = true) {
    const gen = this.runGen;
    this.busy++;
    if (danger) this.dangerBusy++;
    void work()
      .catch((error) => {
        if (gen === this.runGen) this.failStep(String(error));
      })
      .finally(() => {
        this.busy--;
        if (!danger) return;
        this.dangerBusy--;
        // 중단·실패·처음으로 때 미뤄 둔 PC 보호 해제 — 진행 중이거나 폰 확인 대기 중이면 계속 보호
        if (this.dangerBusy === 0 && !this.running && this.runSteps[this.cursor]?.status !== "manual-wait") this.setGuard(false);
      });
  }

  /** 실전 엔진 단계 시작·재진입 — 같은 세대에 두 번 시작하지 않는다 */
  private runEngineStep(cur: RunStep, engine: EngineStep) {
    if (cur.status !== "running") return;
    this.pause();
    if (this.engineRan.get(cur.id) !== this.runGen) {
      this.engineRan.set(cur.id, this.runGen);
      this.dispatchEngine(engine.start, engine.danger);
    } else {
      engine.resume?.();
    }
  }

  /** 단계 id → 실전 엔진 (해당 REAL_STEPS가 꺼져 있으면 null → 시뮬레이션) */
  private engineFor(cur: RunStep): EngineStep | null {
    if (!liveStepEnabled(cur.id, this.executionFlags)) return null;
    switch (cur.id) {
      case "backup":
        // smsie 수동 완료 후 재진입하면 완결 판정으로 마무리. 기기 읽기만 하므로 창 닫기를 막지 않는다
        return this.backupLive ? { start: () => this.runRealBackup(cur), resume: () => this.finishRealBackup(cur), danger: false } : null;
      case "restore":
        // smsie 수동 복원 완료 후 재진입 — 역할 원복·마무리
        return REAL_STEPS.restore
          ? { start: () => this.runRealRestore(cur), resume: () => this.dispatchEngine(() => this.finishRealRestore(cur)), danger: true }
          : null;
      case "fw-verify":
        return REAL_STEPS.verify ? { start: () => this.runRealFwVerify(cur), danger: false } : null;
      case "final-verify":
        // 재부팅·재연결 후 VoLTE 등록 확인(수동 안내) → 재진입하면 마무리
        return REAL_STEPS.verify || REAL_STEPS.efs
          ? { start: () => this.runRealFinalVerify(cur), resume: () => this.finishRealFinalVerify(cur), danger: false }
          : null;
      case "unlock":
        return REAL_STEPS.fastboot ? { start: () => this.runRealUnlock(cur), danger: true } : null;
      case "relock":
        return REAL_STEPS.fastboot ? { start: () => this.runRealRelock(cur), danger: true } : null;
      case "unroot":
        // 순정 재기록(root_reboot + fastboot_flash 조합) — 두 엔진 모두 켜져야 실행
        return REAL_STEPS.root && REAL_STEPS.fastboot ? { start: () => this.runRealUnroot(cur), danger: true } : null;
      case "root":
        // 패치·기록·설치 → su 승인(수동) 후 재진입하면 최종 확인
        return REAL_STEPS.root
          ? { start: () => this.runRealRoot(cur), resume: () => this.dispatchEngine(() => this.finishRealRoot(cur)), danger: true }
          : null;
      case "volte-props":
        return REAL_STEPS.efs ? { start: () => this.runRealVolteProps(cur), danger: true } : null;
      case "comm-check":
        return REAL_STEPS.efs ? { start: () => this.runRealCommCheck(cur), danger: false } : null;
      case "efs-input":
        return REAL_STEPS.efs ? { start: () => this.runRealEfsInputs(cur), danger: false } : null;
      case "efs-preflight":
      case "efs":
      case "verify":
        return REAL_STEPS.efs ? { start: () => this.runRealNativeEfs(cur), danger: true } : null;
      default:
        return null;
    }
  }

  private tick() {
    if (this.awaitingNext) return this.pause();
    while (this.runSteps[this.cursor] && ["done", "skipped"].includes(this.runSteps[this.cursor].status)) this.cursor++;
    const cur = this.runSteps[this.cursor];
    if (!cur) return this.complete();
    if (cur.status === "pending") {
      cur.status = "running";
      this.log(cur, `[시작] ${cur.title}`);
      const list = this.subtasksFor(cur.id);
      if (list.length > 0 && !cur.sub) cur.sub = { list, done: 0 };
      // 완결 게이트 (§3-3) — 파괴 단계(언락/리락) 직전, 실전 모드에서만 백업 완결을 강제한다
      if ((cur.id === "unlock" || cur.id === "relock") && (this.backupLive || REAL_STEPS.fastboot)) {
        this.pause();
        void this.track(this.enforceBackupGate(cur));
        return;
      }
      if (cur.id === "efs") {
        for (const t of this.volteConfig.sims.filter((x) => x.carrier !== null)) {
          const p = efsPreset(t.carrier!, t.slot);
          this.log(
            cur,
            p
              ? `[프리셋] SIM${t.slot} ${t.carrier} · ${EFS_PRESET_VERSION} ${EFS_PRESET_MODE} · 파일 ${p.files}개 · sha256 ${p.sha256.slice(0, 12)}…`
              : `[프리셋] SIM${t.slot} ${t.carrier} — 번들 프리셋을 찾을 수 없습니다`,
          );
        }
      }
      void this.persist(true);
    }
    // 시뮬레이션 스위치는 시뮬레이션으로 도는 단계에만 적용 — 실전 백업을 가짜 오류로 멈추지 않는다
    if (SIMULATED_RUN && !REAL_STEPS.efs && this.simulateEfsFail && !this.efsFailedOnce && cur.id === "efs" && cur.progress > 0.5) {
      this.efsFailedOnce = true;
      this.failStep("EFS 업로드 실패 — SIM1 2차 업로드, 도구 종료 코드 1 (시뮬레이션)");
      return;
    }
    if (SIMULATED_RUN && this.simulateUsbError && !this.erroredOnce && ((cur.id === "backup" && !this.backupLive) || (cur.id === "efs" && !REAL_STEPS.efs))) {
      this.erroredOnce = true;
      this.usbErrorCount++;
      this.usbError = true;
      this.pause();
      this.log(cur, "[오류] DIAG 전송 타임아웃 — USB 연결이 불안정합니다 (재시도 카운트 3/3)");
      this.markStop("USB 연결 오류 — DIAG 전송 타임아웃");
      return;
    }
    // 혼합 모드에서 실전 기록 뒤 시뮬레이션 완료로 넘어갈 수 없도록 먼저 차단한다.
    if ((cur.id === "root" || cur.id === "unroot") && (REAL_STEPS.root || REAL_STEPS.fastboot) && !this.realBootFlowReady()) return;
    // 실전 최종 확인은 재부팅을 먼저 하고, 그 뒤에 VoLTE 등록 확인 안내를 연다(계획의 수동 안내보다 엔진이 먼저)
    if (cur.id === "final-verify" && this.engineRan.get(cur.id) !== this.runGen) {
      const first = this.engineFor(cur);
      if (first) return this.runEngineStep(cur, first);
    }
    const step = this.steps.find((s) => s.id === cur.id);
    const manuals = step?.manual ?? [];
    if (cur.manualDone < manuals.length) {
      const id = manuals[cur.manualDone];
      cur.status = "manual-wait";
      this.pause();
      void this.openManual(cur, id);
      return;
    }
    // 실전 엔진 단계 — 시뮬레이션 대신 백엔드 엔진이 상태를 바꾼다 (REAL_STEPS 전환 시)
    const engine = this.engineFor(cur);
    if (engine) {
      this.runEngineStep(cur, engine);
      return;
    }
    cur.progress = Math.min(1, cur.progress + 0.04 + Math.random() * 0.05);
    if (Math.random() < 0.35) this.log(cur, this.mockLog(cur.id, cur.progress));
    if (cur.sub && this.markSub(cur, Math.floor(cur.progress * cur.sub.list.length + 1e-9))) void this.persist(true);
    if (cur.progress >= 1) {
      return this.stepDone(cur);
    }
    void this.persist();
  }

  /** 단계별 세부 작업 — 끝날 때마다 체크포인트로 기록 (실제 구현도 같은 단위로 기록·재개) */
  private subtasksFor(id: string): string[] {
    const items = this.groups.flatMap((g) => g.items.filter((i) => i.checked).map((i) => i.label));
    switch (id) {
      case "backup": return items.map((l) => `${l} 백업`);
      case "restore": return items.map((l) => `${l} 복원`);
      case "root": return ["Magisk 받기", "부트 이미지·패치 도구 전송", "Magisk 패치", "패치 결과 확인", "패치 이미지 기록", "Magisk 앱 설치"];
      case "efs-preflight": return ["DIAG 포트 전환", "EFS 프로토콜 초기화", "응답 확인"];
      case "efs":
        return this.volteConfig.sims
          .filter((s) => s.carrier !== null)
          .flatMap((s) => [`SIM${s.slot} 프로파일 1차 업로드`, `SIM${s.slot} 프로파일 2차 업로드`]);
      case "verify":
        return this.volteConfig.sims.filter((s) => s.carrier !== null).map((s) => `SIM${s.slot} 전수 리드백·해시 비교`);
      case "volte-props": return ["VoLTE 설정", "영상통화 설정", "Wi-Fi 통화 설정", "재부팅"];
      case "fw-verify": return ["재부팅", "버전 확인", "지문 확인"];
      case "final-verify": return ["재부팅", "네트워크 등록", "VoLTE 활성 확인"];
      default: return [];
    }
  }

  private mockLog(id: string, p: number): string {
    const pct = Math.round(p * 100);
    switch (id) {
      case "backup": return `파일 복사 중… ${pct}%`;
      case "unlock": return `잠금 해제 중… ${pct}%`;
      case "root": return ["Magisk 최신 버전 받는 중…", "부트 이미지·패치 도구 전송", "Magisk 패치 실행(boot_patch.sh)", "패치 결과 확인(ANDROID!·크기)", "fastboot로 패치 이미지 기록", "Magisk 앱 설치"][Math.floor(p * 6) % 6];
      case "efs-preflight": return ["DIAG 포트 전환", "EFS 프로토콜 초기화", "응답 확인"][Math.floor(p * 3) % 3];
      case "efs": return `프로파일 적용 중… (${Math.floor(p * 46)}/46 파일)`;
      case "verify": return `무결성 검증 중… ${pct}%`;
      case "fw-download": return `펌웨어 다운로드 중… ${pct}%`;
      case "fw-flash": return `펌웨어 기록 중… ${pct}%`;
      case "fw-verify": return ["재부팅 대기 중…", "버전 확인", "지문 일치 확인"][Math.floor(p * 3) % 3];
      case "volte-props": return ["VoLTE 설정 적용", "영상통화 설정 적용", "Wi-Fi 통화 설정 적용", "재부팅 중…"][Math.floor(p * 4) % 4];
      case "unroot": return `시스템 복원 중… ${pct}%`;
      case "relock": return `잠금 중… ${pct}%`;
      case "final-verify": return ["재부팅 대기 중…", "네트워크 등록 대기 중…", "VoLTE 활성 확인됨"][Math.floor(p * 3) % 3];
      case "restore": return `데이터 복원 중… ${pct}%`;
      default: return `진행 ${pct}%`;
    }
  }

  /** USB 디버깅 연결·허용 상태 — 같은 기기가 adb "device" 상태로 보이면 충족 */
  private async usbDebugReady(): Promise<boolean> {
    const list = await api.deviceList();
    return !!list?.some((d) => d.state === "device" && d.serial === this.device?.serial);
  }

  /** USB 모드 감지 (fastboot / flashmode) — 장치 디스크립터만 읽음 */
  private async usbModeIs(mode: string): Promise<boolean> {
    const list = await api.usbModes();
    // 여러 대가 연결돼 있으면 어느 폰의 모드인지 구분할 수 없으므로 통과시키지 않는다
    return !!list && list.length === 1 && list[0].mode === mode;
  }

  /** 연결된 Sony USB 장치 수 (모드 확인 실패 사유 안내용) */
  private async sonyUsbCount(): Promise<number> {
    return (await api.usbModes())?.length ?? 0;
  }

  /** VoLTE 패치 대상 슬롯이 모두 IMS 음성 등록(on)인지 — 최종 확인 자동 판정 */
  private async imsReady(): Promise<boolean> {
    return this.refreshCommunication("latest");
  }

  communicationBefore: CommunicationSnapshot | null = $state(null);
  communicationLatest: CommunicationSnapshot | null = $state(null);
  communicationLoading = $state(false);
  communicationError = $state("");
  private communicationReq = 0;

  /** Read-only observation. It never edits target selection or authorizes a device write. */
  private communicationPending: { key: string; result: Promise<boolean> } | null = null;
  async refreshCommunication(phase: "before" | "latest" = "latest"): Promise<boolean> {
    const key = JSON.stringify([phase, this.runGen, this.device?.serial, this.volteConfig.sims, this.manualCurrent?.id, this.watchGeneration]);
    if (this.communicationPending?.key === key) return this.communicationPending.result;
    const result: Promise<boolean> = this.readCommunication(phase).finally(() => {
      if (this.communicationPending?.result === result) this.communicationPending = null;
    });
    this.communicationPending = { key, result };
    return result;
  }
  private async readCommunication(phase: "before" | "latest"): Promise<boolean> {
    const gen = this.runGen;
    const req = ++this.communicationReq;
    const selected = this.device;
    const manual = this.manualCurrent;
    const watch = this.watchGeneration;
    const selection = JSON.stringify(this.volteConfig.sims);
    const presets: CommunicationSnapshot["presets"] = this.volteConfig.sims.flatMap(s => {
      const preset = s.carrier ? efsPreset(s.carrier, s.slot) : null;
      return s.carrier && preset ? [{ slot: s.slot, carrier: s.carrier, version: EFS_PRESET_VERSION, sha256: preset.sha256 }] : [];
    });
    this.communicationLoading = true;
    let list: DeviceStatus[] | null = null;
    try { list = await api.deviceList(); } catch { /* report a read error below, never mock success */ }
    const leftManual = manual && (manual.id === "ims-check" || manual.id === "ims-precheck") && (this.manualCurrent?.id !== manual.id || watch !== this.watchGeneration);
    if (gen !== this.runGen || req !== this.communicationReq || leftManual || this.device?.serial !== selected?.serial || selection !== JSON.stringify(this.volteConfig.sims)) {
      if (req === this.communicationReq) this.communicationLoading = false;
      return false;
    }
    const d = selected ? list?.find(x => x.state === "device" && x.serial === selected.serial) : undefined;
    this.imsSims = d?.sims ?? [];
    this.communicationError = list === null ? "기기 상태 조회 실패 — 다시 확인해 주세요" : !d ? "선택한 기기가 연결되지 않았습니다" : "";
    const metadata = d ?? selected;
    const snapshot: CommunicationSnapshot = {
      checkedAt: new Date().toISOString(), outcome: list === null ? "query-failed" : d ? "observed" : "disconnected",
      model: metadata?.model ?? "", firmware: metadata?.firmware ?? "", fingerprint: metadata?.fingerprint ?? "",
      android: metadata?.android ?? "", baseband: metadata?.baseband ?? "", presets,
      sims: this.imsSims.map(s => ({ slot: s.slot, type: s.type, carrier: s.carrier, state: s.state, volte: s.volte, ...(s.ims ? { ims: { ...s.ims } } : {}) })),
    };
    if (phase === "before") this.communicationBefore = snapshot;
    else {
      const old = this.communicationLatest;
      const changed = old && (old.fingerprint !== snapshot.fingerprint || old.baseband !== snapshot.baseband || JSON.stringify(old.presets) !== JSON.stringify(snapshot.presets) || JSON.stringify(old.sims.map(s => [s.slot, s.carrier, s.state])) !== JSON.stringify(snapshot.sims.map(s => [s.slot, s.carrier, s.state])));
      this.communicationLatest = snapshot;
      // A fresh failed or changed observation must not leave a green completion badge.
      if (changed) { this.callChecks = []; this.callVerified = false; }
      if (!this.imsRegistered) this.imsVerified = false;
      if (this.finished) {
        this.imsVerified = this.imsRegistered;
        this.imsUnverified = !this.imsVerified;
        this.callVerified = this.callAck;
      }
    }
    this.communicationLoading = false;
    if (this.finished) void this.persist(true);
    return this.imsRegistered;
  }

  /** 최종 확인 중 표시할 슬롯별 VoLTE 상태 */
  imsSims: SimInfo[] = $state([]);
  /** 표시용 상태. 대상은 감지한 통신사가 아니라 사용자의 슬롯 선택이다. */
  get imsRegistered(): boolean {
    const slots = this.communicationSlots;
    return slots.length > 0 && slots.every(slot => cellularReady(this.imsSims.find(s => s.slot === slot)));
  }

  get communicationSlots(): (1 | 2)[] {
    const selected = this.volteConfig.sims.filter(s => s.carrier !== null).map(s => s.slot);
    return selected.length ? selected : (this.imsSims.length ? this.imsSims : this.device?.sims ?? []).filter(s => s.carrier).map(s => s.slot);
  }

  callChecks: CallCheck[] = $state([]);
  get callAck(): boolean { return callsComplete(this.callChecks, this.communicationSlots); }
  setCallCheck(slot: 1 | 2, item: CallItem, checked: boolean) {
    if (!this.communicationSlots.includes(slot)) return;
    const row = this.callChecks.find(c => c.slot === slot) ?? newCallCheck(slot);
    this.callChecks = [...this.callChecks.filter(c => c.slot !== slot), { ...row, [item]: checked }].toSorted((a, b) => a.slot - b.slot);
    if (this.finished) this.callVerified = this.callAck;
    if (this.finished) void this.persist(true);
  }

  /** 자동 감지 중인 항목 설명 (모달에 표시) */
  manualWatching = $state("");
  private watchTimer: ReturnType<typeof setInterval> | undefined;
  private watchGeneration = 0;
  private watchInFlight: Promise<boolean> | null = null;
  /** 현재 manualCheckError를 감지(probe)가 썼는지 — 다른 곳이 오류를 쓰면 setter가 해제한다(같은 문구여도 구분) */
  private probeOwnsCheckError = false;

  private reportSmsieProbe(result: { ok: true; value: boolean } | { ok: false; error: string }): boolean {
    if (!result.ok) {
      if (!this.manualCheckErrorText || this.probeOwnsCheckError) {
        this.manualCheckErrorText = result.error;
        this.probeOwnsCheckError = true;
      }
      this.smsieFilesReady = false;
      return false;
    }
    if (this.probeOwnsCheckError) {
      this.manualCheckErrorText = "";
      this.probeOwnsCheckError = false;
    }
    this.smsieFilesReady = result.value;
    return result.value;
  }

  /** 자동 감지: 조건이 충족될 때까지 주기적으로 확인 → 충족되면 자동 진행 (수동 [완료]도 가능) */
  private watchManual(cur: RunStep, id: ManualId, label: string, check: () => Promise<boolean>, everyMs: number, autoAdvance = true) {
    this.stopWatch();
    this.manualWatching = label;
    const watch = this.watchGeneration;
    const gen = this.runGen;
    let busy = false;
    const poll = async () => {
      if (watch !== this.watchGeneration) return;
      if (this.manualCurrent?.id !== id || gen !== this.runGen) return this.stopWatch();
      if (busy || this.manualChecking || (id === "smsie-export" && this.manualSetupState !== "done")) return;
      busy = true;
      let flight: Promise<boolean> | null = null;
      try {
        flight = check();
        this.watchInFlight = flight;
        const ok = await flight;
        if (watch !== this.watchGeneration || gen !== this.runGen || this.manualCurrent?.id !== id) return; // 그사이 중단·진행됨
        if (ok && autoAdvance) {
          this.stopWatch();
          this.log(cur, `[감지] ${label} — 자동으로 진행합니다`);
          this.ackManual();
        }
      } catch {
        if (watch === this.watchGeneration && gen === this.runGen && this.manualCurrent?.id === id) this.manualCheckError = "기기 확인에 실패했습니다 — 연결 상태를 확인하고 다시 시도해 주세요";
      } finally {
        busy = false;
        if (this.watchInFlight === flight) this.watchInFlight = null;
      }
    };
    this.watchTimer = setInterval(poll, everyMs);
    if (!autoAdvance) void poll();
  }

  private stopWatch() {
    this.watchGeneration++;
    if (this.watchTimer) clearInterval(this.watchTimer);
    this.watchTimer = undefined;
    this.manualWatching = "";
  }

  /** 수동 개입 시작 — 이미 충족이면 안내 없이 진행, 자동 감지 가능한 항목은 감지되면 자동 진행 */
  private openingManual = new WeakSet<RunStep>();
  private async openManual(cur: RunStep, id: ManualId) {
    if (this.openingManual.has(cur)) return;
    this.openingManual.add(cur);
    try { await this.showManual(cur, id); }
    finally { this.openingManual.delete(cur); }
  }

  private async showManual(cur: RunStep, id: ManualId) {
    const gen = this.runGen;
    const manualDone = cur.manualDone;
    const usbReady = id === "usb-debug" && (await this.usbDebugReady());
    if (gen !== this.runGen || cur !== this.runSteps[this.cursor] || cur.manualDone !== manualDone) return;
    if (usbReady) {
      this.log(cur, "[확인] USB 디버깅 연결 확인됨 — 자동으로 진행합니다");
      cur.manualDone++;
      cur.status = "running";
      this.begin();
      return;
    }
    if (id === "firmware-select") {
      if (this.firmwareDirInfo) {
        this.logFirmware(cur);
        cur.manualDone++;
        cur.status = "running";
        this.begin();
        return;
      }
      if (!this.firmware) {
        cur.status = "running";
        this.log(cur, `[자동] Sony 서버에서 순정 펌웨어의 ${this.partition ?? "부트"} 이미지 받는 중`);
        await this.track(this.fetchFirmware());
        if (gen !== this.runGen) return; // 받는 사이 중단·처음으로
      }
      if (this.firmware) {
        this.logFirmware(cur);
        cur.manualDone++;
        cur.status = "running";
        this.begin();
        return;
      }
      cur.status = "manual-wait";
      this.log(cur, `[실패] 순정 펌웨어 자동 다운로드: ${this.firmwareError}`);
    }
    this.manualCheckError = "";
    this.oemUnknownAck = false;
    if (id === "ims-check" || id === "ims-precheck") this.callChecks = [];
    this.manualCurrent = { id, ...MANUAL_TEXT[id] };
    if (id === "smsie-export") {
      const selected = this.checkedBackupItems();
      this.manualCurrent.steps = [
        MANUAL_TEXT[id].steps[0],
        ...(selected.includes("sms") ? [MANUAL_TEXT[id].steps[1]] : []),
        ...(selected.includes("calllog") ? ["Export Call Log를 누릅니다 → 내장 저장소 → volte_sms_backup 폴더 선택 → 저장. 통화 기록 내보내기 성공 안내를 기다리세요"] : []),
        MANUAL_TEXT[id].steps[3], MANUAL_TEXT[id].steps[4],
      ];
    }
    if (id === "smsie-export") this.smsieExportAck = false;
    this.smsieFilesReady = false;
    this.smsieRestoreDetails = "";
    this.manualSetupState = "idle";
    this.onManualOpen(id);
    this.log(cur, `[대기] 수동 개입: ${this.manualCurrent.title}`);
    void this.persist(true);
    if (id === "usb-debug") this.watchManual(cur, id, "USB 디버깅 연결 확인", () => this.usbDebugReady(), 2000);
    if (id === "mode-wait") this.watchManual(cur, id, "부트로더(fastboot) 모드 진입 확인", () => this.usbModeIs("fastboot"), 1500);
    if (id === "flash-mode") this.watchManual(cur, id, "플래시 모드 진입 확인", () => this.usbModeIs("flashmode"), 1500);
    if (id === "ims-check") {
      this.imsVerified = false;
      this.callVerified = false;
      this.imsUnverified = false;
      this.watchManual(cur, id, "셀룰러 IMS 음성 등록 확인", () => this.imsReady(), 5000, false);
    }
    if (id === "su-grant")
      this.watchManual(
        cur,
        id,
        "루트 권한(uid=0) 확인",
        async () => (await api.rootCheck(this.device?.serial)) === true,
        3000,
      );
    if (id === "smsie-export") {
      // 선택한 파일이 두 번 연속(5초 간격) 보이면 수집·검사하고 자동으로 넘어간다.
      // 앱이 아직 쓰는 중이면 ZIP CRC·JSON 검사에서 걸러져 계속 기다린다
      let seen = 0;
      this.watchManual(
        cur,
        id,
        "내보내기 파일 감지",
        async () => {
          if (this.manualChecking) return false;
          const watch = this.watchGeneration;
          const stale = () => gen !== this.runGen || watch !== this.watchGeneration || this.manualCurrent?.id !== id;
          const result = await api.smsieProbe(this.device?.serial, this.backupDir, this.opts.backupOnly);
          if (stale() || this.manualChecking) return false;
          if (!this.reportSmsieProbe(result)) { seen = 0; return false; }
          if (++seen < 2) return false;
          seen = 0;
          const err = await this.collectSmsieExport();
          if (stale()) return false;
          if (err) {
            this.manualCheckError = err;
            this.log(cur, `[확인 실패] ${err}`);
            return false;
          }
          return true;
        },
        5000,
      );
    }
  }

  /** 수동 개입이 열릴 때 자동 동작 — 언락: 발급 페이지 열기 + IMEI 읽기 / 펌웨어: 자동 다운로드 / smsie: 앱 준비 */
  private onManualOpen(id: ManualId) {
    if (id === "unlock-code") {
      // 목 모드(fastboot 실전 꺼짐)에서는 코드가 기기에 쓰이지 않으므로 예시값을 채워 [입력 완료]만 누르면 되게 한다
      if (!REAL_STEPS.fastboot && !this.unlockCode.trim()) this.unlockCode = MOCK_UNLOCK_CODE;
      void api.openExternal(LINKS.unlock);
      void this.loadImei();
    } else if (id === "ims-precheck") {
      const cur = this.runSteps[this.cursor];
      if (cur) this.watchManual(cur, id, "셀룰러 IMS 음성 등록 확인", () => this.imsReady(), 5000, false);
    } else if (id === "oem-toggle") {
      // 폰에 해당 설정 화면을 바로 띄움 (개발자 옵션이 꺼져 있으면 휴대전화 정보 — 빌드번호 연타)
      void this.openPhoneSettings();
    } else if (id === "smsie-export") {
      // 앱 설치·권한·임시 폴더를 백그라운드로 준비 (사용자는 폰에서 내보내기만)
      void this.smsiePrepare();
    } else if (id === "smsie-import") {
      // 백업 파일 전송 + 기본 문자 앱 역할(가져오기 권한) — 비행기 모드 안내 문구 반환
      void this.smsieRestoreStage();
    } else if (id === "mode-wait") {
      // 안내("앱이 재부팅합니다")대로 부트로더 재부팅을 앱이 수행 — 감지는 watchManual 폴링
      void this.rebootToBootloader();
    }
  }

  /** adb로 부트로더 재부팅(mode-wait 수동 개입에서 자동) — mode-wait는 언락·리락(fastboot 엔진) 단계에만 있으므로
   *  fastboot 실전일 때만. 루팅·언루팅은 엔진이 직접 재부팅한다 */
  private async rebootToBootloader() {
    if (!REAL_STEPS.fastboot) return;
    const gen = this.runGen;
    const cur = this.runSteps[this.cursor];
    this.log(cur, "[재부팅] 폰을 부트로더 모드로 재부팅합니다 — USB를 유지해 주세요");
    const r = await api.rootReboot(this.device?.serial, "bootloader");
    if (gen !== this.runGen) return;
    if (!r.ok) {
      this.log(cur, `[실패] 부트로더 재부팅: ${r.error} — 전원을 끈 뒤 볼륨 위를 누른 채 USB를 연결해 직접 진입해 주세요`);
    }
    void this.persist(true);
  }

  /** smsie 수동 복원 준비 */
  async smsieRestoreStage() {
    const gen = this.runGen;
    const cur = this.runSteps[this.cursor];
    if (!REAL_STEPS.restore) return;
    this.manualSetupState = "loading";
    this.manualCheckError = "";
    if (!this.backupDir) {
      this.manualSetupState = "failed";
      this.manualCheckError = "복원할 백업 폴더가 없습니다";
      return;
    }
    const r = await api.smsieRestoreStage(this.device?.serial, this.backupDir, this.checkedBackupItems());
    if (gen !== this.runGen) return;
    this.manualSetupState = r.ok ? "done" : "failed";
    if (r.ok) {
      this.smsieRestoreDetails = r.value;
      this.log(cur, `[준비] ${r.value.split("\n")[0]}`);
    } else {
      this.manualCheckError = r.error;
      this.log(cur, `[실패] 문자·통화 기록 복원 준비: ${r.error}`);
    }
    void this.persist(true);
  }

  /** SMS Import/Export 앱 준비 — 설치(필요 시 GitHub 다운로드)·권한·임시 폴더 */
  async smsiePrepare() {
    const gen = this.runGen;
    const cur = this.runSteps[this.cursor];
    if (!this.backupLive) return;
    if (this.manualSetupState === "loading") return;
    this.manualSetupState = "loading";
    this.manualCheckError = "";
    await this.watchInFlight;
    if (gen !== this.runGen || this.manualCurrent?.id !== "smsie-export") return;
    const r = await this.track(api.smsiePrepare(this.device?.serial, true, this.opts.backupOnly));
    if (gen !== this.runGen) return;
    this.manualSetupState = r.ok ? "done" : "failed";
    if (r.ok) {
      this.log(cur, `[준비] SMS Import/Export — ${r.value.join(" · ")}`);
    } else {
      this.manualCheckError = r.error;
      this.log(cur, `[실패] SMS Import/Export 준비: ${r.error}`);
    }
    void this.persist(true);
  }

  async openPhoneSettings() {
    const cur = this.runSteps[this.cursor];
    const screen = this.device?.prep.developerOptions === false ? "about" : "developer";
    if (!(await api.openSettingsScreen(this.device?.serial, screen))) {
      this.log(cur, "[안내] 폰의 설정 화면을 자동으로 열지 못했습니다 — 폰에서 직접 열어 주세요");
    }
  }

  private imeiRequest = 0;
  async loadImei() {
    const gen = this.runGen;
    const request = ++this.imeiRequest;
    this.imeiState = "loading";
    const imei = await api.readImei1(this.device?.serial);
    if (request !== this.imeiRequest) return; // 더 새로운 조회가 상태를 관리한다
    if (gen !== this.runGen) {
      // 중단·처음으로 — 결과는 버리고 로딩 표시만 푼다
      if (this.imeiState === "loading") this.imeiState = "idle";
      return;
    }
    this.imei1 = imei;
    this.imeiState = this.imei1 ? "done" : "failed";
  }

  async fetchFirmware() {
    const gen = this.runGen;
    const request = ++this.firmwareRequest;
    const partition = this.partition;
    this.firmware = null;
    this.firmwareError = "";
    if (!partition) {
      this.firmwareState = "failed";
      this.firmwareFail = "download";
      this.firmwareError = "이 기종의 대상 파티션이 확인되지 않아 자동으로 받을 수 없습니다";
      return;
    }
    this.firmwareState = "loading";
    // 업데이트를 고른 경우 루팅용 이미지는 새 버전 것
    const r = await api.firmwareFetch(this.device?.serial, partition, this.updateVersion ?? undefined, this.firmwareDest || undefined);
    if (request !== this.firmwareRequest) return; // 더 새로운 요청이 상태를 관리한다
    if (gen !== this.runGen || partition !== this.partition) {
      // 중단·기기 변경 — 결과는 버리고 [다시 받기]가 막히지 않게 로딩 표시만 푼다
      if (this.firmwareState === "loading") this.firmwareState = "idle";
      return;
    }
    if (r.ok) {
      this.firmware = r.value;
      this.firmwareState = "done";
      this.firmwareFail = null;
    } else {
      this.firmwareState = "failed";
      const space = r.error.startsWith("NO_SPACE|");
      this.firmwareFail = space ? "space" : "download";
      this.firmwareError = space ? r.error.slice("NO_SPACE|".length) : r.error;
    }
  }

  /** 실패 팝업에서 다시 받기 — 성공하면 팝업을 닫고 바로 진행 */
  async retryFirmware() {
    if (this.manualCurrent?.id !== "firmware-select" || this.firmwareState === "loading") return;
    const gen = this.runGen;
    await this.track(this.fetchFirmware());
    // 그사이 중단·다른 안내로 넘어갔으면 진행하지 않는다
    if (this.firmware && gen === this.runGen && this.manualCurrent?.id === "firmware-select") this.ackManual();
  }

  private logFirmware(cur: RunStep) {
    // 실제로 쓰는 이미지(stockSource)와 같은 우선순위 — 직접 지정한 폴더 → 자동 다운로드
    const fw = this.firmware;
    const dir = this.firmwareDirInfo;
    this.log(
      cur,
      dir
        ? `[입력] 펌웨어 폴더: ${this.firmwareDir} (${dir.file})`
        : fw
          ? `[준비] 순정 펌웨어 ${fw.version} — ${fw.partition} 자동 다운로드(${(fw.downloadedBytes / 1024 ** 2).toFixed(1)} MB 받음), 기기 지문 일치`
          : "[목업] 순정 펌웨어 준비를 건너뛰었습니다",
    );
  }

  /** 언락 코드 형식 — 16자리 16진수 (Sony 발급 코드) */
  get unlockCodeValid(): boolean {
    return /^[0-9a-f]{16}$/i.test(this.normalizedUnlockCode);
  }

  /** 언락 코드 정규화 — 사용자가 붙여넣은 0x 접두어 제거 (명령 조립 시 0x 중복 방지) */
  get normalizedUnlockCode(): string {
    return this.unlockCode.trim().replace(/^0x/i, "");
  }

  /** 언락 사전 조건 중 꺼져 있는 항목 (false만 — 판별 불가는 막지 않음) */
  get prepMissing(): string[] {
    const p = this.device?.prep;
    if (!p) return [];
    const out: string[] = [];
    if (p.developerOptions === false) out.push("개발자 옵션");
    if (p.usbDebugging === false) out.push("USB 디버깅");
    if (p.oemUnlockAllowed === false) out.push("OEM 잠금 해제");
    return out;
  }

  prepChecking = $state(false);

  /** [다시 확인] — 기기 상태를 다시 읽어 사전 조건 갱신 (같은 기기일 때만) */
  async recheckPrep() {
    this.prepChecking = true;
    try {
      const list = await api.deviceList();
      const d = list?.find((x) => x.state === "device" && x.serial === this.device?.serial);
      if (d) this.device = d;
    } finally {
      this.prepChecking = false;
    }
  }

  /** 백업 직전 안내 동의 */
  backupNoticeAck = $state(false);

  // ── 수동 확인 검증 — "완료" 버튼은 건너뛰기가 아니라 실제 확인 후 진행 ──
  manualChecking = $state(false);
  private manualCheckErrorText = $state("");
  get manualCheckError() { return this.manualCheckErrorText; }
  set manualCheckError(value: string) {
    this.manualCheckErrorText = value;
    this.probeOwnsCheckError = false;
  }
  manualSetupState = $state<LoadState>("idle");
  smsieFilesReady = $state(false);
  smsieRestoreDetails = $state("");
  smsieExportAck = $state(false);
  /** 리락 전 통신 확인 — 실제 발신·수신을 확인했다는 체크 */
  imsVerified = $state(false);
  callVerified = $state(false);
  /** 언락 조건 중 "확인 불가" 항목을 폰에서 직접 켰다고 확인 */
  oemUnknownAck = $state(false);
  firmwareDirInfo: FirmwareDirInfo | null = $state(null);
  firmwareDirState = $state<LoadState>("idle");
  firmwareDirError = $state("");
  /** 최종 VoLTE 확인을 건너뛰고 마무리했는지 (SIM 없이 미리 패치 등) */
  imsUnverified = $state(false);

  /** 언락 조건 중 판별할 수 없는 항목 */
  get prepUnknown(): string[] {
    const p = this.device?.prep;
    if (!p) return ["개발자 옵션", "USB 디버깅", "OEM 잠금 해제"];
    const out: string[] = [];
    if (p.developerOptions === null) out.push("개발자 옵션");
    if (p.usbDebugging === null) out.push("USB 디버깅");
    if (p.oemUnlockAllowed === null) out.push("OEM 잠금 해제");
    return out;
  }

  /** 실제 확인이 필요한 수동 단계인지 (동의·입력형 제외) */
  get manualVerifiable(): boolean {
    const id = this.manualCurrent?.id;
    return !!id && ["oem-toggle", "usb-debug", "mode-wait", "flash-mode", "su-grant", "ims-check", "ims-precheck", "smsie-export", "contacts-import"].includes(id);
  }

  /** 목업 실행에서만 — 폰이 실제로 재부팅되지 않아 확인할 수 없는 단계 건너뛰기 */
  get manualSkippable(): boolean {
    return this.simulationControlsVisible && (this.manualVerifiable || this.manualCurrent?.id === "firmware-select");
  }
  get manualCanDismiss(): boolean { return !this.manualChecking && this.busy === 0 && !this.runInDanger; }

  /** 직접 지정한 펌웨어 폴더 — 고르는 즉시 검사 */
  private firmwareRequest = 0;
  private firmwareDirRequest = 0;
  async setFirmwareDir(dir: string) {
    const gen = this.runGen;
    const request = ++this.firmwareDirRequest;
    this.firmwareDir = dir;
    this.firmwareDirInfo = null;
    this.firmwareDirError = "";
    if (!dir) { this.firmwareDirState = "idle"; return; }
    const partition = this.partition;
    if (!partition) {
      this.firmwareDirState = "failed";
      this.firmwareDirError = "이 기종의 대상 파티션이 확인되지 않아 폴더를 검사할 수 없습니다";
      return;
    }
    this.firmwareDirState = "loading";
    const r = await api.firmwareDirCheck(dir, partition);
    if (request !== this.firmwareDirRequest) return; // 더 새로운 검사가 상태를 관리한다
    if (gen !== this.runGen || this.firmwareDir !== dir || partition !== this.partition) {
      if (this.firmwareDirState === "loading") this.firmwareDirState = "idle";
      return;
    }
    if (r.ok) {
      this.firmwareDirInfo = r.value;
      this.firmwareDirState = "done";
    } else {
      this.firmwareDirState = "failed";
      this.firmwareDirError = r.error;
    }
  }

  /** 수동 단계 실제 확인 — 통과하면 null, 아니면 사용자에게 보여줄 사유 */
  private async verifyManual(id: ManualId): Promise<string | null> {
    switch (id) {
      case "oem-toggle": {
        await this.recheckPrep();
        if (this.prepMissing.length > 0) return `${this.prepMissing.join(", ")}이(가) 꺼져 있습니다 — 폰에서 켠 뒤 다시 확인해 주세요`;
        if (this.prepUnknown.length > 0 && !this.oemUnknownAck) return `${this.prepUnknown.join(", ")}을(를) 자동으로 확인할 수 없습니다 — 폰에서 직접 켠 뒤 아래에 체크해 주세요`;
        return null;
      }
      case "usb-debug":
        return (await this.usbDebugReady()) ? null : "폰이 아직 USB 디버깅으로 연결되지 않았습니다 — 폰 화면에서 '허용'을 눌렀는지 확인해 주세요";
      case "mode-wait":
      case "flash-mode": {
        if (await this.usbModeIs(id === "mode-wait" ? "fastboot" : "flashmode")) return null;
        if ((await this.sonyUsbCount()) > 1) return "Xperia가 여러 대 연결되어 있습니다 — 작업할 폰만 남기고 나머지는 분리해 주세요";
        return id === "mode-wait"
          ? "부트로더(fastboot) 모드가 감지되지 않았습니다 — USB 연결을 확인해 주세요"
          : "플래시 모드가 감지되지 않았습니다 — 전원을 끈 뒤 볼륨 아래 버튼을 누른 채 USB를 연결해 주세요";
      }
      case "su-grant": {
        const ok = await api.rootCheck(this.device?.serial);
        return ok ? null : "루트 권한이 확인되지 않았습니다 — 폰에서 Magisk 권한 요청을 '허용'했는지 확인해 주세요";
      }
      case "ims-precheck":
        return this.imsCheckError("VoLTE 등록이 확인되지 않았습니다 — 신호가 잡힐 때까지 기다리거나, 통화가 안 되면 [다시 패치]로 VoLTE 적용부터 다시 진행해 주세요");
      case "ims-check":
        return this.imsCheckError("VoLTE 등록이 아직 확인되지 않았습니다 — 재부팅 후 통신사 신호가 잡힐 때까지 잠시 기다려 주세요");
      case "contacts-import": {
        const gen = this.runGen;
        const serial = this.device?.serial;
        const dir = this.backupDir;
        const r = await api.contactsRestoreCheck(serial, dir);
        if (gen !== this.runGen) return "이전 실행의 연락처 확인 결과입니다";
        if (!r) return "연락처 수를 확인하지 못했습니다 — 폰 연결을 확인해 주세요";
        if (r.onDevice >= r.backedUp) {
          if (REAL_STEPS.restore) {
            const cleanup = await api.contactsRestoreFinish(serial, dir);
            if (gen !== this.runGen) return "이전 실행의 연락처 정리 결과입니다";
            if (!cleanup.ok) return `연락처 가져오기는 확인됐지만 임시 파일 정리에 실패했습니다: ${cleanup.error}`;
          }
          this.log(this.runSteps[this.cursor], `[확인] 연락처 ${r.onDevice}명 (백업 ${r.backedUp}명)`);
          return null;
        }
        return `폰의 연락처가 ${r.onDevice}명으로 백업(${r.backedUp}명)보다 적습니다 — 가져오기가 끝났는지 확인해 주세요`;
      }
      case "smsie-export": {
        // [지금 확인] — 진행 중인 자동 감지가 끝난 뒤 바로 수집·검사한다. 그사이 중단됐으면 수집하지 않는다
        const gen = this.runGen;
        await this.watchInFlight;
        if (gen !== this.runGen || this.manualCurrent?.id !== "smsie-export") return "이전 실행의 문자 백업 확인입니다";
        return this.collectSmsieExport();
      }
      default:
        return null;
    }
  }

  /** 문자·통화 내보내기 파일 수집 — ZIP 전체 CRC·JSON 구조까지 검사하므로 앱이 아직 쓰는 중이면 통과하지 않는다.
   *  통과하면 PC 백업 완료 처리와 폰 임시 사본 정리까지 한다. 성공 null, 아니면 사유 */
  private async collectSmsieExport(): Promise<string | null> {
    const gen = this.runGen, serial = this.device?.serial, dir = this.backupDir, backupOnly = this.opts.backupOnly;
    if (gen !== this.runGen || this.manualCurrent?.id !== "smsie-export") return "이전 실행의 문자 백업 확인입니다";
    const result = await api.smsieCollect(serial, dir, true, backupOnly);
    if (gen !== this.runGen || this.manualCurrent?.id !== "smsie-export") return "이전 실행의 문자 백업 확인입니다";
    if (!result.ok) return `산출 파일 확인 실패: ${result.error}`;
    const outcome = result.value;
    if (outcome?.ready && outcome.summary) {
      if (outcome.cleanupWarning) this.log(this.runSteps[this.cursor], `[경고] PC 백업은 완료됐지만 폰 임시 사본 정리에 실패했습니다: ${outcome.cleanupWarning}`);
      this.backupSummary = outcome.summary;
      return null;
    }
    if (outcome?.summary?.errors.length) return outcome.summary.errors.join(" / ");
    return "아직 내보내기 파일이 감지되지 않았습니다 — 내장 저장소 → volte_sms_backup을 선택하고 저장 버튼을 누른 뒤 내보내기 완료를 기다려 주세요";
  }

  /** VoLTE(IMS) 등록 확인 — 등록되면 null, Wi-Fi 통화로만 잡혔거나 미등록이면 사유 (기기 조회는 한 번) */
  private async imsCheckError(notRegistered: string): Promise<string | null> {
    if (await this.imsReady()) return null;
    if (this.communicationError) return this.communicationError;
    const slots = this.communicationSlots;
    const missing = slots.filter(slot => !cellularReady(this.imsSims.find(s => s.slot === slot)));
    return missing.length ? missing.map(slot => { const sim = this.imsSims.find(s => s.slot === slot); return `SIM${slot}: ${sim ? imsLabel(sim) : "SIM 상태 확인 불가"}`; }).join(" / ") : notRegistered;
  }

  /** [확인하고 진행] — 확인되면 진행, 아니면 사유 표시 */
  async confirmManual() {
    const m = this.manualCurrent;
    if (!m || !this.manualInputReady || this.manualChecking) return;
    this.manualChecking = true;
    this.manualCheckError = "";
    try {
      const gen = this.runGen;
      const err = await this.verifyManual(m.id);
      if (gen !== this.runGen || this.manualCurrent?.id !== m.id) return; // 그사이 중단·자동 감지로 진행됨
      if (err) {
        this.manualCheckError = err;
        this.log(this.runSteps[this.cursor], `[확인 실패] ${err}`);
        void this.persist(true);
        return;
      }
      if (m.id === "ims-check") {
        this.imsVerified = true;
        this.callVerified = this.callAck;
        this.imsUnverified = false;
        this.log(this.runSteps[this.cursor], this.callVerified ? "[확인] IMS 등록 및 사용자 발신·수신 확인" : "[확인] IMS 등록 확인 — 실제 통화는 미확인");
      }
      this.stopWatch();
      this.ackManual();
    } finally {
      this.manualChecking = false;
    }
  }

  /** (목업) 확인 건너뛰기 — SIMULATED_RUN에서만 */
  skipManual() {
    if (!this.manualSkippable) return;
    this.log(this.runSteps[this.cursor], "[목업] 확인을 건너뛰었습니다 — 실제 실행에서는 확인될 때까지 진행하지 않습니다");
    this.stopWatch();
    this.ackManual(true);
  }

  /** 통신 확인 생략 — SIM 상태로 파일 작업·언루팅을 차단하지 않는다. 리락 자체의 검증 게이트는 별도다. */
  finishWithoutIms() {
    const id = this.manualCurrent?.id;
    if (id !== "ims-check" && id !== "ims-precheck") return;
    const cur = this.runSteps[this.cursor];
    if (!cur) return;
    cur.communicationSkipped = true;
    if (id === "ims-check") {
      this.imsUnverified = true;
      this.imsVerified = false;
      this.callVerified = this.callAck;
    }
    void this.persist(true);
    this.log(this.runSteps[this.cursor], "[확인 생략] IMS 등록 미확인으로 마무리합니다 — 파일 기록 검증 결과는 유지됩니다. SIM을 넣거나 바꾸면 재패치가 필요할 수 있습니다");
    this.stopWatch();
    this.ackManual(true);
  }

  /** 입력형 수동 개입은 값이 채워져야 완료 가능 */
  get manualInputReady(): boolean {
    const m = this.manualCurrent;
    if (m?.id === "smsie-export" && this.backupLive) return this.manualSetupState === "done";
    if (m?.id === "smsie-import" && REAL_STEPS.restore) return this.manualSetupState === "done";
    if (m?.id === "backup-notice") return this.backupNoticeAck;
    if (m?.id === "ims-precheck") return this.callAck;
    if (m?.id === "oem-toggle") return this.prepMissing.length === 0 && (this.prepUnknown.length === 0 || this.oemUnknownAck) && !this.prepChecking;
    if (!m?.input) return true;
    if (m.input === "unlock-code") return this.unlockCodeValid;
    return this.firmware !== null || this.firmwareDirInfo !== null;
  }

  /** 수동 단계 완료 처리 — 확인을 거친 경로(confirmManual·자동 감지·목업 건너뛰기)에서만 호출 */
  ackManual(force = false) {
    // 열린 안내가 없으면(중복 클릭·이미 진행됨) manualDone을 더 올리지 않는다
    if (!this.manualCurrent) return;
    if (!force && !this.manualInputReady) return;
    this.manualCheckError = "";
    const cur = this.runSteps[this.cursor];
    const m = this.manualCurrent;
    if (cur) {
      if (m?.input === "unlock-code") this.log(cur, `[입력] 언락 코드: 0x${maskSecret(this.normalizedUnlockCode)}`);
      if (m?.input === "firmware") this.logFirmware(cur);
      cur.status = "running";
      cur.manualDone++;
    }
    this.manualCurrent = null;
    this.begin();
    void this.persist(true);
  }

  // ── 실전 백업·복구 (REAL_STEPS 전환 시) ───────────────────
  // 계약: .plans/02-contracts/tauri-commands.md backup 절. 백엔드 이벤트로 progress·로그·체크포인트를 올린다.

  /** 이번 실행에서 선택한 백업 항목 id (mock 그룹에서 checked만) */
  private checkedBackupItems(): string[] {
    return this.groups.flatMap((g) => g.items.filter((i) => i.checked).map((i) => i.id));
  }

  private async runRealBackup(cur: RunStep) {
    const gen = this.runGen;
    // 준비보다 먼저 실행 식별값을 정한다 — 준비~시작 사이의 중단도 이 실행을 취소한다
    const runId = crypto.randomUUID();
    this.backupRunId = runId;
    const items = this.checkedBackupItems();
    const dest = this.backupPath;
    if (!dest.trim()) {
      this.failStep("백업 저장 위치가 지정되지 않았습니다");
      return;
    }
    // 기존 폴더를 이어 받으면 먼저 PC 검사를 거친다 — 진행률 앞 10%를 그 검사에 쓴다
    const progressMode: "run" | "resume" = this.backupDir ? "resume" : "run";
    // 시작 시각 기준 폴더를 먼저 만들고 절대 경로를 진행 기록에 저장 — 끊겨도 같은 폴더로 이어서 받는다
    if (!this.backupDir) {
      const prep = await api.backupPrepare(this.device?.serial, dest, this.opts.backupOnly);
      if (gen !== this.runGen) return;
      if (!prep.ok) {
        this.failStep(`백업 폴더 생성 실패: ${prep.error}`);
        return;
      }
      this.backupDir = prep.value;
      await this.persist(true);
      if (gen !== this.runGen) return;
    }
    this.log(cur, `[실전] 백업 시작 — 항목 ${items.length}개 → ${this.backupDir}`);
    // 진행 이벤트 구독 → progress·sub 체크포인트 반영
    const itemOrder = items.slice();
    const completedItems = new Set<string>();
    const markItemDone = (itemId: string) => {
      if (!cur.sub) return;
      completedItems.add(itemId);
      while (cur.sub.done < itemOrder.length && completedItems.has(itemOrder[cur.sub.done])) {
        const id = itemOrder[cur.sub.done++];
        this.log(cur, `[체크포인트] ${BACKUP_ITEM_LABEL[id] ?? id} 완료`);
      }
      void this.persist(true);
    };
    let logGate = 0;
    let scanLogAt = 0;
    const un = await api.onBackupProgress((p) => {
      if (gen !== this.runGen) return;
      cur.progress = advance(cur.progress, itemProgress(p, itemOrder, progressMode));
      this.transferStatus = transferStatusText(p);
      if (p.phase === "done") markItemDone(p.itemId);
      else if (p.phase === "scan" && p.file && Date.now() - scanLogAt >= 1000) {
        scanLogAt = Date.now();
        this.log(cur, `[목록 확인] ${BACKUP_ITEM_LABEL[p.itemId] ?? p.itemId} — ${p.file}`);
      }
      else if (p.phase !== "scan" && p.file && logGate++ % 25 === 0) {
        // 항목 이름 [끝난 파일 수 / 전체 파일 수] — 몇 개가 끝났고 몇 개 남았는지
        const count = p.filesTotal > 0 ? ` [${p.filesDone.toLocaleString()} / ${p.filesTotal.toLocaleString()}]` : "";
        this.log(cur, `[${p.phase === "verify" ? "PC 검사" : p.phase === "metadata" ? "원본 속성" : "백업"}] ${BACKUP_ITEM_LABEL[p.itemId] ?? p.itemId}${count} — ${p.file}`);
      }
      void this.persist();
    });
    if (gen !== this.runGen) return un();
    const r = await api
      .backupRun(this.device?.serial, items, dest, runId, this.backupDir || undefined, this.opts.backupOnly)
      .finally(() => {
        un();
        this.transferStatus = "";
        if (this.backupRunId === runId) this.backupRunId = undefined;
      });
    if (gen !== this.runGen) return; // 중단·처음으로
    if (!r.ok) {
      this.failStep(`백업 실패: ${r.error}`);
      return;
    }
    this.backupDir = r.value.dir;
    this.backupSummary = r.value;
    for (const item of r.value.items) if (item.status === "done") markItemDone(item.id);
    this.log(cur, `[백업] ${r.value.dir} — 파일 ${r.value.files.toLocaleString()}개, ${(r.value.bytes / 1024 ** 3).toFixed(2)} GiB`);
    // 다른 항목의 오류와 무관하게 문자·통화를 먼저 마친다 — 시도도 안 한 항목을 "미완료"로 함께 실패시키지 않는다.
    // 이미 완료된 문자·통화는 다시 내보내지 않고, 남은 오류는 마지막(finishRealBackup)에 한 번만 알린다.
    if (!this.needSmsie(items)
      || items.filter(id => ["sms", "calllog"].includes(id)).every(id => r.value.items.some(item => item.id === id && item.status === "done"))) return this.finishRealBackup(cur);
    // 문자·통화 기록(smsie) — 앱 설치·권한은 자동, 내보내기 2탭은 수동 개입
    this.openEngineManual(cur, "smsie-export");
  }

  /** 엔진이 이어 붙이는 수동 안내 — 단계 정의에 추가하고 그 안내부터 연다.
   *  manualDone을 그 안내의 위치로 맞춰, 확인 후 앞의 안내(예: backup-notice)나 같은 안내를 다시 열지 않는다 */
  private openEngineManual(cur: RunStep, id: ManualId) {
    const stepDef = this.steps.find((s) => s.id === cur.id);
    if (stepDef && !stepDef.manual?.includes(id)) stepDef.manual = [...(stepDef.manual ?? []), id];
    cur.manualDone = Math.max(0, stepDef?.manual?.indexOf(id) ?? 0);
    cur.status = "manual-wait";
    this.pause();
    void this.openManual(cur, id);
  }

  private needSmsie(items: string[]): boolean {
    return items.includes("sms") || items.includes("calllog");
  }

  /** 완결 판정으로 백업 단계 마무리 — 파괴 단계(언락/리락) 게이트의 입력이 된다 */
  private finishRealBackup(cur: RunStep) {
    const s = this.backupSummary;
    if (!s) return this.failStep("백업 결과가 없습니다");
    const removedApps = s.omittedApps?.filter(app => !app.cleanupPending) ?? [];
    if (removedApps.length && !this.backupOmissionNotice) {
      this.backupOmittedApps = removedApps;
      this.backupOmissionNotice = true;
      for (const app of removedApps) this.log(cur, `[경고] ${app.package}: 읽기 권한 문제로 앱 데이터 백업 전체 제외·PC 사본 삭제, APK 유지`);
    }
    if (s.complete) {
      cur.progress = 1;
      this.log(cur, s.omittedApps?.length
        ? `[완결] 보관할 백업 완료 — 앱 데이터 제외 ${s.omittedApps.length}개. 안내를 확인한 뒤 [다음]을 눌러 진행하세요`
        : "[완결] 전수 열거 완료 · 오류 0 — [다음]을 눌러 진행하세요");
      this.stepDone(cur);
    } else {
      const detail = s.errors.slice(0, 3).join(" / ");
      const action = s.errors.length > 0 && s.errors.every(error => /permission denied/i.test(error))
        ? "폰에서 표시된 앱 파일·폴더의 읽기를 거부했습니다. 필요한 데이터는 해당 앱에서 별도로 내보내 주세요. 접근 권한이 해결되기 전에는 같은 백업을 반복해도 해결되지 않습니다"
        : "로그의 오류 원인을 해결한 뒤 [이 단계 다시 시도]로 재시도할 수 있습니다";
      this.failStep(`백업 미완결${detail ? ` — ${detail}` : ""} — ${action}`);
    }
  }

  /** 실전 엔진이 있는 단계인지 (엔진을 시작하지는 않는다) */
  private hasRealEngine(id: string): boolean {
    return this.engineFor({ id } as RunStep) !== null;
  }

  /** 실전 업데이트 확인 — 업데이트 후 폰이 다시 연결되면 버전·지문을 대조(재부팅은 펌웨어 기록 뒤 폰이 스스로 한다) */
  private async runRealFwVerify(cur: RunStep) {
    const gen = this.runGen;
    // 대상은 선택값(진행 기록에 남음)에서 — 재시작 뒤 기기 정보가 업데이트 후 값으로 바뀌어도 그대로
    const target = this.volteConfig.firmware;
    if (!target) return this.failStep("업데이트 대상 버전을 알 수 없습니다 — 계획을 다시 만들어 주세요");
    // 펌웨어 기록이 시뮬레이션이면 버전이 바뀔 수 없다 — 오래 기다리지 않고 바로 알린다
    if (!this.hasRealEngine("fw-flash")) {
      return this.failStep("펌웨어 기록이 아직 실전으로 구현되지 않아 업데이트를 실제로 확인할 수 없습니다");
    }
    // 대상 버전 펌웨어를 받아 둔 경우 그 지문(기기·지역 대조를 이미 거친 값)과 정확히 같아야 한다
    const expected = this.firmware?.version === target ? this.firmware.fingerprint : undefined;
    this.log(cur, `[확인] 업데이트 후 폰이 부팅되어 다시 연결되기를 기다립니다 (대상 ${target}) — USB는 연결한 채로 두세요`);
    // 업데이트 후 첫 부팅은 오래 걸릴 수 있다(최적화 포함)
    const back = await this.waitFor(gen, () => this.usbDebugReady(), 900_000, 3000);
    if (gen !== this.runGen) return;
    if (!back) return this.failStep("업데이트 후 폰이 다시 연결되지 않습니다 — 부팅이 끝났는지, USB 디버깅이 켜져 있는지 확인해 주세요");
    this.markSub(cur, 1);
    cur.progress = 1 / 3;
    const d = (await api.deviceList())?.find((x) => x.state === "device" && x.serial === this.device?.serial);
    if (gen !== this.runGen) return;
    if (!d) return this.failStep("폰 정보를 읽지 못했습니다 — 연결을 확인한 뒤 [이 단계 다시 시도]를 눌러 주세요");
    const problems = firmwareUpdateProblems(target, d, { before: this.device?.fingerprint, expected });
    if (problems.length > 0) return this.failStep(`업데이트 확인 실패 — ${problems.join(" / ")}`);
    this.log(cur, `[확인] 버전 ${d.firmware} · 지문 일치`);
    this.markSub(cur, 3);
    cur.progress = 1;
    this.stepDone(cur);
  }

  /** adb로 OS 재부팅 → 연결 끊김 → 다시 연결 대기. 실패 사유 또는 null (호출부에서 세대 확인) */
  private async rebootOsAndReconnect(gen: number): Promise<string | null> {
    const rb = await api.rootReboot(this.device?.serial, "os");
    if (gen !== this.runGen) return null;
    if (!rb.ok) return `재부팅 요청 실패: ${rb.error}`;
    return this.waitForOsReconnect(gen);
  }

  /** 재부팅 전의 연결을 복귀로 오인하지 않도록 끊김과 재연결을 순서대로 확인한다. */
  private async waitForOsReconnect(gen: number): Promise<string | null> {
    // 재부팅 요청 직후에는 아직 연결돼 있을 수 있다 — 끊겼다가 다시 붙는 것까지 확인
    const down = await this.waitFor(gen, async () => !(await this.usbDebugReady()), 60_000);
    if (gen !== this.runGen) return null;
    if (!down) return "재부팅이 감지되지 않습니다 — 폰 화면을 확인한 뒤 [이 단계 다시 시도]를 눌러 주세요";
    const back = await this.waitFor(gen, () => this.usbDebugReady(), 300_000, 3000);
    if (gen !== this.runGen) return null;
    return back ? null : "재부팅 후 폰이 다시 연결되지 않습니다 — 부팅이 끝났는지, USB 디버깅 허용을 확인해 주세요";
  }

  /** 실전 최종 확인 — OS 재부팅 → 재연결 → VoLTE 등록 확인(수동 안내창이 자동 감지) */
  private async runRealFinalVerify(cur: RunStep) {
    const gen = this.runGen;
    // 이어서 진행할 때 이미 재부팅을 마쳤으면 다시 하지 않는다([이 단계 다시 시도]는 체크포인트를 지워 처음부터)
    if ((cur.sub?.done ?? 0) < 1) {
      this.log(cur, "[재부팅] 설정을 반영하기 위해 폰을 다시 시작합니다");
      const error = await this.rebootOsAndReconnect(gen);
      if (gen !== this.runGen) return;
      if (error) return this.failStep(error);
      this.markSub(cur, 1);
    }
    cur.progress = 1 / 3;
    this.log(cur, "[확인] 네트워크·VoLTE 등록을 확인합니다");
    // 등록 대기는 수동 안내창의 자동 감지(imsReady 폴링)가 맡는다 — 미확인 종료 경로도 그대로
    this.openEngineManual(cur, "ims-check");
  }

  /** 최종 확인 마무리 — VoLTE 등록 확인(또는 확인 없이 마무리) 뒤 재진입 */
  private finishRealFinalVerify(cur: RunStep) {
    if (this.imsUnverified) {
      this.log(cur, "[미확인] VoLTE 등록을 확인하지 못한 채 단계를 마칩니다");
    } else {
      this.markSub(cur, 3);
    }
    cur.progress = 1;
    this.stepDone(cur);
  }

  // ── 실전 VoLTE 설정·통신/최종 확인 (REAL_STEPS.efs) ──

  /** VoLTE 활성화 설정 — persist.dbg 4종 setprop 후 재부팅, adb 복귀 대기 */
  private async runRealVolteProps(cur: RunStep) {
    const gen = this.runGen;
    cur.progress = 0.3;
    this.log(cur, "[설정] VoLTE·영상통화·Wi-Fi 통화 프롭 적용 — 루트 권한 필요");
    const r = await api.voltePropsSet(this.device?.serial);
    if (gen !== this.runGen) return;
    if (!r.ok) return this.failStep(`VoLTE 설정 실패: ${r.error}`);
    for (const p of r.value) this.log(cur, `[설정] ${p}`);
    this.log(cur, "[재부팅] 설정 적용 후 재부팅 — 폰이 다시 부팅될 때까지 기다립니다");
    void this.persist(true);
    const error = await this.waitForOsReconnect(gen);
    if (gen !== this.runGen) return;
    if (error) return this.failStep(`VoLTE 설정 후 ${error}`);
    cur.progress = 1;
    this.log(cur, "[완료] VoLTE 설정 적용 — 다음 단계(최종 확인)에서 IMS 등록을 확인합니다");
    this.stepDone(cur);
  }

  /** 통신 확인 — ims-precheck 수동 개입 + imsReady 자동 판정 */
  private async runRealCommCheck(cur: RunStep) {
    const gen = this.runGen;
    if (cur.communicationSkipped) {
      this.log(cur, "[미확인] 사용자 선택으로 통신 확인을 생략합니다 — 파일 기록·리드백 결과는 유지됩니다");
      cur.progress = 1;
      this.stepDone(cur);
      return;
    }
    cur.progress = 0.5;
    // imsReady 호출 — IMS 등록 + 슬롯별 상태 표시
    const ready = await this.imsReady();
    if (gen !== this.runGen) return;
    if (ready) {
      cur.progress = 1;
      this.log(cur, "[확인] VoLTE(IMS) 등록 확인됨");
      this.stepDone(cur);
      return;
    }
    // 미등록 — 수동 개입(ims-precheck)이 아직 처리 안 됐으면 수동으로 넘김
    // (ims-precheck는 PlanStep.manual에 있으므로 openManual이 처리)
    // 여기까지 왔다는 건 수동도 끝났는데도 미등록 → 실패
    this.failStep("통신이 확인되지 않습니다 — [다시 패치]로 VoLTE 적용부터 다시 진행할 수 있습니다");
  }

  /** PC 입력만 검사. 언락·루팅 전에 실행하고, 각 EFS 단계에서도 다시 검사한다. */
  private async validatedEfsInputs(gen: number): Promise<EfsConfiguration | null> {
    const problem = patchProcedureProblem(this.device?.model ?? "", this.volteConfig.sims.flatMap(s => s.carrier ? [s.carrier] : []));
    if (problem) { this.failStep(problem); return null; }
    const cfg = this.efsRunConfiguration ? { ok: true as const, value: this.efsRunConfiguration } : await api.efsConfiguration();
    if (gen !== this.runGen) return null;
    if (!cfg.ok) { this.failStep(cfg.error); return null; }
    if (!cfg.value) { this.failStep("EFS COM·프리셋·스냅샷 위치를 작업 옵션에서 먼저 설정해 주세요"); return null; }
    const selected = this.volteConfig.sims.filter(s => s.carrier !== null).map(s => efsPreset(s.carrier!, s.slot)?.folder);
    if (!selected.length || selected.some(folder => !folder)) { this.failStep("선택한 SIM 프리셋을 찾을 수 없습니다"); return null; }
    const validation = await api.efsValidatePresets(selected as string[], cfg.value);
    if (gen !== this.runGen) return null;
    if (!validation.ok) { this.failStep(validation.error); return null; }
    return this.efsRunConfiguration = { ...cfg.value };
  }

  private async runRealEfsInputs(cur: RunStep) {
    const gen = this.runGen;
    const problem = executionPlanProblem(this.runSteps.map(s => s.id), this.executionFlags);
    if (problem) return this.failStep(problem);
    if (!await this.validatedEfsInputs(gen)) return;
    const tool = await api.efsToolCheck();
    if (gen !== this.runGen) return;
    if (!tool.ok) return this.failStep(tool.error);
    if (!tool.value.deviceExecution) return this.failStep("이 빌드에서는 EFS 기기 실행이 비활성화되어 있습니다");
    const needsRoot = this.runSteps.some(s => ["root", "unroot"].includes(s.id));
    if (needsRoot && !tool.value.rootExecution) return this.failStep("이 계획에 필요한 root-write 기능이 빌드에 없습니다");
    if ((needsRoot || this.runSteps.some(s => s.id === "unlock")) && !tool.value.fastbootExecution) {
      return this.failStep("이 계획에 필요한 fastboot-write 기능이 빌드에 없습니다");
    }
    this.log(cur, "[확인] EFS 설정·선택한 프리셋 해시·슬롯 간 충돌 검사 통과 (기기 접근 없음)");
    cur.progress = 1;
    this.stepDone(cur);
  }

  /** Native EFS: explicit COM, before-image, two passes per slot and target readback. */
  private async runRealNativeEfs(cur: RunStep) {
    const gen = this.runGen;
    let unlog = () => {};
    let unprogress = () => {};
    let progressBase = 0;
    let progressScale = 0;
    try {
      const cfg = await this.validatedEfsInputs(gen);
      if (!cfg) return;
      unlog = await api.onEfsLog(ev => { if (gen === this.runGen) this.log(cur, `[EFS/${ev.cmd}] ${ev.line}`); });
      if (gen !== this.runGen) return;
      unprogress = await api.onEfsProgress(ev => {
        if (gen === this.runGen && ev.total > 0) cur.progress = Math.min(0.99, progressBase + progressScale * ev.n / ev.total);
      });
      if (gen !== this.runGen) return;
      const showWarnings = (warnings: { code: string; target: string; message: string }[]) => {
        for (const w of warnings) this.log(cur, `[경고/${w.code}] ${w.target}: ${w.message}`);
      };
      if (cur.id === "efs-preflight") {
        const diag = await api.efsDiagOpen(this.device?.serial);
        if (gen !== this.runGen) return;
        if (!diag.ok) return this.failStep(diag.error);
        const r = await api.efsPreflight(cfg);
        if (gen !== this.runGen) return;
        if (!r.ok) return this.failStep(`${r.error} — DIAG 드라이버와 지정한 COM을 확인하세요. 포트가 나타나지 않으면 폰의 USB 모드를 MTP로 바꾼 뒤 충전 모드로 되돌리고 다시 시도하세요`);
        for (const line of r.value.log) this.log(cur, line);
        for (const w of r.value.warnings) this.log(cur, `[경고/setup] ${w}`);
        if (r.value.errors.length) return this.failStep(r.value.errors.join(" / "));
        this.markSub(cur, cur.sub?.list.length ?? 0);
      } else {
        const targets = this.volteConfig.sims.filter(s => s.carrier !== null).toSorted((a, b) => a.slot - b.slot);
        if (!targets.length) return this.failStep("패치 대상 SIM이 없습니다");
        for (const [index, target] of targets.entries()) {
          const preset = efsPreset(target.carrier!, target.slot);
          if (!preset) return this.failStep(`SIM${target.slot} 프리셋이 없습니다`);
          if (cur.id === "efs") {
            progressBase = index / targets.length;
            progressScale = 1 / (3 * targets.length);
            const dest = `${cfg.snapshotRoot}/${Date.now()}-sim${target.slot}`;
            const snap = await api.efsSnapshot(dest, preset.folder, cfg);
            if (gen !== this.runGen) return;
            if (!snap.ok) return this.failStep(snap.error);
            this.log(cur, `[before-image] SIM${target.slot}: ${snap.value.path}`);
            showWarnings(snap.value.warnings);
            for (let round = 1; round <= 2; round++) {
              progressBase = (index + round / 3) / targets.length;
              this.log(cur, `[EFS] SIM${target.slot} ${round}차 업로드`);
              const r = await api.efsUpload(preset.folder, cfg);
              if (gen !== this.runGen) return;
              if (!r.ok) return this.failStep(r.error);
              showWarnings(r.value.warnings);
              if (r.value.errors.length) return this.failStep(r.value.errors.join(" / "));
              this.log(cur, `[EFS] 쓴 항목 ${r.value.filesSeen}/${r.value.planned}, 건너뜀 ${r.value.skipped}`);
              this.markSub(cur, index * 2 + round);
            }
          } else {
            progressBase = index / targets.length;
            progressScale = 1 / targets.length;
            const r = await api.efsVerify(preset.folder, cfg);
            if (gen !== this.runGen) return;
            if (!r.ok) return this.failStep(r.error);
            showWarnings(r.value.warnings);
            if (!r.value.ok) return this.failStep(`SIM${target.slot} 리드백 실패: ${[...r.value.missing, ...r.value.mismatches].join(" / ")}`);
            this.log(cur, `[검증] SIM${target.slot} ${r.value.matched}/${r.value.files} 일치, 제외 ${r.value.skipped}`);
            this.markSub(cur, index + 1);
          }
          cur.progress = (index + 1) / targets.length;
          void this.persist(true);
        }
      }
      cur.progress = 1;
      this.stepDone(cur);
    } catch (e) {
      if (gen === this.runGen) this.failStep(`EFS 실행 실패: ${String(e)}`);
    } finally { unlog(); unprogress(); }
  }

  /** 단계 완료 공통 처리 — 시뮬레이션 tick의 완료 블록과 같은 규칙 */
  private stepDone(cur: RunStep) {
    if (cur.status === "done" || cur.status === "skipped") return;
    cur.status = "done";
    cur.progress = 1;
    this.log(cur, "[완료]");
    this.pause();
    this.cursor++;
    this.awaitingNext = this.cursor < this.runSteps.length ? cur.id : null;
    if (this.dangerBusy === 0) this.setGuard(false);
    if (this.cursor >= this.runSteps.length) this.complete();
    else { this.log(cur, `[대기] 다음 단계: ${this.nextStepTitle} — [다음]을 눌러 진행하세요`); void this.persist(true); }
  }

  /** 실전 복구 러너 — APK 재설치 → tar 스트리밍 → 설정 → 연락처 전송 후, 문자·통화(smsie)는 수동 개입 */
  private async runRealRestore(cur: RunStep) {
    const gen = this.runGen;
    if (!this.backupDir) {
      this.failStep("복구할 백업 폴더가 없습니다");
      return;
    }
    const items = this.checkedBackupItems();
    this.log(cur, `[실전] 복구 시작 — ${this.backupDir}`);
    const itemOrder = items.slice();
    const un = await api.onRestoreProgress((p) => {
      if (gen !== this.runGen) return;
      cur.progress = advance(cur.progress, itemProgress(p, itemOrder));
      this.transferStatus = transferStatusText(p);
      void this.persist();
    });
    if (gen !== this.runGen) return un();
    const r = await api.restoreRun(this.device?.serial, this.backupDir, items).finally(() => {
      un();
      this.transferStatus = "";
    });
    if (gen !== this.runGen) return; // 중단·처음으로
    if (!r.ok) {
      this.failStep(`복구 실패: ${r.error}`);
      return;
    }
    for (const line of r.value.logs) this.log(cur, `[복구] ${line}`);
    for (const fail of r.value.failures) this.log(cur, `[실패] ${fail}`);
    if (r.value.failures.length > 0) return this.failStep(`복구 미완료 — ${r.value.failures.join(" / ")}`);
    cur.progress = 0.99;
    void this.persist(true);
    // 폰에서 직접 해야 하는 복원 — 연락처 가져오기, 문자·통화 기록 순서
    const manuals: ManualId[] = [];
    if (items.includes("contacts") && !r.value.failures.some((f) => f.startsWith("연락처"))) manuals.push("contacts-import");
    if (r.value.smsiePending && this.needSmsie(items)) manuals.push("smsie-import");
    if (manuals.length > 0) {
      const stepDef = this.steps.find((s) => s.id === "restore");
      if (stepDef) stepDef.manual = manuals;
      return this.openEngineManual(cur, manuals[0]);
    }
    this.stepDone(cur);
  }

  /** 완결 게이트 — 백업을 선택한 계획에서 파괴 단계(언락/리락)는 완결(전수+오류0) 없이 진행하지 않는다 (§3-3)
   *  백업을 선택하지 않은 계획은 실행 전 이중 확인 모달(기존)으로 통과한다 */
  private async enforceBackupGate(cur: RunStep) {
    const gen = this.runGen;
    const resume = () => {
      cur.status = "running";
      this.begin();
    };
    if (!this.runSteps.some((s) => s.id === "backup")) {
      this.log(cur, "[게이트] 백업 선택 없음 — 실행 전 이중 확인으로 진행");
      return resume();
    }
    if (this.backupDir) {
      // 재개·이어받기 등: 기존 폴더를 파일 존재·크기·해시 대조로 재검사
      const runId=crypto.randomUUID(); this.backupRunId=runId;
      const un=await api.onBackupProgress(p=>{
        if (gen!==this.runGen || this.backupRunId!==runId) return;
        cur.progress=advance(cur.progress,itemProgress(p,this.checkedBackupItems(),"verify"));
        if (p.file && p.filesDone===p.filesTotal) this.log(cur,`[PC 검사] ${p.file}`);
      });
      if (gen!==this.runGen) {un();return;}
      const s=await api.backupManifestCheck(this.backupDir,runId).catch(error=>{
        if (gen===this.runGen) this.failStep(`백업 파일 검사 실패 — ${String(error)}`);
        return null;
      }).finally(()=>{un();if(this.backupRunId===runId)this.backupRunId=undefined;});
      if (gen !== this.runGen) return; // 그사이 중단됨
      const key = await this.deviceKeyHex();
      if (gen !== this.runGen) return;
      // 같은 기기 확인은 언제나 필수. 완결이 아니어도 사용자가 [이대로 진행]으로 받은 만큼을 인정했으면 통과한다
      if (s && (s.complete || this.backupIncompleteAccepted) && key && s.deviceKey?.toLowerCase() === key) {
        this.backupSummary = s;
        this.log(cur, s.complete
          ? "[게이트] 기존 백업 완결 재검사 통과(파일·해시 대조)"
          : `[게이트] 사용자가 불완전 백업을 인정하고 진행 — 백업되지 않은 항목 ${s.errors.length}건`);
        return resume();
      }
      const errs = s?.complete || this.backupIncompleteAccepted ?"백업 원본 기기와 현재 기기가 다르거나 식별 기록이 없습니다" : s?.errors.length ? s.errors.join(" / ") : "완결 아님";
      return this.failStep(`백업 완결 게이트 실패 — ${errs}. 백업 단계를 다시 진행해 주세요`);
    }
    this.failStep("백업이 완결되지 않아 파괴 단계를 진행할 수 없습니다 — 백업 단계를 먼저 끝내주세요");
  }

  /** 로그 이벤트를 구독한 채로 작업 — 성공·실패·예외 모두 구독을 해제한다. 구독하는 사이 중단되면 작업하지 않는다 */
  private async withLog<T>(
    gen: number,
    cur: RunStep,
    tag: string,
    subscribe: (cb: (line: string) => void) => Promise<() => void>,
    work: () => Promise<T>,
  ): Promise<T | undefined> {
    const un = await subscribe((line) => {
      if (gen !== this.runGen) return;
      this.log(cur, `[${tag}] ${line}`);
      void this.persist();
    });
    try {
      if (gen !== this.runGen) return undefined;
      return await work();
    } finally {
      un();
    }
  }

  private fastbootLog(gen: number, cur: RunStep, work: () => Promise<void>) {
    return this.withLog(gen, cur, "fastboot", (cb) => api.onFastbootLog(cb), work);
  }

  /** 실전 언락 — 사전 프로브(getvar) → oem unlock 0x{code} → getvar 이중 확인 (설계 .plans/04-engine/fastboot.md) */
  private async runRealUnlock(cur: RunStep) {
    const gen = this.runGen;
    if (!this.unlockCodeValid) {
      this.failStep("언락 코드가 입력되지 않았거나 형식(16자리 16진수)이 맞지 않습니다");
      return;
    }
    await this.fastbootLog(gen, cur, async () => {
      // 사전 프로브 — 모드·슬롯·언락 상태 확인(§10-2)
      cur.progress = 0.2;
      this.log(cur, "[실전] fastboot 연결 확인(getvar)");
      const vars = await api.fastbootGetvar();
      if (gen !== this.runGen) return;
      if (!vars) return this.failStep("fastboot 프로브 실패 — 폰이 부트로더 모드(파란 LED)인지 확인해 주세요");
      this.log(cur, `[fastboot] unlocked=${vars.unlocked ?? "?"} · slot=${vars["current-slot"] ?? "?"}`);
      const state = (vars.unlocked ?? "").trim().toLowerCase();
      if (state !== "yes" && state !== "no") return this.failStep("부트로더 잠금 상태를 확인할 수 없습니다 — 언락을 실행하지 않습니다");
      if ((vars["is-userspace"] ?? "").trim().toLowerCase() !== "no") {
        return this.failStep("부트로더 모드를 확인할 수 없습니다 — fastbootd에서는 언락할 수 없습니다");
      }
      if (state === "yes") {
        this.log(cur, "[확인] 이미 언락되어 있습니다 — OS 재부팅을 확인합니다");
        const rebooted = await api.fastbootReboot("os", this.device?.serial ?? "");
        if (gen !== this.runGen) return;
        if (!rebooted.ok) return this.failStep(`언락 상태지만 OS 재부팅을 확인하지 못했습니다 — 기기 상태를 확인해 주세요 (${rebooted.error})`);
        cur.progress = 1;
        return this.stepDone(cur);
      }
      cur.progress = 0.5;
      const r = await api.fastbootUnlock(this.normalizedUnlockCode, true, this.device?.serial ?? "");
      if (gen !== this.runGen) return;
      if (!r.ok) return this.failStep(`언락 실패: ${r.error}`);
      if (!r.value.unlocked) return this.failStep("언락 명령 후에도 unlocked=yes가 확인되지 않습니다 — 기기 상태를 확인해 주세요");
      const rebooted = await api.fastbootReboot("os", this.device?.serial ?? "");
      if (gen !== this.runGen) return;
      if (!rebooted.ok) return this.failStep(`언락은 확인됐지만 OS 재부팅을 확인하지 못했습니다 — 기기 상태를 확인해 주세요 (${rebooted.error})`);
      cur.progress = 1;
      this.log(cur, "[완료] 부트로더 언락 확인(unlocked=yes) — 기기가 초기화된 뒤 재부팅됩니다");
      this.stepDone(cur);
    });
  }

  /** 순정 부트 이미지 — 직접 지정한 폴더(검사 통과)가 우선, 없으면 자동 다운로드 결과 (root·unroot·relock 공용) */
  private stockSource(): { path: string; fingerprint: string } | null {
    const source = this.firmwareDirInfo ?? this.firmware;
    return source ? { path: source.path, fingerprint: source.fingerprint } : null;
  }

  private realBootFlowReady(): boolean {
    if (!REAL_STEPS.root || !REAL_STEPS.fastboot) {
      this.failStep("루팅·언루팅은 root와 fastboot 실전 실행이 모두 활성화되어야 합니다");
      return false;
    }
    if (this.steps.some(step => step.id === "fw-flash" && step.enabled)) {
      this.failStep("펌웨어 업데이트가 시뮬레이션인 상태에서는 새 버전의 부트 이미지를 실제로 기록할 수 없습니다");
      return false;
    }
    return true;
  }

  /** 리락 게이트 사전 점검용 deviceKey — fastboot serial과 같은 알고리즘(SHA-256 hex). 없으면 null */
  private async deviceKeyHex(): Promise<string | null> {
    const serial = this.device?.serial;
    return serial?.trim() ? sha256Hex(serial.trim()) : null;
  }

  /** adb로 부트로더 재부팅 → fastboot 모드 감지 대기. 실패 사유 또는 null (호출부에서 세대 확인) */
  private async enterFastboot(gen: number): Promise<string | null> {
    const rb = await api.rootReboot(this.device?.serial, "bootloader");
    if (gen !== this.runGen) return null;
    if (!rb.ok) return `부트로더 재부팅 실패: ${rb.error}`;
    const inFastboot = await this.waitFor(gen, () => this.usbModeIs("fastboot"), 90_000);
    if (gen !== this.runGen) return null;
    return inFastboot ? null : "부트로더 모드 진입이 감지되지 않습니다 — USB 연결을 확인해 주세요";
  }

  /** fastboot → OS 재부팅 → adb(USB 디버깅) 복귀 대기. 실패 사유 또는 null (호출부에서 세대 확인) */
  private async returnToAdb(gen: number, rebootFailed: string, notBack: string): Promise<string | null> {
    const rebooted = await api.fastbootReboot("os", this.device?.serial ?? "");
    if (gen !== this.runGen) return null;
    if (!rebooted.ok) return `${rebootFailed} (${rebooted.error})`;
    const back = await this.waitFor(gen, () => this.usbDebugReady(), 240_000);
    if (gen !== this.runGen) return null;
    return back ? null : notBack;
  }

  /** 실전 언루팅 — 원본 unRoot 계승: 순정 이미지 양 슬롯 기록 후 안내(앱 삭제는 수동) */
  private async runRealUnroot(cur: RunStep) {
    const gen = this.runGen;
    if (!this.realBootFlowReady()) return;
    const partition = this.partition;
    const stock = this.stockSource();
    if (!partition || !stock) {
      this.failStep("순정 부트 이미지가 준비되지 않았습니다 — 사전 준비에서 펌웨어를 먼저 받아 주세요");
      return;
    }
    const source = await api.bootImageCheck(this.device?.serial ?? "", stock.path, stock.fingerprint);
    if (gen !== this.runGen) return;
    if (!source.ok) return this.failStep(`부트 이미지 확인 실패: ${source.error}`);
    this.log(cur, `[언루팅] 순정 ${partition} 이미지로 복원합니다`);
    // 부트로더 진입 → 감지 대기
    cur.progress = 0.2;
    const fbError = await this.enterFastboot(gen);
    if (gen !== this.runGen) return;
    if (fbError) return this.failStep(fbError);
    // 순정 기록(양 슬롯) — 이 기록이 리락 게이트(§3-3)의 순정 증거가 된다
    cur.progress = 0.4;
    const flash = await this.withLog(gen, cur, "fastboot", (cb) => api.onFastbootLog(cb), () =>
      api.fastbootFlash(partition, stock.path, true, this.device?.serial ?? "", source.value),
    );
    if (!flash || gen !== this.runGen) return;
    if (!flash.ok) return this.failStep(`순정 이미지 기록 실패: ${flash.error}`);
    this.log(cur, `[언루팅] ${partition}_a/_b 순정 기록 완료`);
    void this.persist(true);
    // 재부팅 → 복귀 대기
    const backError = await this.returnToAdb(
      gen,
      "순정 기록 후 OS 재부팅을 확인하지 못했습니다 — 기기 상태를 확인해 주세요",
      "재부팅 후 기기 연결이 확인되지 않습니다 — 기록은 완료됐으므로 폰을 확인한 뒤 이 단계를 다시 시도해 주세요",
    );
    if (gen !== this.runGen) return;
    if (backError) return this.failStep(backError);
    const stillRooted = await api.rootCheck(this.device?.serial);
    if (gen !== this.runGen) return;
    if (stillRooted === true) return this.failStep("순정 기록 후에도 루트 권한이 남아 있습니다 — 이미지와 기기 상태를 확인해 주세요");
    if (stillRooted === null) this.log(cur, "[미확인] 루트 해제 상태를 자동으로 확인하지 못했습니다 — 폰의 Magisk에서 직접 확인해 주세요");
    cur.progress = 1;
    this.log(cur, "[완료] 순정 이미지 기록·재연결 — Magisk 앱을 열어 루트가 해제됐는지 확인한 뒤 앱을 직접 삭제해 주세요");
    this.log(cur, "[안내] Play 프로텍트 인증이 안 되면 Play 스토어 > 앱 정보 > 저장공간 > 데이터 삭제를 해주세요");
    this.stepDone(cur);
  }

  /** 실전 리락 — 현재 OS/순정 이미지 대조 → 양 슬롯 이력 → 부트로더 진입 → 잠금·상태 확인.
   *  백엔드는 추출 출처·이력을 다시 읽고 실제 fastboot serial/모드 기준으로 재검사한다. */
  private async runRealRelock(cur: RunStep) {
    const gen = this.runGen;
    const partition = this.partition;
    const stock = this.stockSource();
    if (!partition || !stock) {
      this.failStep("리락 게이트에 필요한 부트 파티션·순정 이미지가 준비되지 않았습니다 — 사전 준비에서 펌웨어를 먼저 받아 주세요");
      return;
    }
    const source = await api.bootImageCheck(this.device?.serial ?? "", stock.path, stock.fingerprint);
    if (gen !== this.runGen) return;
    if (!source.ok) return this.failStep(`리락 전 현재 펌웨어 확인 실패: ${source.error}`);
    await this.fastbootLog(gen, cur, async () => {
      // 사전 게이트 — 미충족 사유를 로그로 보여준다
      cur.progress = 0.3;
      const key = await this.deviceKeyHex();
      if (gen !== this.runGen) return;
      if (!key) return this.failStep("리락 게이트의 기기 식별값이 없습니다");
      const gateRes = await api.relockGateCheck(partition, stock.path, key);
      if (gen !== this.runGen) return;
      if (!gateRes.ok) return this.failStep(`리락 게이트 점검 실패: ${gateRes.error}`);
      const gate = gateRes.value;
      for (const c of gate.checked) this.log(cur, `[게이트] ${c.partition}${c.slot} — ${c.detail}`);
      if (!gate.ok) return this.failStep(`리락 게이트 실패 — ${gate.reasons.join(" / ")}`);
      const fbError = await this.enterFastboot(gen);
      if (gen !== this.runGen) return;
      if (fbError) return this.failStep(fbError);
      cur.progress = 0.6;
      const r = await api.fastbootLock(true, partition, stock.path, this.device?.serial ?? "");
      if (gen !== this.runGen) return;
      if (!r.ok) return this.failStep(`리락 실패: ${r.error}`);
      if (r.value.unlocked) return this.failStep("리락 후에도 unlocked=no가 확인되지 않습니다 — 기기 상태를 확인해 주세요");
      const rebooted = await api.fastbootReboot("os", this.device?.serial ?? "");
      if (gen !== this.runGen) return;
      if (!rebooted.ok) return this.failStep(`리락 후 OS 재부팅을 확인하지 못했습니다 — 기기 상태를 확인해 주세요 (${rebooted.error})`);
      cur.progress = 1;
      this.log(cur, "[완료] 부트로더 리락 확인(unlocked=no) — 기기가 초기화된 뒤 재부팅됩니다");
      this.stepDone(cur);
    });
  }

  /** 실전 루팅 — 검증된 절차(계약 root 절): 패치 → 부트로더 → 기록 → 복귀 → 앱 설치 → su 승인(수동) */
  private async runRealRoot(cur: RunStep) {
    const gen = this.runGen;
    if (!this.realBootFlowReady()) return;
    const partition = this.partition;
    if (!partition) {
      this.failStep("이 기종의 부트 파티션이 확인되지 않아 루팅할 수 없습니다");
      return;
    }
    // 순정 이미지 — 사전 준비(firmware_fetch) 결과 또는 직접 지정 폴더
    const stock = this.stockSource();
    if (!stock) {
      this.failStep("순정 부트 이미지가 준비되지 않았습니다 — 사전 준비 단계에서 펌웨어를 먼저 받아 주세요");
      return;
    }
    const { path: imagePath, fingerprint } = stock;
    const source = await api.bootImageCheck(this.device?.serial ?? "", imagePath, fingerprint);
    if (gen !== this.runGen) return;
    if (!source.ok) return this.failStep(`부트 이미지 확인 실패: ${source.error}`);
    // 1) Magisk APK 확보(캐시 재사용)
    cur.progress = 0.05;
    const prep = await api.magiskPrepare();
    if (gen !== this.runGen) return;
    if (!prep.ok) return this.failStep(`Magisk 다운로드 실패: ${prep.error}`);
    this.log(cur, `[루팅] Magisk ${prep.value.version} 준비 (sha256 ${prep.value.sha256.slice(0, 12)}…)`);
    this.markSub(cur, 1); // 받기
    void this.persist(true);
    // 2) 부트 패치(스테이징·스크립트·검증·수신)
    cur.progress = 0.15;
    const patch = await this.withLog(gen, cur, "magisk", (cb) => api.onMagiskLog(cb), () =>
      api.magiskPatch({
        serial: this.device?.serial ?? "", apkPath: prep.value.apkPath, imagePath, partition,
        imageSha256: source.value, fingerprint, apkSha256: prep.value.sha256,
      }),
    );
    if (!patch || gen !== this.runGen) return;
    if (!patch.ok) return this.failStep(`부트 패치 실패: ${patch.error}`);
    this.patchedImage = patch.value.path;
    this.log(cur, `[루팅] 패치 완료 — ${(patch.value.bytes / 1024 ** 2).toFixed(1)} MiB · 원본과 해시 상이 확인`);
    this.markSub(cur, 4); // 받기·전송·패치·결과 확인
    void this.persist(true);
    // 3) 부트로더 진입 → fastboot 감지 대기
    cur.progress = 0.5;
    this.log(cur, "[루팅] 부트로더 모드로 재부팅합니다");
    const fbError = await this.enterFastboot(gen);
    if (gen !== this.runGen) return;
    if (fbError) return this.failStep(fbError);
    // 4) 패치 이미지 기록(양 슬롯) — fastboot 엔진 재사용
    cur.progress = 0.7;
    const flash = await this.withLog(gen, cur, "fastboot", (cb) => api.onFastbootLog(cb), () =>
      api.fastbootFlash(partition, patch.value.path, true, this.device?.serial ?? "", patch.value.patchedSha256),
    );
    if (!flash || gen !== this.runGen) return;
    if (!flash.ok) return this.failStep(`부트 이미지 기록 실패: ${flash.error}`);
    this.log(cur, `[루팅] ${partition}_a/_b 기록 완료`);
    this.markSub(cur, 5); // 기록
    void this.persist(true);
    // 5) 재부팅 → adb 복귀 대기
    const backError = await this.returnToAdb(
      gen,
      "패치 기록 후 OS 재부팅을 확인하지 못했습니다 — 기기 상태를 확인해 주세요",
      "기기가 다시 연결되지 않습니다 — 재부팅 후 USB 디버깅 승인을 확인해 주세요",
    );
    if (gen !== this.runGen) return;
    if (backError) return this.failStep(backError);
    // 6) Magisk 앱 설치
    cur.progress = 0.9;
    const inst = await api.magiskInstall(this.device?.serial, prep.value.apkPath, prep.value.sha256);
    if (gen !== this.runGen) return;
    if (!inst.ok) return this.failStep(`Magisk 앱 설치 실패: ${inst.error}`);
    this.markSub(cur, 6); // 앱 설치
    void this.persist(true);
    // 7) su 승인(수동 개입) — 확인 버튼·자동 감지가 root_check로 검증
    this.openEngineManual(cur, "su-grant");
  }

  /** su 승인 후 최종 확인 — root_check로 uid=0 검증 */
  private async finishRealRoot(cur: RunStep) {
    const gen = this.runGen;
    const ok = await api.rootCheck(this.device?.serial);
    if (gen !== this.runGen) return;
    if (ok) {
      this.log(cur, "[완료] 루트 권한 확인(su -c id = uid=0)");
      cur.progress = 1;
      this.stepDone(cur);
    } else {
      this.failStep("루트 권한이 확인되지 않습니다 — 폰의 Magisk 권한 요청을 '허용'한 뒤 다시 확인해 주세요");
    }
  }

  /** 세부 작업 체크포인트를 done개까지 갱신(시뮬레이션·실전 루팅 공용) — 새로 끝난 항목이 있으면 true */
  private markSub(cur: RunStep, done: number): boolean {
    if (!cur.sub) return false;
    const reached = Math.min(cur.sub.list.length, done);
    if (reached <= cur.sub.done) return false;
    for (let k = cur.sub.done; k < reached; k++) this.log(cur, `[체크포인트] ${cur.sub.list[k]} 완료`);
    cur.sub.done = reached;
    return true;
  }

  /** 조건 폴링 대기 — 세대 가드·타임아웃·check 예외 방어(모드 전환 대기 공용) */
  private waitFor(gen: number, check: () => Promise<boolean>, timeoutMs: number, everyMs = 2000): Promise<boolean> {
    return waitUntil(check, () => gen === this.runGen, timeoutMs, everyMs);
  }

  /** smsie 수동 복원 마무리 — 기본 문자 앱 역할 원복 + 안내 로그 */
  private async finishRealRestore(cur: RunStep) {
    const gen = this.runGen;
    if (this.needSmsie(this.checkedBackupItems())) {
      const r = await api.smsieRestoreFinish(this.device?.serial);
      if (gen !== this.runGen) return;
      if (r.ok) {
        for (const line of r.value) this.log(cur, `[마무리] ${line}`);
      } else {
        return this.failStep(`문자 앱 원복 실패: ${r.error}`);
      }
    }
    cur.progress = 1;
    void this.persist(true);
    this.stepDone(cur);
  }

  // ── 단계 실패 — 실패 상태를 유지하고 다음 단계(리락 포함)로 넘어가지 않는다. 다시 시도 또는 중단만 허용
  stepError = $state("");
  failureSequence = $state(0);
  simulateEfsFail = $state(false);
  private efsFailedOnce = false;

  /** 실제 백엔드도 이 경로로 실패를 알린다 — 실패 파일·슬롯·시도 횟수·종료 코드·출력 요약을 reason과 로그에 남긴다 */
  failStep(reason: string) {
    const cur = this.runSteps[this.cursor];
    if (!cur) return;
    this.failureSequence++;
    cur.status = "failed";
    this.log(cur, `[실패] ${reason}`);
    this.stepError = reason;
    // 아직 끝나지 않은 같은 세대의 비동기 작업이 실패한 단계를 완료로 덮어쓰지 못하게 한다
    this.runGen++;
    this.pause();
    this.stopWatch();
    this.markStop(reason);
  }

  /** 실패한 단계를 처음부터 다시 */
  retryStep() {
    if (!this.readyToResume()) return;
    const cur = this.runSteps[this.cursor];
    if (!cur || cur.status !== "failed") return;
    cur.status = "pending";
    cur.progress = 0;
    cur.manualDone = 0;
    cur.communicationSkipped = false;
    cur.sub = undefined;
    this.clearRuntimeManuals(cur.id);
    this.log(cur, "[재시도] 이 단계를 처음부터 다시 진행합니다");
    this.stepError = "";
    this.awaitingNext = null;
    this.runGen++;
    this.begin();
  }

  /** 실패한 백업을 받은 만큼 인정하고 넘어갔는지 — 뒤의 초기화 게이트가 이 결정을 따른다(앱을 다시 켜면 초기화) */
  backupIncompleteAccepted = $state(false);
  /** [이대로 진행] — 백업 단계에만, 백업 폴더가 있을 때 */
  get canContinueAfterFailure(): boolean {
    const cur = this.runSteps[this.cursor];
    return cur?.id === "backup" && cur.status === "failed" && !!this.backupDir && this.busy === 0;
  }
  /** 남은 단계에 초기화(언락/리락)가 있는지 — 있으면 화면이 한 번 더 확인한다 */
  get wipeAhead(): boolean {
    return this.runSteps.slice(this.cursor + 1).some((s) => s.id === "unlock" || s.id === "relock");
  }
  /** 실패한 백업을 받은 만큼으로 마치고 다음으로 — 빠진 항목은 로그에 남긴다 */
  continueAfterFailure() {
    if (!this.canContinueAfterFailure) return;
    const cur = this.runSteps[this.cursor];
    const missing = this.backupSummary?.errors ?? [];
    this.backupIncompleteAccepted = true;
    this.log(cur, `[계속] 백업되지 않은 항목이 있는 채로 진행합니다${missing.length ? ` — ${missing.slice(0, 5).join(" / ")}${missing.length > 5 ? ` 외 ${missing.length - 5}건` : ""}` : ""}`);
    this.stepError = "";
    this.stepDone(cur);
  }

  get corruptRelockHistory(): boolean {
    return this.stepError.includes("FLASH_HISTORY_CORRUPT|");
  }
  get displayedStepError(): string { return this.stepError.replaceAll("FLASH_HISTORY_CORRUPT|", ""); }
  get backupPermissionBlocked(): boolean {
    return this.runSteps.some(step => step.id === "backup" && step.status === "failed") &&
      this.stepError.startsWith("백업 미완결") && !!this.backupSummary?.errors.length &&
      this.backupSummary.errors.every(error => /permission denied/i.test(error));
  }

  async archiveFlashHistory() {
    if (!this.corruptRelockHistory || this.busy > 0 || this.runInDanger) return;
    const gen = this.runGen;
    const result = await this.track(api.flashHistoryArchive(true));
    if (gen !== this.runGen) return;
    if (!result.ok) { this.stepError = `FLASH_HISTORY_CORRUPT|${result.error}`; return; }
    const index = this.runSteps.findIndex(s => s.id === "unroot");
    this.log(this.runSteps[this.cursor], `[이력 보관] ${result.value}`);
    if (index < 0) {
      this.stepError = "손상 이력을 보관했습니다 — 순정 양 슬롯을 다시 기록하는 계획을 만들어 주세요";
      return;
    }
    for (let i = index; i <= this.cursor; i++) {
      Object.assign(this.runSteps[i], { status: "pending", progress: 0, manualDone: 0, sub: undefined });
    }
    this.cursor = index;
    this.awaitingNext = null;
    this.stepError = "";
    this.runGen++;
    this.log(this.runSteps[index], "[안내] 폰을 OS로 부팅해 USB 디버깅을 연결한 다음 [이어서]를 누르세요 — 순정 복원과 양 슬롯 검증을 다시 진행합니다");
    await this.persist(true);
  }

  /** 통신 확인 실패 시 VoLTE 적용부터 다시 (리락 전이라 루트가 남아 있음) */
  repatch() {
    if (!this.readyToResume()) return;
    const idx = this.runSteps.findIndex((s) => s.id === "efs-preflight" || s.id === "efs");
    if (idx < 0 || idx > this.cursor) return;
    for (let k = idx; k <= this.cursor; k++) {
      const st = this.runSteps[k];
      st.status = "pending";
      st.progress = 0;
      st.manualDone = 0;
      st.communicationSkipped = false;
      st.sub = undefined;
      this.clearRuntimeManuals(st.id);
    }
    this.log(this.runSteps[idx], "[재패치] 통신이 확인되지 않아 VoLTE 적용부터 다시 진행합니다");
    this.stopWatch();
    this.manualCurrent = null;
    this.manualCheckError = "";
    this.cursor = idx;
    this.awaitingNext = null;
    this.runGen++;
    void this.persist(true);
    this.begin();
  }

  dismissUsbError() {
    if (!this.readyToResume()) return;
    this.usbError = false;
    const cur = this.runSteps[this.cursor];
    if (cur) { cur.status = "running"; this.log(cur, "[재개] 재연결 확인 — 이어서 진행합니다"); }
    this.begin();
  }

  /** 실행을 멈춘 뒤 돌아오더라도 running/manual-wait가 남아 단계를 건너뛰지 않게 한다. */
  private stopRun(resetFailed: boolean) {
    void api.efsCancel();
    this.runGen++;
    this.pause();
    this.stopWatch();
    // 진행 중인 실전 백업이 있으면 백엔드에도 취소 전달(시작 직전이면 시작 즉시 멈춘다)
    void api.backupCancel(this.backupRunId);
    // 진행 중·수동 대기 단계 모두 대기 상태로 (사이드바 스피너가 남지 않도록), 수동 개입은 처음부터 다시
    this.runSteps.forEach((s) => {
      if (s.status === "running" || s.status === "manual-wait") {
        s.status = "pending";
        s.manualDone = 0;
      } else if (resetFailed && s.status === "failed") {
        // 실패한 단계는 다시 시작할 때 처음부터
        s.status = "pending";
        s.progress = 0;
        s.manualDone = 0;
        s.sub = undefined;
      }
      if (s.status === "pending") this.clearRuntimeManuals(s.id);
    });
    this.manualCurrent = null;
    this.usbError = false;
    this.backupNoticeAck = false;
  }

  /** 엔진이 추가한 안내는 재시도 시 엔진 준비가 끝난 뒤 다시 표시한다. */
  private clearRuntimeManuals(id: string) {
    const runtime: Partial<Record<string, ManualId[]>> = {
      root: ["su-grant"], backup: ["smsie-export"], restore: ["contacts-import", "smsie-import"],
    };
    const step = this.steps.find(step => step.id === id);
    if (step?.manual && runtime[id]) step.manual = step.manual.filter(item => !runtime[id]!.includes(item));
    if (id === "root") this.patchedImage = "";
  }

  abort() {
    this.stepError = "";
    this.stopRun(true);
    this.markStop("사용자가 작업을 중단했습니다");
  }

  goFinish() {
    this.view = "step4";
  }

  /** [처음으로] — 모든 선택·입력·실행 상태 초기화 */
  restart() {
    this.resetSession();
    this.view = "device";
    this.device = null;
    this.env = [];
    this.groups = [];
    this.opts = { unroot: false, relock: false, restore: true };
    this.backupPath = "";
    this.sizes = null;
    this.sizesState = "idle";
    this.appClasses = null;
    this.appClassesState = "idle";
    this.settingsInfo = null;
    this.settingsInfoState = "idle";
    this.optionsFor = null;
    this.fwVersions = null;
    this.fwVersionsState = "idle";
    this.fwVersionsFor = null;
    this.sessionFor = null;
  }

  private complete() {
    this.pause();
    this.finished = true;
    this.setGuard(false);
    void this.persist(true);
  }

  get overall(): number {
    if (!this.runSteps.length) return 0;
    const total = this.runSteps.length;
    const done = this.runSteps.filter((s) => s.status === "done").length;
    const cur = this.runSteps.find((s) => s.status === "running" || s.status === "manual-wait");
    return (done + (cur?.progress ?? 0)) / total;
  }
}

export const wizard = new Wizard();
