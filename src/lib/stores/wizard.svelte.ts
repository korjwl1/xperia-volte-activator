// 위자드 상태 머신 + 실행 시뮬레이션 러너 (mock)
import type { FirmwareDirInfo, AppItem, FirmwareResult, FirmwareVersions, SettingsOverview, SimInfo, BackupGroup, DeviceStatus, EnvCheckItem, ManualId, ManualPrompt, PlanStep, RunJournal, RunStep, VolteConfig } from "$lib/types";
import { api } from "$lib/api";
import { LINKS, maskSecret } from "$lib/data/links";
import { bootPartition } from "$lib/data/devices";
import { mockBackupGroups } from "$lib/mock/apps";
import { bootloaderOnly, buildPlan, updateTarget, type PlanOptions } from "$lib/domain/plan";
import { SIMULATED_RUN, REAL_STEPS } from "$lib/data/runMode";
import { EFS_PRESET_MODE, EFS_PRESET_VERSION, efsPreset } from "$lib/data/efsPresets";
import type { BackupSummary } from "$lib/types";
import { AsyncQueue } from "$lib/domain/asyncQueue";
import { decodeJournal } from "$lib/domain/journal";
import { waitUntil } from "$lib/domain/waitUntil";

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
    title: "리락 전 통신 확인",
    steps: [
      "재부팅 후 통신사 신호가 잡히면 앱이 VoLTE(IMS) 등록을 확인합니다",
      "다른 전화로 실제로 걸고 받아 통화가 되는지 확인해 주세요 — 리락한 뒤에는 다시 고치려면 초기화가 한 번 더 필요합니다",
      "통화가 안 되면 [다시 패치]로 VoLTE 적용부터 다시 진행할 수 있습니다",
    ],
  },
  "ims-check": {
    title: "최종 VoLTE 확인",
    steps: [
      "폰이 완전히 부팅되고 통신사 신호를 잡을 때까지 기다립니다 (2~3분)",
      "앱이 IMS(VoLTE) 등록 상태를 자동으로 확인합니다 — 등록되면 바로 다음 단계로 진행됩니다",
      "직접 확인하려면: 전화 앱 → *#*#4636#*#* → 휴대전화 정보 → IMS 서비스 상태",
    ],
  },
  "smsie-export": {
    title: "문자·통화 기록 내보내기 (폰 조작)",
    steps: [
      "SMS Import/Export 앱 설치·권한은 자동으로 진행됩니다",
      "폰의 SMS Import/Export 앱 → 'Export messages'와 'Export call logs'를 각각 누릅니다",
      "저장 위치 선택 화면에서 xvolte-smsie 폴더(자동 생성됨)를 고릅니다",
      "내보내기가 끝나면 자동으로 감지됩니다",
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
      "SMS Import/Export 앱 → 'Import messages' / 'Import call logs' → xvolte-smsie 폴더의 파일 선택",
      "가져오기가 끝나면 [확인하고 진행]을 누릅니다 — 앱이 기본 문자 앱에서 원래대로 돌아갑니다",
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

export class Wizard {
  view = $state<WizardView>("device");
  device: DeviceStatus | null = $state(null);
  env: EnvCheckItem[] = $state([]);
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
    return this.hasPatchTarget || this.updateVersion !== null || this.bootloaderOnly !== null;
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
      this.pause();
      this.stopWatch();
      this.setGuard(false);
      this.runGen++;
      this.backupDir = "";
      this.patchedImage = "";
      this.backupSummary = null;
      this.runSteps = [];
      this.steps = [];
      this.journalKey = null;
      this.pendingJournal = null;
      this.stepError = "";
      this.finished = false;
      this.sessionFor = key;
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
    this.view = "warning";
  }

  // 경고 페이지 동의 (뒤로 왔다 다시 와도 유지, 처음으로 가면 초기화)
  omdAck = $state(false);
  riskAck = $state(false);

  // ── 2단계(작업 옵션 선택) — 이전/다음 이동 시 유지, 다른 기기거나 처음으로 가면 초기화 ──
  groups: BackupGroup[] = $state([]); // 백업 항목 (항목 단위 checked)
  opts = $state<PlanOptions>({ unroot: false, relock: false, restore: true });
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
  ensureOptions() {
    const d = this.device;
    if (!d) return;
    // 작업 종류(부트로더만 작업 여부)가 바뀌면 기본 선택을 다시 만든다
    const key = `${d.serial ?? d.serialMasked}|${this.bootloaderOnly ?? ""}`;
    if (this.optionsFor === key) return;
    this.optionsFor = key;
    // 초기화 경로(잠긴 기기의 언락, 부트로더만 언락/리락)가 있으면 백업 기본 전체 선택 — 패치 흐름의 리락은 기본 해제
    const defaultOn = (this.hasPatchTarget && d.bootloader === "locked") || this.bootloaderOnly !== null;
    this.groups = mockBackupGroups.map((g) => ({
      ...g,
      items: g.items.map((i) => ({ ...i, checked: defaultOn })),
    }));
    this.opts = { unroot: false, relock: false, restore: defaultOn };
    void this.loadMeasurements(key, d.serial);
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
    return this.device ? bootPartition(this.device.model) : null;
  }

  // ── 실행 상태 ──
  steps: PlanStep[] = $state([]);
  runSteps: RunStep[] = $state([]);
  running = $state(false);
  finished = $state(false);
  manualCurrent: ManualPrompt | null = $state(null);
  /** 실전 백업 결과(완결 게이트·복구에 사용) — REAL_STEPS.backup 전환 시에만 채워짐 */
  backupSummary: BackupSummary | null = $state(null);
  /** 실전 백업이 만든 폴더(manifest 위치) — 재시도 이어받기·복구·journal에 저장 */
  backupDir = $state("");
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
  /** 이번 세대에 실전 백업/복구 러너를 시작했는지(재시도·재개 시 다시 돌도록 -1로 리셋) */
  private backupRanGen = -1;
  private restoreRanGen = -1;
  private unlockRanGen = -1;
  private relockRanGen = -1;
  private rootRanGen = -1;
  private unrootRanGen = -1;
  private efsPreflightRanGen = -1;
  private efsRanGen = -1;
  private efsVerifyRanGen = -1;
  private timer: ReturnType<typeof setInterval> | undefined;
  private cursor = 0;

  get macroStepIdx(): number {
    return MACRO_STEPS.findIndex((s) => s.view === this.view);
  }

  /** 2단계 [실행] 확정 — 현재 계획으로 실행 시작 */
  launch() {
    this.steps = this.plan;
    this.view = "step3";
    this.prepareRun();
    this.journalStarted = new Date().toISOString();
    this.journalSims = this.simSnapshot();
    void this.journalKeyReady().then(() => this.persist(true));
    this.begin();
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
    const buf = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(`${d.model}|${d.serial}`));
    if (gen !== this.runGen || this.device?.serial !== d.serial) return null;
    this.journalKey = [...new Uint8Array(buf)].slice(0, 16).map((b) => b.toString(16).padStart(2, "0")).join("");
    return this.journalKey;
  }

  /** 경고 페이지 [다음] — 같은 폰의 끝나지 않은 작업이 있으면 pendingJournal에 두고 true */
  async checkJournal(): Promise<boolean> {
    const gen = this.runGen;
    const key = await this.journalKeyReady();
    if (!key) return false;
    const raw = await api.journalLoad(key);
    if (!raw || gen !== this.runGen) return false;
    const journal = decodeJournal(raw);
    if (!journal || journal.model !== this.device?.model || journal.serialMasked !== this.device?.serialMasked) return false;
    this.pendingJournal = journal;
    return true;
  }

  /** [새로 시작] — 이전 기록은 discarded로 보관하고 1단계부터 */
  discardJournal() {
    const key = this.journalKey;
    if (key) void this.journalWrites.push(() => api.journalArchive(key, "discarded"));
    this.pendingJournal = null;
    this.backupDir = "";
    this.patchedImage = "";
    this.backupSummary = null;
    this.view = "step1";
  }

  /** [이어서 진행] — 선택했던 옵션·진행 상황을 되살리고 실행 화면으로 (자동 시작하지 않음) */
  resumeJournal() {
    const j = this.pendingJournal;
    if (!j) return;
    this.runGen++;
    this.stepError = "";
    this.manualCurrent = null;
    this.stopWatch();
    this.volteConfig = { ...defaultVolteConfig(), ...j.config };
    this.ensureOptions();
    for (const g of this.groups) for (const i of g.items) i.checked = j.backupItems.includes(i.id);
    this.opts = { ...j.opts };
    this.backupPath = j.backupPath;
    this.backupDir = j.backupDir ?? "";
    this.patchedImage = j.patchedImage ?? "";
    this.backupSummary = null; // 이어서 진행 시 백업 결과는 단계 재검증으로 다시 채운다
    this.firmware = j.firmware;
    this.firmwareState = j.firmware ? "done" : "idle";
    this.firmwareDir = j.firmwareDir;
    this.imsUnverified = j.imsUnverified ?? false;
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
    if (this.simChangedSince(j)) {
      for (const st of runSteps) {
        if ((st.id === "comm-check" || st.id === "final-verify") && st.status === "done") {
          st.status = "pending";
          st.progress = 0;
          st.manualDone = 0;
          st.logs.push("[재개] 작업 시작 때와 SIM 구성이 달라 통신 확인을 다시 진행합니다");
        }
      }
    }
    this.journalSims = j.sims;
    const first = runSteps.findIndex((st) => st.status !== "done" && st.status !== "skipped");
    if (first >= 0) {
      const sub = runSteps[first].sub;
      runSteps[first].logs.push(
        sub && sub.done > 0
          ? `[재개] 이전 진행 기록을 불러왔습니다 — '${sub.list[sub.done - 1]}'까지 끝났으므로 '${sub.list[sub.done] ?? "마무리"}'부터 진행합니다`
          : "[재개] 이전 진행 기록을 불러왔습니다 — 이 단계부터 다시 진행합니다",
      );
    }
    this.runSteps = runSteps;
    for (const step of runSteps) if (step.status === "pending") this.clearRuntimeManuals(step.id);
    this.cursor = first >= 0 ? first : runSteps.length;
    this.journalStarted = j.startedAt;
    this.stopInfo = null;
    this.finished = false;
    this.usbError = false;
    this.erroredOnce = false;
    this.pendingJournal = null;
    this.view = "step3";
    void this.persist(true);
  }

  /** 진행 기록 저장 — 상태 변화 시 즉시, 진행률·로그는 2초 간격 */
  private persist(force = false): Promise<boolean> {
    const key = this.journalKey;
    const d = this.device;
    if (!key || !d || this.runSteps.length === 0) return Promise.resolve(false);
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
      runSteps: this.runSteps.map((st) => ({ ...$state.snapshot(st), logs: st.logs.slice(-300) })),
      cursor: this.cursor,
      firmware: this.firmware ? { ...this.firmware } : null,
      firmwareDir: this.firmwareDir,
      imsUnverified: this.imsUnverified,
      sims: this.journalSims,
      stop: this.stopInfo,
    };
    const data = JSON.stringify(j);
    return this.journalWrites.push(() => api.journalSave(key, data));
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
      this.runSteps[this.cursor]?.logs.push("[경고] PC 절전·종료 방지를 켜지 못했습니다 — 작업이 끝날 때까지 PC가 잠들거나 꺼지지 않게 해 주세요");
    });
  }

  /** Windows 종료 요청 (guard.rs 이벤트) — query: 종료 보류 중 기록 저장 / end: 그래도 종료됨 */
  onSessionEnd(kind: string) {
    if (!this.runUnfinished) return;
    const cur = this.runSteps[this.cursor];
    if (kind === "end" && cur && !this.stopInfo) {
      this.stopInfo = { stepId: cur.id, stepTitle: cur.title, reason: "Windows 종료로 프로그램이 종료되었습니다", at: new Date().toISOString() };
      cur.logs.push(`[종료] ${this.stopInfo.reason}`);
    } else if (kind === "query" && cur) {
      cur.logs.push("[경고] Windows 종료 요청을 보류했습니다 — 작업이 끝날 때까지 PC를 끄지 마세요");
    }
    void this.persist(true);
  }

  /** 끝나지 않은 실행이 있는지 — 창을 닫을 때 확인 */
  get runUnfinished(): boolean {
    return this.view === "step3" && this.runSteps.length > 0 && !this.finished;
  }

  /** 지금 진행 중인 단계가 되돌리기 어려운 작업인지 (창 닫기 경고용) */
  get runInDanger(): boolean {
    const cur = this.runSteps[this.cursor];
    return !!cur && cur.status === "running" && this.steps.find((s) => s.id === cur.id)?.risk === "danger";
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
      cur.logs.push(`[종료] ${this.stopInfo.reason}`);
    }
    try {
      return await this.persist(true);
    } finally {
      this.setGuard(false);
    }
  }

  /** 명시적으로 멈춘 사유를 기록 */
  private markStop(reason: string) {
    const cur = this.runSteps[this.cursor];
    this.stopInfo = cur ? { stepId: cur.id, stepTitle: cur.title, reason, at: new Date().toISOString() } : null;
    this.setGuard(false);
    void this.persist(true);
  }

  // ── 실행 시뮬레이션 ───────────────────
  prepareRun() {
    this.runGen++;
    this.patchedImage = "";
    this.runSteps = this.steps
      .filter((s) => s.enabled)
      .map((s) => ({ id: s.id, title: s.title, status: "pending", progress: 0, logs: [], manualDone: 0 }));
    this.cursor = 0;
    this.finished = false;
    this.usbError = false;
    this.erroredOnce = false;
    this.efsFailedOnce = false;
    this.stepError = "";
  }

  begin() {
    if (this.running || this.usbError || this.stepError) return;
    this.running = true;
    this.stopInfo = null;
    this.setGuard(true);
    this.timer = setInterval(() => this.tick(), 140);
  }

  pause() {
    this.running = false;
    if (this.timer) clearInterval(this.timer);
    this.timer = undefined;
  }

  private dispatchEngine(work: () => Promise<void>) {
    const gen = this.runGen;
    void work().catch(error => {
      if (gen === this.runGen) this.failStep(String(error));
    });
  }

  private tick() {
    while (this.runSteps[this.cursor] && ["done", "skipped"].includes(this.runSteps[this.cursor].status)) this.cursor++;
    const cur = this.runSteps[this.cursor];
    if (!cur) return this.complete();
    if (cur.status === "pending") {
      cur.status = "running";
      cur.logs.push(`[시작] ${cur.title}`);
      const list = this.subtasksFor(cur.id);
      if (list.length > 0 && !cur.sub) cur.sub = { list, done: 0 };
      // 완결 게이트 (§3-3) — 파괴 단계(언락/리락) 직전, 실전 모드에서만 백업 완결을 강제한다
      if ((cur.id === "unlock" || cur.id === "relock") && (REAL_STEPS.backup || REAL_STEPS.fastboot)) {
        this.pause();
        void this.enforceBackupGate(cur);
        return;
      }
      if (cur.id === "efs") {
        for (const t of this.volteConfig.sims.filter((x) => x.carrier !== null)) {
          const p = efsPreset(t.carrier!, t.slot);
          cur.logs.push(
            p
              ? `[프리셋] SIM${t.slot} ${t.carrier} · ${EFS_PRESET_VERSION} ${EFS_PRESET_MODE} · 파일 ${p.files}개 · sha256 ${p.sha256.slice(0, 12)}…`
              : `[프리셋] SIM${t.slot} ${t.carrier} — 번들 프리셋을 찾을 수 없습니다`,
          );
        }
      }
      void this.persist(true);
    }
    if (this.simulateEfsFail && !this.efsFailedOnce && cur.id === "efs" && cur.progress > 0.5) {
      this.efsFailedOnce = true;
      this.failStep("EFS 업로드 실패 — SIM1 2차 업로드, 도구 종료 코드 1 (시뮬레이션)");
      return;
    }
    if (this.simulateUsbError && !this.erroredOnce && (cur.id === "backup" || cur.id === "efs")) {
      this.erroredOnce = true;
      this.usbErrorCount++;
      this.usbError = true;
      this.pause();
      cur.logs.push("[오류] DIAG 전송 타임아웃 — USB 연결이 불안정합니다 (재시도 카운트 3/3)");
      this.markStop("USB 연결 오류 — DIAG 전송 타임아웃");
      return;
    }
    // 혼합 모드에서 실전 기록 뒤 시뮬레이션 완료로 넘어갈 수 없도록 먼저 차단한다.
    if ((cur.id === "root" || cur.id === "unroot") && (REAL_STEPS.root || REAL_STEPS.fastboot) && !this.realBootFlowReady()) return;
    // 리락은 이력만으로 안전성을 증명할 수 없어 모드 전환 전에도 차단한다.
    if (cur.id === "relock" && REAL_STEPS.fastboot) return this.failStep("리락은 순정 출처·AVB·전체 부트 체인 검증이 구현될 때까지 차단됩니다");
    const step = this.steps.find((s) => s.id === cur.id);
    const manuals = step?.manual ?? [];
    if (cur.manualDone < manuals.length) {
      const id = manuals[cur.manualDone];
      cur.status = "manual-wait";
      this.pause();
      void this.openManual(cur, id);
      return;
    }
    // 실전 백업 단계 — 시뮬레이션 대신 백엔드 엔진이 상태를 바꾼다 (REAL_STEPS 전환 시)
    if (cur.id === "backup" && REAL_STEPS.backup) {
      if (cur.status === "running") {
        if (this.backupRanGen !== this.runGen) {
          this.backupRanGen = this.runGen;
          this.pause();
          this.dispatchEngine(() => this.runRealBackup(cur));
        } else {
          // 문자·통화 기록(smsie) 수동 완료 후 재진입 — 완결 판정으로 마무리
          this.pause();
          this.finishRealBackup(cur);
        }
      }
      return;
    }
    if (cur.id === "restore" && REAL_STEPS.restore) {
      if (cur.status === "running") {
        if (this.restoreRanGen !== this.runGen) {
          this.restoreRanGen = this.runGen;
          this.pause();
          this.dispatchEngine(() => this.runRealRestore(cur));
        } else {
          // smsie 수동 복원 완료 후 재진입 — 역할 원복·마무리
          this.pause();
          this.dispatchEngine(() => this.finishRealRestore(cur));
        }
      }
      return;
    }
    // 실전 언락/리락 — fastboot 엔진 (REAL_STEPS.fastboot 전환 시)
    if (cur.id === "unlock" && REAL_STEPS.fastboot) {
      if (cur.status === "running" && this.unlockRanGen !== this.runGen) {
        this.unlockRanGen = this.runGen;
        this.pause();
        this.dispatchEngine(() => this.runRealUnlock(cur));
      }
      return;
    }
    if (cur.id === "relock" && REAL_STEPS.fastboot) {
      if (cur.status === "running" && this.relockRanGen !== this.runGen) {
        this.relockRanGen = this.runGen;
        this.pause();
        this.dispatchEngine(() => this.runRealRelock(cur));
      }
      return;
    }
    // 실전 언루팅 — 순정 재기록(root_reboot + fastboot_flash 조합) — 두 엔진 모두 켜져야 실행
    if (cur.id === "unroot" && REAL_STEPS.root && REAL_STEPS.fastboot) {
      if (cur.status === "running" && this.unrootRanGen !== this.runGen) {
        this.unrootRanGen = this.runGen;
        this.pause();
        this.dispatchEngine(() => this.runRealUnroot(cur));
      }
      return;
    }
    // 실전 루팅 — Magisk 엔진 (REAL_STEPS.root 전환 시). 2단계: 패치·기록·설치 → su 승인 검증
    if (cur.id === "root" && REAL_STEPS.root) {
      if (cur.status === "running") {
        if (this.rootRanGen !== this.runGen) {
          this.rootRanGen = this.runGen;
          this.pause();
          this.dispatchEngine(() => this.runRealRoot(cur));
        } else {
          // su-grant 수동 완료 후 재진입 — 최종 확인
          this.pause();
          this.dispatchEngine(() => this.finishRealRoot(cur));
        }
      }
      return;
    }
    // 실전 EFS — 래퍼 엔진 (REAL_STEPS.efs 전환 시). efs(슬롯별 2회 업로드) → verify(전수 리드백)
    if (cur.id === "efs-preflight" && REAL_STEPS.efs) {
      if (cur.status === "running" && this.efsPreflightRanGen !== this.runGen) {
        this.efsPreflightRanGen = this.runGen;
        this.pause();
        this.dispatchEngine(() => this.runRealEfsPreflight(cur));
      }
      return;
    }
    if (cur.id === "efs" && REAL_STEPS.efs) {
      if (cur.status === "running" && this.efsRanGen !== this.runGen) {
        this.efsRanGen = this.runGen;
        this.pause();
        this.dispatchEngine(() => this.runRealEfsUpload(cur));
      }
      return;
    }
    if (cur.id === "verify" && REAL_STEPS.efs) {
      if (cur.status === "running" && this.efsVerifyRanGen !== this.runGen) {
        this.efsVerifyRanGen = this.runGen;
        this.pause();
        this.dispatchEngine(() => this.runRealEfsVerify(cur));
      }
      return;
    }
    cur.progress = Math.min(1, cur.progress + 0.04 + Math.random() * 0.05);
    if (Math.random() < 0.35) cur.logs.push(this.mockLog(cur.id, cur.progress));
    if (cur.sub) {
      const reached = Math.min(cur.sub.list.length, Math.floor(cur.progress * cur.sub.list.length + 1e-9));
      if (reached > cur.sub.done) {
        for (let k = cur.sub.done; k < reached; k++) cur.logs.push(`[체크포인트] ${cur.sub.list[k]} 완료`);
        cur.sub.done = reached;
        void this.persist(true);
      }
    }
    if (cur.progress >= 1) {
      cur.status = "done";
      cur.logs.push("[완료]");
      this.cursor++;
      if (this.cursor >= this.runSteps.length) return this.complete();
      void this.persist(true);
      return;
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
      case "efs-preflight": return ["USB 연결 확인", "드라이버 확인", "전원 관리 해제", "연결 안정성 테스트"];
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
      case "efs-preflight": return ["USB 연결 확인", "드라이버 확인", "전원 관리 일시 해제", "연결 안정성 테스트 통과"][Math.floor(p * 4) % 4];
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
    const gen = this.runGen;
    const list = await api.deviceList();
    const d = list?.find((x) => x.state === "device" && x.serial === this.device?.serial);
    if (!d || gen !== this.runGen) return false;
    this.imsSims = d.sims;
    const targets = this.volteConfig.sims.filter((s) => s.carrier !== null).map((s) => s.slot);
    const slots = targets.length > 0 ? targets : d.sims.filter((s) => s.carrier).map((s) => s.slot);
    return slots.length > 0 && slots.every((slot) => d.sims.find((s) => s.slot === slot)?.volte === "on");
  }

  /** 최종 확인 중 표시할 슬롯별 VoLTE 상태 */
  imsSims: SimInfo[] = $state([]);

  /** 자동 감지 중인 항목 설명 (모달에 표시) */
  manualWatching = $state("");
  private watchTimer: ReturnType<typeof setInterval> | undefined;
  private watchGeneration = 0;

  /** 자동 감지: 조건이 충족될 때까지 주기적으로 확인 → 충족되면 자동 진행 (수동 [완료]도 가능) */
  private watchManual(cur: RunStep, id: ManualId, label: string, check: () => Promise<boolean>, everyMs: number) {
    this.stopWatch();
    this.manualWatching = label;
    const watch = this.watchGeneration;
    const gen = this.runGen;
    let busy = false;
    this.watchTimer = setInterval(async () => {
      if (watch !== this.watchGeneration) return;
      if (this.manualCurrent?.id !== id || gen !== this.runGen) return this.stopWatch();
      if (busy) return;
      busy = true;
      try {
        const ok = await check();
        if (watch !== this.watchGeneration || gen !== this.runGen || this.manualCurrent?.id !== id) return; // 그사이 중단·진행됨
        if (ok) {
          this.stopWatch();
          cur.logs.push(`[감지] ${label} — 자동으로 진행합니다`);
          this.ackManual();
        }
      } catch {
        if (watch === this.watchGeneration && gen === this.runGen && this.manualCurrent?.id === id) this.manualCheckError = "기기 확인에 실패했습니다 — 연결 상태를 확인하고 다시 시도해 주세요";
      } finally {
        busy = false;
      }
    }, everyMs);
  }

  private stopWatch() {
    this.watchGeneration++;
    if (this.watchTimer) clearInterval(this.watchTimer);
    this.watchTimer = undefined;
    this.manualWatching = "";
  }

  /** 수동 개입 시작 — 이미 충족이면 안내 없이 진행, 자동 감지 가능한 항목은 감지되면 자동 진행 */
  private async openManual(cur: RunStep, id: ManualId) {
    const gen = this.runGen;
    const usbReady = id === "usb-debug" && (await this.usbDebugReady());
    if (gen !== this.runGen) return; // 확인하는 사이 중단됨
    if (usbReady) {
      cur.logs.push("[확인] USB 디버깅 연결 확인됨 — 자동으로 진행합니다");
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
        cur.logs.push(`[자동] Sony 서버에서 순정 펌웨어의 ${this.partition ?? "부트"} 이미지 받는 중`);
        await this.fetchFirmware();
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
      cur.logs.push(`[실패] 순정 펌웨어 자동 다운로드: ${this.firmwareError}`);
    }
    this.manualCheckError = "";
    this.oemUnknownAck = false;
    this.callAck = false;
    this.manualCurrent = { id, ...MANUAL_TEXT[id] };
    this.manualSetupState = "idle";
    this.onManualOpen(id);
    cur.logs.push(`[대기] 수동 개입: ${this.manualCurrent.title}`);
    void this.persist(true);
    if (id === "usb-debug") this.watchManual(cur, id, "USB 디버깅 연결 확인", () => this.usbDebugReady(), 2000);
    if (id === "mode-wait") this.watchManual(cur, id, "부트로더(fastboot) 모드 진입 확인", () => this.usbModeIs("fastboot"), 1500);
    if (id === "flash-mode") this.watchManual(cur, id, "플래시 모드 진입 확인", () => this.usbModeIs("flashmode"), 1500);
    if (id === "ims-check") this.watchManual(cur, id, "VoLTE(IMS) 등록 확인", () => this.imsReady(), 5000);
    if (id === "su-grant")
      this.watchManual(
        cur,
        id,
        "루트 권한(uid=0) 확인",
        async () => (await api.rootCheck(this.device?.serial)) === true,
        3000,
      );
    if (id === "smsie-export")
      this.watchManual(
        cur,
        id,
        "내보내기 파일 감지",
        async () => {
          const outcome = await api.smsieCollect(this.device?.serial, this.backupDir);
          if (outcome?.ready && outcome.summary) {
            this.backupSummary = outcome.summary;
            return true;
          }
          return false;
        },
        5000,
      );
  }

  /** 수동 개입이 열릴 때 자동 동작 — 언락: 발급 페이지 열기 + IMEI 읽기 / 펌웨어: 자동 다운로드 / smsie: 앱 준비 */
  private onManualOpen(id: ManualId) {
    if (id === "unlock-code") {
      void api.openExternal(LINKS.unlock);
      void this.loadImei();
    } else if (id === "ims-precheck") {
      void this.imsReady(); // 슬롯별 현재 상태 표시 (진행은 통화 확인 체크 후 [확인하고 진행])
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

  /** adb로 부트로더 재부팅(mode-wait 수동 개입에서 자동) — 시뮬레이션에서는 건너뛴다 */
  private async rebootToBootloader() {
    if (!REAL_STEPS.root && !REAL_STEPS.fastboot) return;
    const gen = this.runGen;
    const cur = this.runSteps[this.cursor];
    cur?.logs.push("[재부팅] 폰을 부트로더 모드로 재부팅합니다 — USB를 유지해 주세요");
    const r = await api.rootReboot(this.device?.serial, "bootloader");
    if (gen !== this.runGen) return;
    if (!r.ok) {
      cur?.logs.push(`[실패] 부트로더 재부팅: ${r.error} — 전원을 끈 뒤 볼륨 위를 누른 채 USB를 연결해 직접 진입해 주세요`);
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
      cur?.logs.push(`[준비] ${r.value.split("\n")[0]}`);
    } else {
      this.manualCheckError = r.error;
      cur?.logs.push(`[실패] 문자·통화 기록 복원 준비: ${r.error}`);
    }
    void this.persist(true);
  }

  /** SMS Import/Export 앱 준비 — 설치(필요 시 GitHub 다운로드)·권한·임시 폴더 */
  async smsiePrepare() {
    const gen = this.runGen;
    const cur = this.runSteps[this.cursor];
    if (!REAL_STEPS.backup) return;
    this.manualSetupState = "loading";
    this.manualCheckError = "";
    const r = await api.smsiePrepare(this.device?.serial, true);
    if (gen !== this.runGen) return;
    this.manualSetupState = r.ok ? "done" : "failed";
    if (r.ok) {
      cur?.logs.push(`[준비] SMS Import/Export — ${r.value.join(" · ")}`);
    } else {
      this.manualCheckError = r.error;
      cur?.logs.push(`[실패] SMS Import/Export 준비: ${r.error}`);
    }
    void this.persist(true);
  }

  async openPhoneSettings() {
    const screen = this.device?.prep.developerOptions === false ? "about" : "developer";
    await api.openSettingsScreen(this.device?.serial, screen);
  }

  async loadImei() {
    const gen = this.runGen;
    this.imeiState = "loading";
    const imei = await api.readImei1(this.device?.serial);
    if (gen !== this.runGen) return;
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
    if (gen !== this.runGen || request !== this.firmwareRequest || partition !== this.partition) return;
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
    await this.fetchFirmware();
    if (this.firmware) this.ackManual();
  }

  private logFirmware(cur: RunStep) {
    const fw = this.firmware;
    cur.logs.push(
      fw
        ? `[준비] 순정 펌웨어 ${fw.version} — ${fw.partition} 자동 다운로드(${(fw.downloadedBytes / 1024 ** 2).toFixed(1)} MB 받음), 기기 지문 일치`
        : this.firmwareDirInfo
          ? `[입력] 펌웨어 폴더: ${this.firmwareDir} (${this.firmwareDirInfo.file})`
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
  manualCheckError = $state("");
  manualSetupState = $state<LoadState>("idle");
  /** 리락 전 통신 확인 — 실제 발신·수신을 확인했다는 체크 */
  callAck = $state(false);
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
    return SIMULATED_RUN && !REAL_STEPS.fastboot && !REAL_STEPS.root && (this.manualVerifiable || this.manualCurrent?.id === "firmware-select");
  }

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
    if (gen !== this.runGen || request !== this.firmwareDirRequest || this.firmwareDir !== dir || partition !== this.partition) return;
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
        if (await this.imsReady()) return null;
        if (this.imsSims.some((s) => s.volte === "wifi")) return "Wi-Fi 통화로만 등록되어 있습니다 — 폰의 Wi-Fi를 끄고 셀룰러 VoLTE로 등록되는지 확인해 주세요";
        return (await this.imsReady())
          ? null
          : "VoLTE 등록이 확인되지 않았습니다 — 신호가 잡힐 때까지 기다리거나, 통화가 안 되면 [다시 패치]로 VoLTE 적용부터 다시 진행해 주세요";
      case "ims-check":
        if (await this.imsReady()) return null;
        if (this.imsSims.some((s) => s.volte === "wifi")) return "Wi-Fi 통화로만 등록되어 있습니다 — 폰의 Wi-Fi를 끄고 셀룰러 VoLTE로 등록되는지 확인해 주세요";
        return (await this.imsReady())
          ? null
          : "VoLTE 등록이 아직 확인되지 않았습니다 — 재부팅 후 통신사 신호가 잡힐 때까지 잠시 기다려 주세요";
      case "contacts-import": {
        const r = await api.contactsRestoreCheck(this.device?.serial, this.backupDir);
        if (!r) return "연락처 수를 확인하지 못했습니다 — 폰 연결을 확인해 주세요";
        if (r.onDevice >= r.backedUp) {
          this.runSteps[this.cursor]?.logs.push(`[확인] 연락처 ${r.onDevice}명 (백업 ${r.backedUp}명)`);
          return null;
        }
        return `폰의 연락처가 ${r.onDevice}명으로 백업(${r.backedUp}명)보다 적습니다 — 가져오기가 끝났는지 확인해 주세요`;
      }
      case "smsie-export": {
        // 산출물 수신 확인 — 수집이 합쳐지면 완결 여부도 갱신
        const outcome = await api.smsieCollect(this.device?.serial, this.backupDir);
        if (outcome?.ready && outcome.summary) {
          this.backupSummary = outcome.summary;
          return null;
        }
        if (outcome === null && !SIMULATED_RUN) {
          return "산출 파일 확인에 실패했습니다 — 폰 연결과 폴더 선택(xvolte-smsie)을 확인해 주세요";
        }
        return "아직 내보내기 파일이 감지되지 않았습니다 — 앱에서 내보내기를 마치고 저장 위치에 xvolte-smsie 폴더를 골랐는지 확인해 주세요";
      }
      default:
        return null;
    }
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
        this.runSteps[this.cursor]?.logs.push(`[확인 실패] ${err}`);
        void this.persist(true);
        return;
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
    this.runSteps[this.cursor]?.logs.push("[목업] 확인을 건너뛰었습니다 — 실제 실행에서는 확인될 때까지 진행하지 않습니다");
    this.stopWatch();
    this.ackManual(true);
  }

  /** 최종 VoLTE 확인 없이 마무리 — SIM 없이 미리 패치한 경우 등 (작업 자체는 모두 끝난 상태) */
  finishWithoutIms() {
    if (this.manualCurrent?.id !== "ims-check") return;
    this.imsUnverified = true;
    void this.persist(true);
    this.runSteps[this.cursor]?.logs.push("[확인 생략] VoLTE 등록을 확인하지 못한 채 마무리했습니다 — 통신 미검증. SIM을 넣으면 프로파일이 바뀌어 재패치가 필요할 수 있습니다");
    this.stopWatch();
    this.ackManual(true);
  }

  /** 입력형 수동 개입은 값이 채워져야 완료 가능 */
  get manualInputReady(): boolean {
    const m = this.manualCurrent;
    if ((m?.id === "smsie-export" && REAL_STEPS.backup) || (m?.id === "smsie-import" && REAL_STEPS.restore)) return this.manualSetupState === "done";
    if (m?.id === "backup-notice") return this.backupNoticeAck;
    if (m?.id === "ims-precheck") return this.callAck;
    if (m?.id === "oem-toggle") return this.prepMissing.length === 0 && (this.prepUnknown.length === 0 || this.oemUnknownAck) && !this.prepChecking;
    if (!m?.input) return true;
    if (m.input === "unlock-code") return this.unlockCodeValid;
    return this.firmware !== null || this.firmwareDirInfo !== null;
  }

  /** 수동 단계 완료 처리 — 확인을 거친 경로(confirmManual·자동 감지·목업 건너뛰기)에서만 호출 */
  ackManual(force = false) {
    if (!force && !this.manualInputReady) return;
    this.manualCheckError = "";
    const cur = this.runSteps[this.cursor];
    const m = this.manualCurrent;
    if (cur) {
      if (m?.input === "unlock-code") cur.logs.push(`[입력] 언락 코드: 0x${maskSecret(this.normalizedUnlockCode)}`);
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
    const items = this.checkedBackupItems();
    const dest = this.backupPath;
    if (!dest.trim()) {
      this.failStep("백업 저장 위치가 지정되지 않았습니다");
      return;
    }
    // 시작 시각 기준 폴더를 먼저 만들고 절대 경로를 진행 기록에 저장 — 끊겨도 같은 폴더로 이어서 받는다
    if (!this.backupDir) {
      const prep = await api.backupPrepare(this.device?.serial, dest);
      if (gen !== this.runGen) return;
      if (!prep.ok) {
        this.failStep(`백업 폴더 생성 실패: ${prep.error}`);
        return;
      }
      this.backupDir = prep.value;
      await this.persist(true);
      if (gen !== this.runGen) return;
    }
    cur.logs.push(`[실전] 백업 시작 — 항목 ${items.length}개 → ${this.backupDir}`);
    // 진행 이벤트 구독 → progress·sub 체크포인트 반영
    const itemOrder = items.slice();
    const completedItems = new Set<string>();
    const markItemDone = (itemId: string) => {
      if (!cur.sub) return;
      completedItems.add(itemId);
      while (cur.sub.done < itemOrder.length && completedItems.has(itemOrder[cur.sub.done])) {
        const id = itemOrder[cur.sub.done++];
        cur.logs.push(`[체크포인트] ${BACKUP_ITEM_LABEL[id] ?? id} 완료`);
      }
      void this.persist(true);
    };
    let logGate = 0;
    const un = await api.onBackupProgress((p) => {
      if (gen !== this.runGen) return;
      const ratio = p.bytesTotal > 0 ? p.bytesDone / p.bytesTotal : p.filesTotal > 0 ? p.filesDone / p.filesTotal : 0;
      const idx = Math.max(0, itemOrder.indexOf(p.itemId));
      cur.progress = Math.min(0.99, (idx + ratio) / Math.max(1, itemOrder.length));
      if (p.phase === "done") markItemDone(p.itemId);
      else if (p.file && logGate++ % 25 === 0) {
        cur.logs.push(`[백업] ${BACKUP_ITEM_LABEL[p.itemId] ?? p.itemId} — ${p.file}`);
      }
      void this.persist();
    });
    if (gen !== this.runGen) return un();
    const r = await api.backupRun(this.device?.serial, items, dest, this.backupDir || undefined).finally(un);
    if (gen !== this.runGen) return; // 중단·처음으로
    if (!r.ok) {
      this.failStep(`백업 실패: ${r.error}`);
      return;
    }
    this.backupDir = r.value.dir;
    this.backupSummary = r.value;
    for (const item of r.value.items) if (item.status === "done") markItemDone(item.id);
    cur.logs.push(`[백업] ${r.value.dir} — 파일 ${r.value.files.toLocaleString()}개, ${(r.value.bytes / 1024 ** 3).toFixed(2)} GiB`);
    if (!this.needSmsie(items)) return this.finishRealBackup(cur);
    // 문자·통화 기록(smsie) — 앱 설치·권한은 자동, 내보내기 2탭은 수동 개입
    const stepDef = this.steps.find((s) => s.id === "backup");
    if (stepDef && !stepDef.manual?.includes("smsie-export")) stepDef.manual = [...(stepDef.manual ?? []), "smsie-export"];
    cur.manualDone = 0;
    cur.status = "manual-wait";
    this.pause();
    void this.openManual(cur, "smsie-export");
  }

  private needSmsie(items: string[]): boolean {
    return items.includes("sms") || items.includes("calllog");
  }

  /** 완결 판정으로 백업 단계 마무리 — 파괴 단계(언락/리락) 게이트의 입력이 된다 */
  private finishRealBackup(cur: RunStep) {
    const s = this.backupSummary;
    if (!s) return this.failStep("백업 결과가 없습니다");
    if (s.complete) {
      cur.progress = 1;
      cur.logs.push("[완결] 전수 열거 완료 · 오류 0 — 파괴 단계 진행 가능");
      this.stepDone(cur);
    } else {
      const detail = s.errors.slice(0, 3).join(" / ");
      this.failStep(`백업 미완결(완결 게이트 실패)${detail ? ` — ${detail}` : ""} — 로그를 확인하고 [이 단계 다시 시도]로 재시도할 수 있습니다`);
    }
  }

  /** 단계 완료 공통 처리 — 시뮬레이션 tick의 완료 블록과 같은 규칙 */
  private stepDone(cur: RunStep) {
    cur.status = "done";
    cur.logs.push("[완료]");
    this.cursor++;
    void this.persist(true);
    if (this.cursor >= this.runSteps.length) this.complete();
    else this.begin();
  }

  /** 실전 복구 러너 — APK 재설치 → tar 스트리밍 → 설정 → 연락처 전송 후, 문자·통화(smsie)는 수동 개입 */
  private async runRealRestore(cur: RunStep) {
    const gen = this.runGen;
    if (!this.backupDir) {
      this.failStep("복구할 백업 폴더가 없습니다");
      return;
    }
    const items = this.checkedBackupItems();
    cur.logs.push(`[실전] 복구 시작 — ${this.backupDir}`);
    const itemOrder = items.slice();
    const un = await api.onRestoreProgress((p) => {
      if (gen !== this.runGen) return;
      const ratio = p.bytesTotal > 0 ? p.bytesDone / p.bytesTotal : p.filesTotal > 0 ? p.filesDone / p.filesTotal : 0;
      const idx = Math.max(0, itemOrder.indexOf(p.itemId));
      cur.progress = Math.min(0.99, (idx + ratio) / Math.max(1, itemOrder.length));
      void this.persist();
    });
    if (gen !== this.runGen) return un();
    const r = await api.restoreRun(this.device?.serial, this.backupDir, items).finally(un);
    if (gen !== this.runGen) return; // 중단·처음으로
    if (!r.ok) {
      this.failStep(`복구 실패: ${r.error}`);
      return;
    }
    for (const line of r.value.logs) cur.logs.push(`[복구] ${line}`);
    for (const fail of r.value.failures) cur.logs.push(`[실패] ${fail}`);
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
      cur.manualDone = 0;
      cur.status = "manual-wait";
      this.pause();
      void this.openManual(cur, manuals[0]);
      return;
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
      cur.logs.push("[게이트] 백업 선택 없음 — 실행 전 이중 확인으로 진행");
      return resume();
    }
    if (this.backupSummary?.complete) {
      cur.logs.push("[게이트] 이번 실행의 백업 완결 확인 — 진행");
      return resume();
    }
    if (this.backupDir) {
      // 재개·이어받기 등: 기존 폴더를 파일 존재·크기·해시 대조로 재검사
      const s = await api.backupManifestCheck(this.backupDir);
      if (gen !== this.runGen) return; // 그사이 중단됨
      if (s?.complete) {
        this.backupSummary = s;
        cur.logs.push("[게이트] 기존 백업 완결 재검사 통과(파일·해시 대조)");
        return resume();
      }
      const errs = s?.errors.length ? s.errors.join(" / ") : "완결 아님";
      return this.failStep(`백업 완결 게이트 실패 — ${errs}. 백업 단계를 다시 진행해 주세요`);
    }
    this.failStep("백업이 완결되지 않아 파괴 단계를 진행할 수 없습니다 — 백업 단계를 먼저 끝내주세요");
  }

  /** 실전 언락 — 사전 프로브(getvar) → oem unlock 0x{code} → getvar 이중 확인 (설계 .plans/04-engine/fastboot.md) */
  private async runRealUnlock(cur: RunStep) {
    const gen = this.runGen;
    if (!this.unlockCodeValid) {
      this.failStep("언락 코드가 입력되지 않았거나 형식(16자리 16진수)이 맞지 않습니다");
      return;
    }
    const un = await api.onFastbootLog((line) => {
      if (gen !== this.runGen) return;
      cur.logs.push(`[fastboot] ${line}`);
      void this.persist();
    });
    if (gen !== this.runGen) return un();
    // 사전 프로브 — 모드·슬롯·언락 상태 확인(§10-2)
    cur.progress = 0.2;
    cur.logs.push("[실전] fastboot 연결 확인(getvar)");
    const vars = await api.fastbootGetvar();
    if (gen !== this.runGen) return un();
    if (!vars) {
      un();
      return this.failStep("fastboot 프로브 실패 — 폰이 부트로더 모드(파란 LED)인지 확인해 주세요");
    }
    cur.logs.push(`[fastboot] unlocked=${vars.unlocked ?? "?"} · slot=${vars["current-slot"] ?? "?"}`);
    const state = (vars.unlocked ?? "").trim().toLowerCase();
    if (state !== "yes" && state !== "no") {
      un();
      return this.failStep("부트로더 잠금 상태를 확인할 수 없습니다 — 언락을 실행하지 않습니다");
    }
    if ((vars["is-userspace"] ?? "").trim().toLowerCase() !== "no") {
      un();
      return this.failStep("부트로더 모드를 확인할 수 없습니다 — fastbootd에서는 언락할 수 없습니다");
    }
    if (state === "yes") {
      cur.logs.push("[확인] 이미 언락되어 있습니다 — OS 재부팅을 확인합니다");
      const rebooted = await api.fastbootReboot("os", this.device?.serial ?? "");
      un();
      if (gen !== this.runGen) return;
      if (!rebooted) return this.failStep("언락 상태지만 OS 재부팅을 확인하지 못했습니다 — 기기 상태를 확인해 주세요");
      cur.progress = 1;
      return this.stepDone(cur);
    }
    cur.progress = 0.5;
    const r = await api.fastbootUnlock(this.normalizedUnlockCode, true, this.device?.serial ?? "");
    if (gen !== this.runGen) return un();
    if (!r.ok) {
      un();
      return this.failStep(`언락 실패: ${r.error}`);
    }
    if (!r.value.unlocked) {
      un();
      return this.failStep("언락 명령 후에도 unlocked=yes가 확인되지 않습니다 — 기기 상태를 확인해 주세요");
    }
    const rebooted = await api.fastbootReboot("os", this.device?.serial ?? "");
    un();
    if (gen !== this.runGen) return;
    if (!rebooted) return this.failStep("언락은 확인됐지만 OS 재부팅을 확인하지 못했습니다 — 기기 상태를 확인해 주세요");
    cur.progress = 1;
    cur.logs.push("[완료] 부트로더 언락 확인(unlocked=yes) — 기기가 초기화된 뒤 재부팅됩니다");
    this.stepDone(cur);
  }

  /** 순정 부트 이미지 경로 — 사전 준비(firmware_fetch) 결과 또는 직접 지정 폴더 (root·unroot·relock 공용) */
  private stockImagePath(): string | null {
    return this.firmwareDir ? this.firmwareDirInfo?.path ?? null : this.firmware?.path ?? null;
  }

  private stockFingerprint(): string {
    return this.firmwareDir ? this.firmwareDirInfo?.fingerprint ?? "" : this.firmware?.fingerprint ?? "";
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
    if (!serial) return null;
    const buf = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(serial));
    return [...new Uint8Array(buf)].map((b) => b.toString(16).padStart(2, "0")).join("");
  }

  /** 실전 언루팅 — 원본 unRoot 계승: 순정 이미지 양 슬롯 기록 후 안내(앱 삭제는 수동) */
  private async runRealUnroot(cur: RunStep) {
    const gen = this.runGen;
    if (!this.realBootFlowReady()) return;
    const partition = this.partition;
    const stockPath = this.stockImagePath();
    if (!partition || !stockPath) {
      this.failStep("순정 부트 이미지가 준비되지 않았습니다 — 사전 준비에서 펌웨어를 먼저 받아 주세요");
      return;
    }
    const source = await api.bootImageCheck(this.device?.serial ?? "", stockPath, this.stockFingerprint());
    if (gen !== this.runGen) return;
    if (!source.ok) return this.failStep(`부트 이미지 확인 실패: ${source.error}`);
    cur.logs.push(`[언루팅] 순정 ${partition} 이미지로 복원합니다`);
    // 부트로더 진입 → 감지 대기
    cur.progress = 0.2;
    const rb = await api.rootReboot(this.device?.serial, "bootloader");
    if (gen !== this.runGen) return;
    if (!rb.ok) return this.failStep(`부트로더 재부팅 실패: ${rb.error}`);
    const inFb = await this.waitFor(gen, () => this.usbModeIs("fastboot"), 90_000);
    if (gen !== this.runGen) return;
    if (!inFb) return this.failStep("부트로더 모드 진입이 감지되지 않습니다 — USB 연결을 확인해 주세요");
    // 순정 기록(양 슬롯) — 이 기록이 리락 게이트(§3-3)의 순정 증거가 된다
    cur.progress = 0.4;
    const un = await api.onFastbootLog((line) => {
      if (gen !== this.runGen) return;
      cur.logs.push(`[fastboot] ${line}`);
      void this.persist();
    });
    if (gen !== this.runGen) return un();
    const flash = await api.fastbootFlash(partition, stockPath, true, this.device?.serial ?? "", source.value).finally(un);
    if (gen !== this.runGen) return;
    if (!flash.ok) return this.failStep(`순정 이미지 기록 실패: ${flash.error}`);
    cur.logs.push(`[언루팅] ${partition}_a/_b 순정 기록 완료`);
    void this.persist(true);
    // 재부팅 → 복귀 대기
    const rebooted = await api.fastbootReboot("os", this.device?.serial ?? "");
    if (gen !== this.runGen) return;
    if (!rebooted) return this.failStep("순정 기록 후 OS 재부팅을 확인하지 못했습니다 — 기기 상태를 확인해 주세요");
    const back = await this.waitFor(gen, () => this.usbDebugReady(), 240_000);
    if (gen !== this.runGen) return;
    if (!back) {
      return this.failStep("재부팅 후 기기 연결이 확인되지 않습니다 — 기록은 완료됐으므로 폰을 확인한 뒤 이 단계를 다시 시도해 주세요");
    }
    const stillRooted = await api.rootCheck(this.device?.serial);
    if (gen !== this.runGen) return;
    if (stillRooted === true) return this.failStep("순정 기록 후에도 루트 권한이 남아 있습니다 — 이미지와 기기 상태를 확인해 주세요");
    if (stillRooted === null) cur.logs.push("[미확인] 루트 해제 상태를 자동으로 확인하지 못했습니다 — 폰의 Magisk에서 직접 확인해 주세요");
    cur.progress = 1;
    cur.logs.push("[완료] 순정 이미지 기록·재연결 — Magisk 앱을 열어 루트가 해제됐는지 확인한 뒤 앱을 직접 삭제해 주세요");
    cur.logs.push("[안내] Play 프로텍트 인증이 안 되면 Play 스토어 > 앱 정보 > 저장공간 > 데이터 삭제를 해주세요");
    this.stepDone(cur);
  }

  /** 실전 리락 — 게이트(§3-3) 사전 점검 → oem lock → 확인 (최종 판정은 백엔드가 기기 fastboot serial 기준으로 재수행) */
  private async runRealRelock(cur: RunStep) {
    const gen = this.runGen;
    const partition = this.partition;
    const stockPath = this.stockImagePath();
    if (!partition || !stockPath) {
      this.failStep("리락 게이트에 필요한 부트 파티션·순정 이미지가 준비되지 않았습니다 — 사전 준비에서 펌웨어를 먼저 받아 주세요");
      return;
    }
    const un = await api.onFastbootLog((line) => {
      if (gen !== this.runGen) return;
      cur.logs.push(`[fastboot] ${line}`);
      void this.persist();
    });
    if (gen !== this.runGen) return un();
    // 사전 게이트 — 미춴족 사유를 로그로 보여준다
    cur.progress = 0.3;
    const key = await this.deviceKeyHex();
    if (gen !== this.runGen) return un();
    if (!key) { un(); return this.failStep("리락 게이트의 기기 식별값이 없습니다"); }
    const gateRes = await api.relockGateCheck(partition, stockPath, key ?? undefined);
    if (gen !== this.runGen) return un();
    if (!gateRes.ok) {
      un();
      return this.failStep(`리락 게이트 점검 실패: ${gateRes.error}`);
    }
    const gate = gateRes.value;
    for (const c of gate.checked) cur.logs.push(`[게이트] ${c.partition}${c.slot} — ${c.detail}`);
    if (!gate.ok) {
      un();
      return this.failStep(`리락 게이트 실패 — ${gate.reasons.join(" / ")}`);
    }
    cur.progress = 0.6;
    const r = await api.fastbootLock(true, partition, stockPath, this.device?.serial ?? "");
    if (gen !== this.runGen) return un();
    if (!r.ok) {
      un();
      return this.failStep(`리락 실패: ${r.error}`);
    }
    if (r.value.unlocked) {
      un();
      return this.failStep("리락 후에도 unlocked=no가 확인되지 않습니다 — 기기 상태를 확인해 주세요");
    }
    const rebooted = await api.fastbootReboot("os", this.device?.serial ?? "");
    un();
    if (gen !== this.runGen) return;
    if (!rebooted) return this.failStep("리락 후 OS 재부팅을 확인하지 못했습니다 — 기기 상태를 확인해 주세요");
    cur.progress = 1;
    cur.logs.push("[완료] 부트로더 리락 확인(unlocked=no) — 기기가 초기화된 뒤 재부팅됩니다");
    this.stepDone(cur);
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
    const imagePath = this.stockImagePath();
    if (!imagePath) {
      this.failStep("순정 부트 이미지가 준비되지 않았습니다 — 사전 준비 단계에서 펌웨어를 먼저 받아 주세요");
      return;
    }
    const fingerprint = this.stockFingerprint();
    const source = await api.bootImageCheck(this.device?.serial ?? "", imagePath, fingerprint);
    if (gen !== this.runGen) return;
    if (!source.ok) return this.failStep(`부트 이미지 확인 실패: ${source.error}`);
    // 1) Magisk APK 확보(캐시 재사용)
    cur.progress = 0.05;
    const prep = await api.magiskPrepare();
    if (gen !== this.runGen) return;
    if (!prep.ok) return this.failStep(`Magisk 다운로드 실패: ${prep.error}`);
    cur.logs.push(`[루팅] Magisk ${prep.value.version} 준비 (sha256 ${prep.value.sha256.slice(0, 12)}…)`);
    this.markRootSub(cur, 1); // 받기
    void this.persist(true);
    // 2) 부트 패치(스테이징·스크립트·검증·수신)
    cur.progress = 0.15;
    const un = await api.onMagiskLog((line) => {
      if (gen !== this.runGen) return;
      cur.logs.push(`[magisk] ${line}`);
      void this.persist();
    });
    if (gen !== this.runGen) return un();
    const patch = await api.magiskPatch({
      serial: this.device?.serial ?? "", apkPath: prep.value.apkPath, imagePath, partition,
      imageSha256: source.value, fingerprint, apkSha256: prep.value.sha256,
    }).finally(un);
    if (gen !== this.runGen) return;
    if (!patch.ok) return this.failStep(`부트 패치 실패: ${patch.error}`);
    this.patchedImage = patch.value.path;
    cur.logs.push(`[루팅] 패치 완료 — ${(patch.value.bytes / 1024 ** 2).toFixed(1)} MiB · 원본과 해시 상이 확인`);
    this.markRootSub(cur, 4); // 받기·전송·패치·결과 확인
    void this.persist(true);
    // 3) 부트로더 진입 → fastboot 감지 대기
    cur.progress = 0.5;
    cur.logs.push("[루팅] 부트로더 모드로 재부팅합니다");
    const rb = await api.rootReboot(this.device?.serial, "bootloader");
    if (gen !== this.runGen) return;
    if (!rb.ok) return this.failStep(`부트로더 재부팅 실패: ${rb.error}`);
    const inFastboot = await this.waitFor(gen, () => this.usbModeIs("fastboot"), 90_000);
    if (gen !== this.runGen) return;
    if (!inFastboot) return this.failStep("부트로더 모드 진입이 감지되지 않습니다 — USB 연결을 확인해 주세요");
    // 4) 패치 이미지 기록(양 슬롯) — fastboot 엔진 재사용
    cur.progress = 0.7;
    const unFlash = await api.onFastbootLog(line => {
      if (gen === this.runGen) cur.logs.push(`[fastboot] ${line}`);
    });
    if (gen !== this.runGen) return unFlash();
    const flash = await api.fastbootFlash(partition, patch.value.path, true, this.device?.serial ?? "", patch.value.patchedSha256).finally(unFlash);
    if (gen !== this.runGen) return;
    if (!flash.ok) return this.failStep(`부트 이미지 기록 실패: ${flash.error}`);
    cur.logs.push(`[루팅] ${partition}_a/_b 기록 완료`);
    this.markRootSub(cur, 5); // 기록
    void this.persist(true);
    // 5) 재부팅 → adb 복귀 대기
    const rebooted = await api.fastbootReboot("os", this.device?.serial ?? "");
    if (gen !== this.runGen) return;
    if (!rebooted) return this.failStep("패치 기록 후 OS 재부팅을 확인하지 못했습니다 — 기기 상태를 확인해 주세요");
    const back = await this.waitFor(gen, () => this.usbDebugReady(), 240_000);
    if (gen !== this.runGen) return;
    if (!back) return this.failStep("기기가 다시 연결되지 않습니다 — 재부팅 후 USB 디버깅 승인을 확인해 주세요");
    // 6) Magisk 앱 설치
    cur.progress = 0.9;
    const inst = await api.magiskInstall(this.device?.serial, prep.value.apkPath, prep.value.sha256);
    if (gen !== this.runGen) return;
    if (!inst.ok) return this.failStep(`Magisk 앱 설치 실패: ${inst.error}`);
    this.markRootSub(cur, 6); // 앱 설치
    void this.persist(true);
    // 7) su 승인(수동 개입) — 확인 버튼·자동 감지가 root_check로 검증
    const stepDef = this.steps.find((s) => s.id === "root");
    if (stepDef && !stepDef.manual?.includes("su-grant")) stepDef.manual = [...(stepDef.manual ?? []), "su-grant"];
    cur.manualDone = 0;
    cur.status = "manual-wait";
    this.pause();
    void this.openManual(cur, "su-grant");
  }

  /** su 승인 후 최종 확인 — root_check로 uid=0 검증 */
  private async finishRealRoot(cur: RunStep) {
    const gen = this.runGen;
    const ok = await api.rootCheck(this.device?.serial);
    if (gen !== this.runGen) return;
    if (ok) {
      cur.logs.push("[완료] 루트 권한 확인(su -c id = uid=0)");
      cur.progress = 1;
      this.stepDone(cur);
    } else {
      this.failStep("루트 권한이 확인되지 않습니다 — 폰의 Magisk 권한 요청을 '허용'한 뒤 다시 확인해 주세요");
    }
  }

  /** 루팅 세부 작업 체크포인트 갱신 — subtasksFor("root") 6종과 대응(시뮬레이션과 같은 단위) */
  private markRootSub(cur: RunStep, done: number) {
    if (!cur.sub) return;
    const reached = Math.min(cur.sub.list.length, done);
    if (reached > cur.sub.done) {
      for (let k = cur.sub.done; k < reached; k++) cur.logs.push(`[체크포인트] ${cur.sub.list[k]} 완료`);
      cur.sub.done = reached;
    }
  }

  // ── 실전 EFS (REAL_STEPS.efs 전환 시) — .plans/04-engine/efstools-wrapper.md ──

  /** 로그 구독 공용 — EfsTools 라인을 단계 로그로 흘린다 */
  private async subscribeEfsLog(gen: number, cur: RunStep) {
    return api.onEfsLog((ev) => {
      if (gen !== this.runGen) return;
      cur.logs.push(`[EFS/${ev.cmd}] ${ev.line}`);
      void this.persist();
    });
  }

  /** EFS 사전 점검 — DIAG 전환 → targetInfo + efsInfo */
  private async runRealEfsPreflight(cur: RunStep) {
    const gen = this.runGen;
    // DIAG 전환 (원본 efsPortOpen 계승)
    cur.progress = 0.2;
    cur.logs.push("[EFS] DIAG 포트 전환 — 폰의 루트 권한 요청을 허용해 주세요");
    const diag = await api.efsDiagOpen(this.device?.serial);
    if (gen !== this.runGen) return;
    if (!diag.ok) return this.failStep(`DIAG 포트 전환 실패: ${diag.error}`);
    cur.logs.push("[EFS] DIAG 전환 완료 — EfsTools 사전 점검 실행");
    // 점검
    cur.progress = 0.5;
    const un = await this.subscribeEfsLog(gen, cur);
    const r = await api.efsPreflight();
    un();
    if (gen !== this.runGen) return;
    if (!r.ok) return this.failStep(`EFS 사전 점검 실패: ${r.error}`);
    for (const line of r.value.log) cur.logs.push(`[EFS] ${line}`);
    if (r.value.errors.length > 0) {
      for (const e of r.value.errors) cur.logs.push(`[경고] ${e}`);
      cur.logs.push("[주의] 점검에서 경고가 발견되었습니다 — 연결 상태를 확인한 뒤 진행합니다");
    } else {
      cur.logs.push("[EFS] 사전 점검 통과");
    }
    cur.progress = 1;
    this.stepDone(cur);
  }

  /** EFS 업로드 — 슬롯별 2회(원본 계승). 성공 판정은 하지 않는다(verify 단계가 담당) */
  private async runRealEfsUpload(cur: RunStep) {
    const gen = this.runGen;
    // 프리셋 경로 결정 — 각 슬롯의 통신사에 대응
    const targets = this.volteConfig.sims.filter((s) => s.carrier !== null);
    if (targets.length === 0) return this.failStep("패치 대상 SIM이 없습니다");
    const un = await this.subscribeEfsLog(gen, cur);
    let uploadIdx = 0;
    const totalUploads = targets.length * 2; // 슬롯별 2회
    for (const t of targets) {
      const preset = efsPreset(t.carrier!, t.slot);
      if (!preset) {
        un();
        return this.failStep(`SIM${t.slot} ${t.carrier} — 번들 프리셋을 찾을 수 없습니다`);
      }
      // 프리셋 폴더 경로 — efsPresets.ts의 folder 필드 (프로젝트 루트 상대)
      const presetDir = preset.folder;
      for (let round = 1; round <= 2; round++) {
        cur.progress = Math.min(0.95, (uploadIdx + 0.5) / totalUploads);
        cur.logs.push(`[EFS] SIM${t.slot} ${t.carrier} — ${round}차 업로드 (${uploadIdx + 1}/${totalUploads})`);
        void this.persist(true);
        const r = await api.efsUpload(presetDir);
        if (gen !== this.runGen) return un();
        if (!r.ok) {
          un();
          return this.failStep(`SIM${t.slot} ${round}차 업로드 실패: ${r.error}`);
        }
        if (r.value.errors.length > 0) {
          cur.logs.push(`[경고] 업로드 중 오류 라인 ${r.value.errors.length}개 — 확인 단계에서 최종 판정합니다`);
        }
        uploadIdx++;
        if (cur.sub) {
          const subIdx = cur.sub.list.findIndex((s) => s.includes(`SIM${t.slot}`) && s.includes(`${round}차`));
          if (subIdx >= 0 && cur.sub.done < subIdx + 1) {
            cur.sub.done = subIdx + 1;
            void this.persist(true);
          }
        }
      }
    }
    un();
    cur.progress = 1;
    cur.logs.push(`[EFS] 업로드 완료 — ${totalUploads}회 — 다음 단계(적용 확인)에서 전수 리드백 검증합니다`);
    this.stepDone(cur);
  }

  /** EFS 전수 리드백 검증 — 업로드 성공의 유일한 최종 근거 */
  private async runRealEfsVerify(cur: RunStep) {
    const gen = this.runGen;
    const targets = this.volteConfig.sims.filter((s) => s.carrier !== null);
    if (targets.length === 0) return this.failStep("검증할 패치 대상 SIM이 없습니다");
    const un = await this.subscribeEfsLog(gen, cur);
    let allOk = true;
    let totalMatched = 0;
    let totalFiles = 0;
    let slotIdx = 0;
    for (const t of targets) {
      const preset = efsPreset(t.carrier!, t.slot);
      if (!preset) {
        un();
        return this.failStep(`SIM${t.slot} ${t.carrier} — 번들 프리셋을 찾을 수 없습니다`);
      }
      cur.progress = Math.min(0.95, (slotIdx + 0.5) / targets.length);
      cur.logs.push(`[검증] SIM${t.slot} ${t.carrier} — 전수 리드백·해시 비교`);
      void this.persist(true);
      const r = await api.efsVerify(preset.folder);
      if (gen !== this.runGen) return un();
      if (!r.ok) {
        un();
        return this.failStep(`SIM${t.slot} 검증 실패: ${r.error}`);
      }
      const report = r.value;
      totalMatched += report.matched;
      totalFiles += report.files;
      if (report.ok) {
        cur.logs.push(`[검증] SIM${t.slot} 통과 — ${report.matched}/${report.files} 파일 해시 일치`);
      } else {
        allOk = false;
        for (const m of report.mismatches.slice(0, 5)) cur.logs.push(`[불일치] ${m}`);
        if (report.mismatches.length > 5) cur.logs.push(`… 외 ${report.mismatches.length - 5}개 불일치`);
        for (const m of report.missing.slice(0, 5)) cur.logs.push(`[누락] ${m}`);
        if (report.missing.length > 5) cur.logs.push(`… 외 ${report.missing.length - 5}개 누락`);
      }
      if (cur.sub) {
        const subIdx = cur.sub.list.findIndex((s) => s.includes(`SIM${t.slot}`));
        if (subIdx >= 0 && cur.sub.done < subIdx + 1) {
          cur.sub.done = subIdx + 1;
          void this.persist(true);
        }
      }
      slotIdx++;
    }
    un();
    if (!allOk) {
      return this.failStep(
        `전수 리드백 검증 실패 — 매칭 ${totalMatched}/${totalFiles}. [이 단계 다시 시도]로 재검증하거나 [중단]할 수 있습니다`,
      );
    }
    cur.progress = 1;
    cur.logs.push(`[완결] 전수 리드백 통과 — ${totalMatched}/${totalFiles} 파일 해시 일치`);
    this.stepDone(cur);
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
        for (const line of r.value) cur.logs.push(`[마무리] ${line}`);
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
  simulateEfsFail = $state(false);
  private efsFailedOnce = false;

  /** 실제 백엔드도 이 경로로 실패를 알린다 — 실패 파일·슬롯·시도 횟수·종료 코드·출력 요약을 reason과 로그에 남긴다 */
  failStep(reason: string) {
    const cur = this.runSteps[this.cursor];
    if (!cur) return;
    cur.status = "failed";
    cur.logs.push(`[실패] ${reason}`);
    this.stepError = reason;
    this.pause();
    this.stopWatch();
    this.markStop(reason);
  }

  /** 실패한 단계를 처음부터 다시 */
  retryStep() {
    const cur = this.runSteps[this.cursor];
    if (!cur || cur.status !== "failed") return;
    cur.status = "pending";
    cur.progress = 0;
    cur.manualDone = 0;
    cur.sub = undefined;
    this.clearRuntimeManuals(cur.id);
    cur.logs.push("[재시도] 이 단계를 처음부터 다시 진행합니다");
    this.stepError = "";
    this.runGen++;
    this.begin();
  }

  /** 통신 확인 실패 시 VoLTE 적용부터 다시 (리락 전이라 루트가 남아 있음) */
  repatch() {
    const idx = this.runSteps.findIndex((s) => s.id === "efs-preflight" || s.id === "efs");
    if (idx < 0 || idx > this.cursor) return;
    for (let k = idx; k <= this.cursor; k++) {
      const st = this.runSteps[k];
      st.status = "pending";
      st.progress = 0;
      st.manualDone = 0;
      st.sub = undefined;
      this.clearRuntimeManuals(st.id);
    }
    this.runSteps[idx].logs.push("[재패치] 통신이 확인되지 않아 VoLTE 적용부터 다시 진행합니다");
    this.stopWatch();
    this.manualCurrent = null;
    this.manualCheckError = "";
    this.cursor = idx;
    this.runGen++;
    void this.persist(true);
    this.begin();
  }

  dismissUsbError() {
    this.usbError = false;
    const cur = this.runSteps[this.cursor];
    if (cur) { cur.status = "running"; cur.logs.push("[재개] 재연결 확인 — 이어서 진행합니다"); }
    this.begin();
  }

  /** 실행을 멈춘 뒤 돌아오더라도 running/manual-wait가 남아 단계를 건너뛰지 않게 한다. */
  private stopRun(resetFailed: boolean) {
    this.runGen++;
    this.pause();
    this.stopWatch();
    // 진행 중인 실전 백업이 있으면 백엔드에도 취소 전달
    void api.backupCancel();
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
    this.runGen++;
    this.pause();
    this.stopWatch();
    this.view = "device";
    this.device = null;
    this.env = [];
    this.volteConfig = defaultVolteConfig();
    this.omdAck = false;
    this.riskAck = false;
    this.groups = [];
    this.opts = { unroot: false, relock: false, restore: true };
    this.backupPath = "";
    this.sizes = null;
    this.sizesState = "idle";
    this.appClasses = null;
    this.appClassesState = "idle";
    this.settingsInfo = null;
    this.settingsInfoState = "idle";
    this.backupNoticeAck = false;
    this.optionsFor = null;
    this.fwVersions = null;
    this.fwVersionsState = "idle";
    this.fwVersionsFor = null;
    this.steps = [];
    this.runSteps = [];
    this.finished = false;
    this.manualCurrent = null;
    this.unlockCode = "";
    this.firmwareDir = "";
    this.imei1 = null;
    this.imeiState = "idle";
    this.firmware = null;
    this.firmwareState = "idle";
    this.firmwareError = "";
    this.firmwareFail = null;
    this.firmwareDest = "";
    this.usbError = false;
    this.usbErrorCount = 0;
    this.simulateUsbError = false;
    this.erroredOnce = false;
    this.cursor = 0;
    this.setGuard(false);
    this.manualChecking = false;
    this.manualCheckError = "";
    this.oemUnknownAck = false;
    this.firmwareDirInfo = null;
    this.firmwareDirState = "idle";
    this.firmwareDirError = "";
    this.imsUnverified = false;
    this.callAck = false;
    this.stepError = "";
    this.simulateEfsFail = false;
    this.efsFailedOnce = false;
    this.backupSummary = null;
    this.backupDir = "";
    this.patchedImage = "";
    this.backupRanGen = -1;
    this.restoreRanGen = -1;
    this.unlockRanGen = -1;
    this.relockRanGen = -1;
    this.rootRanGen = -1;
    this.unrootRanGen = -1;
    this.efsPreflightRanGen = -1;
    this.efsRanGen = -1;
    this.efsVerifyRanGen = -1;
    this.journalSims = undefined;
    this.sessionFor = null;
    this.pendingJournal = null;
    this.journalKey = null;
    this.journalStarted = "";
    this.stopInfo = null;
  }

  private complete() {
    this.pause();
    this.finished = true;
    this.setGuard(false);
    void this.persist(true);
    const key = this.journalKey;
    if (key) void this.journalWrites.push(() => api.journalArchive(key, "done"));
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
