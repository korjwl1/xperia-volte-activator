// 위자드 상태 머신 + 실행 시뮬레이션 러너 (mock)
import type { AppItem, SettingsOverview, BackupGroup, DeviceStatus, EnvCheckItem, ManualId, ManualPrompt, PlanStep, RunStep, VolteConfig } from "$lib/types";
import { api } from "$lib/api";
import { maskSecret } from "$lib/data/links";
import { bootPartition } from "$lib/data/devices";
import { mockBackupGroups } from "$lib/mock/apps";
import { buildPlan, type PlanOptions } from "$lib/mock/plan";

export type WizardView = "device" | "warning" | "step1" | "step2" | "step3" | "step4";

export const MACRO_STEPS = [
  { id: 1, view: "step1" as const, label: "SIM 및 통신사 선택" },
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
    title: "언락 코드 입력",
    input: "unlock-code",
    steps: [
      "아래 버튼으로 언락 코드 발급 사이트를 열고, 쿠키 팝업이 뜨면 Accept Optional Cookies 선택",
      "Select your device에서 기기 모델 선택 (목록에 없는 최신 기종은 다른 최신 기종 아무거나)",
      "IMEI 입력란에 SIM 슬롯 1번의 IMEI 입력 (설정 > 휴대전화 정보 > IMEI(SIM 슬롯 1) 또는 패키지 박스의 IMEI 1)",
      "동의 체크 후 Submit → reCAPTCHA 수행 → 표시된 언락 코드를 복사해 아래에 붙여넣기",
    ],
  },
  "firmware-select": {
    title: "펌웨어 폴더 선택",
    input: "firmware",
    steps: [
      "XperiFirm 등으로 현재 기기와 같은 버전의 펌웨어를 내려받습니다",
      "펌웨어 폴더(init_boot / boot .sin 파일이 있는 폴더)를 아래에서 선택합니다",
      "언루팅 시 사용할 순정 이미지도 같은 펌웨어에서 추출합니다",
    ],
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
  "mode-wait": {
    title: "부트로더 모드 진입 대기",
    steps: ["폰에서 재부팅 후 파란색 LED(부트로더) 확인", "USB 연결 유지", "자동 감지되면 다음 단계로 진행됩니다"],
  },
  "usb-debug": {
    title: "USB 디버깅 승인 대기",
    steps: ["재부팅 후 개발자 옵션 활성화", "USB 디버깅 켜기", "PC 연결 시 폰 화면에서 '허용' 선택"],
  },
  "magisk-patch": {
    title: "Magisk 부트 패치 (폰 조작)",
    steps: [
      "Magisk 앱 설치와 시스템 이미지 전송은 자동으로 진행됩니다",
      "폰의 Magisk 앱 → Magisk 영역의 설치 → 파일 선택 및 패치 → 전송된 img 파일 선택",
      "패치가 완료되면 자동으로 감지됩니다",
    ],
  },
  "ims-check": {
    title: "최종 IMS 등록 확인",
    steps: ["폰이 완전히 부팅될 때까지 대기 (2~3분)", "전화 앱 → *#*#4636#*#* → 휴대전화 정보 → IMS 서비스 상태", "VoLTE 사용 가능으로 표시되는지 확인"],
  },
};

const defaultVolteConfig = (): VolteConfig => ({
  sims: [
    { slot: 1, carrier: null },
    { slot: 2, carrier: null },
  ],
});

export class Wizard {
  view = $state<WizardView>("device");
  device: DeviceStatus | null = $state(null);
  env: EnvCheckItem[] = $state([]);
  volteConfig = $state<VolteConfig>(defaultVolteConfig());

  /** 패치할 슬롯이 하나라도 있는지 (1단계 [다음] 활성 조건) */
  get hasPatchTarget(): boolean {
    return this.volteConfig.sims.some((s) => s.carrier !== null);
  }

  // 경고 페이지 동의 (뒤로 왔다 다시 와도 유지, 처음으로 가면 초기화)
  omdAck = $state(false);
  riskAck = $state(false);

  // ── 2단계(작업 옵션 선택) — 이전/다음 이동 시 유지, 다른 기기거나 처음으로 가면 초기화 ──
  groups: BackupGroup[] = $state([]); // 백업 항목 (항목 단위 checked)
  opts = $state<PlanOptions>({ unroot: true, relock: true, restore: true });
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
    const key = d.serial ?? d.serialMasked;
    if (this.optionsFor === key) return;
    this.optionsFor = key;
    // 초기화 경로(잠긴 기기의 언락, 또는 기본 ON인 리락)가 있으면 백업 기본 전체 선택
    const defaultOn = d.bootloader === "locked" || d.bootloader === "unlocked";
    this.groups = mockBackupGroups.map((g) => ({
      ...g,
      items: g.items.map((i) => ({ ...i, checked: defaultOn && i.checked })),
    }));
    this.opts = { unroot: true, relock: true, restore: defaultOn };
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
  // 실행 중 입력값 — 언락 코드는 UI/로그에 마스킹해서만 표시
  unlockCode = $state("");
  firmwareDir = $state("");
  usbError = $state(false);
  usbErrorCount = $state(0);
  simulateUsbError = $state(false);
  private erroredOnce = false;
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
    this.begin();
  }

  // ── 실행 시뮬레이션 ───────────────────
  prepareRun() {
    this.runSteps = this.steps
      .filter((s) => s.enabled)
      .map((s) => ({ id: s.id, title: s.title, status: "pending", progress: 0, logs: [], manualDone: 0 }));
    this.cursor = 0;
    this.finished = false;
    this.usbError = false;
    this.erroredOnce = false;
  }

  begin() {
    if (this.running || this.usbError) return;
    this.running = true;
    this.timer = setInterval(() => this.tick(), 140);
  }

  pause() {
    this.running = false;
    if (this.timer) clearInterval(this.timer);
    this.timer = undefined;
  }

  private tick() {
    const cur = this.runSteps[this.cursor];
    if (!cur) return this.complete();
    if (cur.status === "pending") {
      cur.status = "running";
      cur.logs.push(`[시작] ${cur.title}`);
    }
    if (this.simulateUsbError && !this.erroredOnce && (cur.id === "backup" || cur.id === "efs")) {
      this.erroredOnce = true;
      this.usbErrorCount++;
      this.usbError = true;
      this.pause();
      cur.logs.push("[오류] DIAG 전송 타임아웃 — USB 연결이 불안정합니다 (재시도 카운트 3/3)");
      return;
    }
    const step = this.steps.find((s) => s.id === cur.id);
    const manuals = step?.manual ?? [];
    if (cur.manualDone < manuals.length) {
      const id = manuals[cur.manualDone];
      cur.status = "manual-wait";
      this.manualCurrent = { id, ...MANUAL_TEXT[id] };
      this.pause();
      cur.logs.push(`[대기] 수동 개입: ${this.manualCurrent.title}`);
      return;
    }
    cur.progress = Math.min(1, cur.progress + 0.04 + Math.random() * 0.05);
    if (Math.random() < 0.35) cur.logs.push(this.mockLog(cur.id, cur.progress));
    if (cur.progress >= 1) {
      cur.status = "done";
      cur.logs.push("[완료]");
      this.cursor++;
      if (this.cursor >= this.runSteps.length) this.complete();
    }
  }

  private mockLog(id: string, p: number): string {
    const pct = Math.round(p * 100);
    switch (id) {
      case "backup": return `파일 복사 중… ${pct}%`;
      case "unlock": return `잠금 해제 중… ${pct}%`;
      case "root": return `시스템 패치 중… ${pct}%`;
      case "efs-preflight": return ["USB 연결 확인", "드라이버 확인", "전원 관리 일시 해제", "연결 안정성 테스트 통과"][Math.floor(p * 4) % 4];
      case "efs": return `프로파일 적용 중… (${Math.floor(p * 46)}/46 파일)`;
      case "verify": return `무결성 검증 중… ${pct}%`;
      case "volte-props": return ["VoLTE 설정 적용", "영상통화 설정 적용", "Wi-Fi 통화 설정 적용", "재부팅 중…"][Math.floor(p * 4) % 4];
      case "unroot": return `시스템 복원 중… ${pct}%`;
      case "relock": return `잠금 중… ${pct}%`;
      case "final-verify": return ["재부팅 대기 중…", "네트워크 등록 대기 중…", "VoLTE 활성 확인됨"][Math.floor(p * 3) % 3];
      case "restore": return `데이터 복원 중… ${pct}%`;
      default: return `진행 ${pct}%`;
    }
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

  /** 입력형 수동 개입은 값이 채워져야 완료 가능 */
  get manualInputReady(): boolean {
    const m = this.manualCurrent;
    if (m?.id === "backup-notice") return this.backupNoticeAck;
    if (m?.id === "oem-toggle") return this.prepMissing.length === 0 && !this.prepChecking;
    if (!m?.input) return true;
    if (m.input === "unlock-code") return this.normalizedUnlockCode.length > 0;
    return this.firmwareDir.trim().length > 0;
  }

  ackManual() {
    if (!this.manualInputReady) return;
    const cur = this.runSteps[this.cursor];
    const m = this.manualCurrent;
    if (cur) {
      if (m?.input === "unlock-code") cur.logs.push(`[입력] 언락 코드: 0x${maskSecret(this.normalizedUnlockCode)}`);
      if (m?.input === "firmware") cur.logs.push(`[입력] 펌웨어 폴더: ${this.firmwareDir}`);
      cur.status = "running";
      cur.manualDone++;
    }
    this.manualCurrent = null;
    this.begin();
  }

  dismissUsbError() {
    this.usbError = false;
    const cur = this.runSteps[this.cursor];
    if (cur) { cur.status = "running"; cur.logs.push("[재개] 재연결 확인 — 이어서 진행합니다"); }
    this.begin();
  }

  abort() {
    this.pause();
    // 진행 중·수동 대기 단계 모두 대기 상태로 (사이드바 스피너가 남지 않도록), 수동 개입은 처음부터 다시
    this.runSteps.forEach((s) => {
      if (s.status === "running" || s.status === "manual-wait") {
        s.status = "pending";
        s.manualDone = 0;
      }
    });
    this.manualCurrent = null;
    this.usbError = false;
    this.backupNoticeAck = false;
  }

  goFinish() {
    this.view = "step4";
  }

  /** [처음으로] — 모든 선택·입력·실행 상태 초기화 */
  restart() {
    this.pause();
    this.view = "device";
    this.device = null;
    this.env = [];
    this.volteConfig = defaultVolteConfig();
    this.omdAck = false;
    this.riskAck = false;
    this.groups = [];
    this.opts = { unroot: true, relock: true, restore: true };
    this.backupPath = "";
    this.sizes = null;
    this.sizesState = "idle";
    this.appClasses = null;
    this.appClassesState = "idle";
    this.settingsInfo = null;
    this.settingsInfoState = "idle";
    this.backupNoticeAck = false;
    this.optionsFor = null;
    this.steps = [];
    this.runSteps = [];
    this.finished = false;
    this.manualCurrent = null;
    this.unlockCode = "";
    this.firmwareDir = "";
    this.usbError = false;
    this.usbErrorCount = 0;
    this.simulateUsbError = false;
    this.erroredOnce = false;
    this.cursor = 0;
  }

  private complete() {
    this.pause();
    this.finished = true;
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
