// 위자드 상태 머신 + 실행 시뮬레이션 러너 (mock)
import type { AppItem, FirmwareResult, FirmwareVersions, SettingsOverview, SimInfo, BackupGroup, DeviceStatus, EnvCheckItem, ManualId, ManualPrompt, PlanStep, RunJournal, RunStep, VolteConfig } from "$lib/types";
import { api } from "$lib/api";
import { LINKS, maskSecret } from "$lib/data/links";
import { bootPartition } from "$lib/data/devices";
import { mockBackupGroups } from "$lib/mock/apps";
import { bootloaderOnly, buildPlan, updateTarget, type PlanOptions } from "$lib/mock/plan";
import { SIMULATED_RUN, REAL_STEPS } from "$lib/data/runMode";
import { EFS_PRESET_MODE, EFS_PRESET_VERSION, efsPreset } from "$lib/data/efsPresets";
import type { BackupSummary } from "$lib/types";

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
const BACKUP_ITEM_LABEL: Record<string, string> = {
  "settings-all": "전체 설정 백업",
  apk: "APK 파일",
  "app-data": "앱 데이터",
  dcim: "사진·영상 (DCIM)",
  download: "다운로드",
  pictures: "Pictures",
  movies: "Movies",
  music: "Music",
  documents: "Documents",
  recordings: "Recordings",
  "fs-rest": "그 외 전체 파일 시스템",
  calllog: "통화 기록",
  sms: "문자",
  contacts: "연락처",
};

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
    if (!d?.serial) return (this.journalKey = null);
    const buf = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(`${d.model}|${d.serial}`));
    this.journalKey = [...new Uint8Array(buf)].slice(0, 16).map((b) => b.toString(16).padStart(2, "0")).join("");
    return this.journalKey;
  }

  /** 경고 페이지 [다음] — 같은 폰의 끝나지 않은 작업이 있으면 pendingJournal에 두고 true */
  async checkJournal(): Promise<boolean> {
    const key = await this.journalKeyReady();
    if (!key) return false;
    const raw = await api.journalLoad(key);
    if (!raw) return false;
    try {
      const j = JSON.parse(raw) as RunJournal;
      if (j.version !== 1 || !j.runSteps?.length) return false;
      this.pendingJournal = j;
      return true;
    } catch {
      return false;
    }
  }

  /** [새로 시작] — 이전 기록은 discarded로 보관하고 1단계부터 */
  discardJournal() {
    if (this.journalKey) void api.journalArchive(this.journalKey, "discarded");
    this.pendingJournal = null;
    this.view = "step1";
  }

  /** [이어서 진행] — 선택했던 옵션·진행 상황을 되살리고 실행 화면으로 (자동 시작하지 않음) */
  resumeJournal() {
    const j = this.pendingJournal;
    if (!j) return;
    this.volteConfig = { ...defaultVolteConfig(), ...j.config };
    this.ensureOptions();
    for (const g of this.groups) for (const i of g.items) i.checked = j.backupItems.includes(i.id);
    this.opts = { ...j.opts };
    this.backupPath = j.backupPath;
    this.backupDir = j.backupDir ?? "";
    this.backupSummary = null; // 이어서 진행 시 백업 결과는 단계 재검증으로 다시 채운다
    this.firmware = j.firmware;
    this.firmwareState = j.firmware ? "done" : "idle";
    this.firmwareDir = j.firmwareDir;
    this.imsUnverified = j.imsUnverified ?? false;
    // 직접 지정한 폴더는 그사이 바뀌었을 수 있으므로 다시 검사
    if (!j.firmware && j.firmwareDir) void this.setFirmwareDir(j.firmwareDir);
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
    this.cursor = first >= 0 ? first : runSteps.length;
    this.journalStarted = j.startedAt;
    this.stopInfo = null;
    this.finished = false;
    this.usbError = false;
    this.erroredOnce = false;
    this.runGen++;
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
    return api.journalSave(key, JSON.stringify(j));
  }

  // ── 작업 중 PC 보호 (절전·Windows 종료 방지) — 실행 중·폰 확인 대기 중에는 켜고, 끝나거나 멈추면 끈다
  private guardOn = false;
  private setGuard(on: boolean) {
    if (this.guardOn === on) return;
    this.guardOn = on;
    void api.runGuard(on, "Xperia VoLTE 작업 진행 중 — 끝날 때까지 PC를 끄지 마세요").then((ok) => {
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
    const cur = this.runSteps[this.cursor];
    const wasWaiting = cur?.status === "manual-wait";
    this.pause();
    this.stopWatch();
    if (!this.stopInfo && cur) {
      this.stopInfo = {
        stepId: cur.id,
        stepTitle: cur.title,
        reason: wasWaiting ? "폰 확인을 기다리는 중에 프로그램을 종료했습니다" : "작업 중에 프로그램을 종료했습니다",
        at: new Date().toISOString(),
      };
      cur.logs.push(`[종료] ${this.stopInfo.reason}`);
    }
    const saved = await this.persist(true);
    this.setGuard(false);
    return saved;
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

  private tick() {
    while (this.runSteps[this.cursor] && ["done", "skipped"].includes(this.runSteps[this.cursor].status)) this.cursor++;
    const cur = this.runSteps[this.cursor];
    if (!cur) return this.complete();
    if (cur.status === "pending") {
      cur.status = "running";
      cur.logs.push(`[시작] ${cur.title}`);
      const list = this.subtasksFor(cur.id);
      if (list.length > 0 && !cur.sub) cur.sub = { list, done: 0 };
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
          void this.runRealBackup(cur);
        } else {
          // 문자·통화 기록(smsie) 수동 완료 후 재진입 — 완결 판정으로 마무리
          this.pause();
          this.finishRealBackup(cur);
        }
      }
      return;
    }
    if (cur.id === "restore" && REAL_STEPS.restore) {
      if (cur.status === "running" && this.restoreRanGen !== this.runGen) {
        this.restoreRanGen = this.runGen;
        this.pause();
        void this.runRealRestore(cur);
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
    const list = await api.deviceList();
    const d = list?.find((x) => x.state === "device" && x.serial === this.device?.serial);
    if (!d) return false;
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

  /** 자동 감지: 조건이 충족될 때까지 주기적으로 확인 → 충족되면 자동 진행 (수동 [완료]도 가능) */
  private watchManual(cur: RunStep, id: ManualId, label: string, check: () => Promise<boolean>, everyMs: number) {
    this.manualWatching = label;
    const gen = this.runGen;
    let busy = false;
    this.watchTimer = setInterval(async () => {
      if (this.manualCurrent?.id !== id || gen !== this.runGen) return this.stopWatch();
      if (busy) return;
      busy = true;
      try {
        const ok = await check();
        if (gen !== this.runGen || this.manualCurrent?.id !== id) return; // 그사이 중단·진행됨
        if (ok) {
          this.stopWatch();
          cur.logs.push(`[감지] ${label} — 자동으로 진행합니다`);
          this.ackManual();
        }
      } finally {
        busy = false;
      }
    }, everyMs);
  }

  private stopWatch() {
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
      if (!this.firmware) {
        cur.status = "running";
        cur.logs.push(`[자동] Sony 서버에서 순정 펌웨어의 ${this.partition ?? "부트"} 이미지 받는 중`);
        await this.fetchFirmware();
        if (gen !== this.runGen) return; // 받는 사이 중단·처음으로
      }
      if (this.firmware) {
        this.logFirmware(cur);
        cur.manualDone++;
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
    this.onManualOpen(id);
    cur.logs.push(`[대기] 수동 개입: ${this.manualCurrent.title}`);
    void this.persist(true);
    if (id === "usb-debug") this.watchManual(cur, id, "USB 디버깅 연결 확인", () => this.usbDebugReady(), 2000);
    if (id === "mode-wait") this.watchManual(cur, id, "부트로더(fastboot) 모드 진입 확인", () => this.usbModeIs("fastboot"), 1500);
    if (id === "flash-mode") this.watchManual(cur, id, "플래시 모드 진입 확인", () => this.usbModeIs("flashmode"), 1500);
    if (id === "ims-check") this.watchManual(cur, id, "VoLTE(IMS) 등록 확인", () => this.imsReady(), 5000);
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
    }
  }

  /** SMS Import/Export 앱 준비 — 설치(필요 시 GitHub 다운로드)·권한·임시 폴더 */
  async smsiePrepare() {
    const cur = this.runSteps[this.cursor];
    const r = await api.smsiePrepare(this.device?.serial, true);
    if (r.ok) {
      cur?.logs.push(`[준비] SMS Import/Export — ${r.value.join(" · ")}`);
    } else {
      cur?.logs.push(`[실패] SMS Import/Export 준비: ${r.error}`);
    }
    void this.persist(true);
  }

  async openPhoneSettings() {
    const screen = this.device?.prep.developerOptions === false ? "about" : "developer";
    await api.openSettingsScreen(this.device?.serial, screen);
  }

  async loadImei() {
    this.imeiState = "loading";
    this.imei1 = await api.readImei1(this.device?.serial);
    this.imeiState = this.imei1 ? "done" : "failed";
  }

  async fetchFirmware() {
    const gen = this.runGen;
    const partition = this.partition;
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
    if (gen !== this.runGen) return;
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
  /** 리락 전 통신 확인 — 실제 발신·수신을 확인했다는 체크 */
  callAck = $state(false);
  /** 언락 조건 중 "확인 불가" 항목을 폰에서 직접 켰다고 확인 */
  oemUnknownAck = $state(false);
  firmwareDirInfo: { file: string; imageBytes: number } | null = $state(null);
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
    return !!id && ["oem-toggle", "usb-debug", "mode-wait", "flash-mode", "su-grant", "ims-check", "ims-precheck", "smsie-export"].includes(id);
  }

  /** 목업 실행에서만 — 폰이 실제로 재부팅되지 않아 확인할 수 없는 단계 건너뛰기 */
  get manualSkippable(): boolean {
    return SIMULATED_RUN && (this.manualVerifiable || this.manualCurrent?.id === "firmware-select");
  }

  /** 직접 지정한 펌웨어 폴더 — 고르는 즉시 검사 */
  async setFirmwareDir(dir: string) {
    this.firmwareDir = dir;
    this.firmwareDirInfo = null;
    this.firmwareDirError = "";
    const partition = this.partition;
    if (!partition) {
      this.firmwareDirState = "failed";
      this.firmwareDirError = "이 기종의 대상 파티션이 확인되지 않아 폴더를 검사할 수 없습니다";
      return;
    }
    this.firmwareDirState = "loading";
    const r = await api.firmwareDirCheck(dir, partition);
    if (this.firmwareDir !== dir) return;
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
    cur.logs.push(`[실전] 백업 시작 — 항목 ${items.length}개 → ${dest}`);
    // 진행 이벤트 구독 → progress·sub 체크포인트 반영
    const itemOrder = items.slice();
    const markItemDone = (itemId: string) => {
      if (!cur.sub) return;
      const idx = itemOrder.indexOf(itemId);
      if (idx >= 0 && cur.sub.done < idx + 1) {
        for (let k = cur.sub.done; k <= idx; k++) cur.logs.push(`[체크포인트] ${BACKUP_ITEM_LABEL[itemOrder[k]] ?? itemOrder[k]} 완료`);
        cur.sub.done = idx + 1;
        void this.persist(true);
      }
    };
    let logGate = 0;
    const un = await api.onBackupProgress((p) => {
      if (gen !== this.runGen) return;
      const ratio = p.bytesTotal > 0 ? p.bytesDone / p.bytesTotal : p.filesTotal > 0 ? p.filesDone / p.filesTotal : 0;
      const idx = Math.max(0, itemOrder.indexOf(p.itemId));
      cur.progress = Math.min(0.99, (idx + ratio) / Math.max(1, itemOrder.length));
      if (p.filesDone >= p.filesTotal && p.filesTotal > 0) markItemDone(p.itemId);
      else if (p.file && logGate++ % 25 === 0) {
        cur.logs.push(`[백업] ${BACKUP_ITEM_LABEL[p.itemId] ?? p.itemId} — ${p.file}`);
      }
      void this.persist();
    });
    const r = await api.backupRun(this.device?.serial, items, dest, this.backupDir || undefined);
    un();
    if (gen !== this.runGen) return; // 중단·처음으로
    if (!r.ok) {
      this.failStep(`백업 실패: ${r.error}`);
      return;
    }
    this.backupDir = r.value.dir;
    this.backupSummary = r.value;
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

  /** 실전 복구 러너 — restore 엔진 연동은 restore 커밋에서 채운다 (플래그 기본 꺼짐) */
  private async runRealRestore(_cur: RunStep) {
    if (!this.backupDir) {
      this.failStep("복구할 백업 폴더가 없습니다");
      return;
    }
    // TODO(restore): restore_run 이벤트 구독·연결 — 복구 엔진 커밋에서 구현
    this.failStep("복구 엔진이 아직 연결되지 않았습니다");
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

  abort() {
    this.runGen++;
    this.stepError = "";
    this.pause();
    this.stopWatch();
    // 진행 중인 실전 백업이 있으면 백엔드에도 취소 전달
    void api.backupCancel();
    // 진행 중·수동 대기 단계 모두 대기 상태로 (사이드바 스피너가 남지 않도록), 수동 개입은 처음부터 다시
    this.runSteps.forEach((s) => {
      if (s.status === "running" || s.status === "manual-wait") {
        s.status = "pending";
        s.manualDone = 0;
      } else if (s.status === "failed") {
        // 실패한 단계는 다시 시작할 때 처음부터
        s.status = "pending";
        s.progress = 0;
        s.manualDone = 0;
        s.sub = undefined;
      }
    });
    this.manualCurrent = null;
    this.usbError = false;
    this.backupNoticeAck = false;
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
    this.backupRanGen = -1;
    this.restoreRanGen = -1;
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
    if (this.journalKey) void api.journalArchive(this.journalKey, "done");
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
