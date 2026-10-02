// 위자드 상태 머신 + 실행 시뮬레이션 러너 (mock)
import type { AppItem, FirmwareResult, FirmwareVersions, SettingsOverview, SimInfo, BackupGroup, DeviceStatus, EnvCheckItem, ManualId, ManualPrompt, PlanStep, RunStep, VolteConfig } from "$lib/types";
import { api } from "$lib/api";
import { LINKS, maskSecret } from "$lib/data/links";
import { bootPartition } from "$lib/data/devices";
import { mockBackupGroups } from "$lib/mock/apps";
import { bootloaderOnly, buildPlan, updateTarget, type PlanOptions } from "$lib/mock/plan";

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
  "ims-check": {
    title: "최종 VoLTE 확인",
    steps: [
      "폰이 완전히 부팅되고 통신사 신호를 잡을 때까지 기다립니다 (2~3분)",
      "앱이 IMS(VoLTE) 등록 상태를 자동으로 확인합니다 — 등록되면 바로 다음 단계로 진행됩니다",
      "직접 확인하려면: 전화 앱 → *#*#4636#*#* → 휴대전화 정보 → IMS 서비스 상태",
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
      this.pause();
      void this.openManual(cur, id);
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
    return (await api.usbModes())?.some((m) => m.mode === mode) ?? false;
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
    let busy = false;
    this.watchTimer = setInterval(async () => {
      if (this.manualCurrent?.id !== id) return this.stopWatch();
      if (busy) return;
      busy = true;
      try {
        if (await check()) {
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
    if (id === "usb-debug" && (await this.usbDebugReady())) {
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
    this.manualCurrent = { id, ...MANUAL_TEXT[id] };
    this.onManualOpen(id);
    cur.logs.push(`[대기] 수동 개입: ${this.manualCurrent.title}`);
    if (id === "usb-debug") this.watchManual(cur, id, "USB 디버깅 연결 확인", () => this.usbDebugReady(), 2000);
    if (id === "mode-wait") this.watchManual(cur, id, "부트로더(fastboot) 모드 진입 확인", () => this.usbModeIs("fastboot"), 1500);
    if (id === "flash-mode") this.watchManual(cur, id, "플래시 모드 진입 확인", () => this.usbModeIs("flashmode"), 1500);
    if (id === "ims-check") this.watchManual(cur, id, "VoLTE(IMS) 등록 확인", () => this.imsReady(), 5000);
  }

  /** 수동 개입이 열릴 때 자동 동작 — 언락: 발급 페이지 열기 + IMEI 읽기 / 펌웨어: 자동 다운로드 */
  private onManualOpen(id: ManualId) {
    if (id === "unlock-code") {
      void api.openExternal(LINKS.unlock);
      void this.loadImei();
    } else if (id === "oem-toggle") {
      // 폰에 해당 설정 화면을 바로 띄움 (개발자 옵션이 꺼져 있으면 휴대전화 정보 — 빌드번호 연타)
      void this.openPhoneSettings();
    }
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
        : `[입력] 펌웨어 폴더: ${this.firmwareDir}`,
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

  /** 입력형 수동 개입은 값이 채워져야 완료 가능 */
  get manualInputReady(): boolean {
    const m = this.manualCurrent;
    if (m?.id === "backup-notice") return this.backupNoticeAck;
    if (m?.id === "oem-toggle") return this.prepMissing.length === 0 && !this.prepChecking;
    if (!m?.input) return true;
    if (m.input === "unlock-code") return this.unlockCodeValid;
    return this.firmware !== null || this.firmwareDir.trim().length > 0;
  }

  ackManual() {
    if (!this.manualInputReady) return;
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
  }

  dismissUsbError() {
    this.usbError = false;
    const cur = this.runSteps[this.cursor];
    if (cur) { cur.status = "running"; cur.logs.push("[재개] 재연결 확인 — 이어서 진행합니다"); }
    this.begin();
  }

  abort() {
    this.pause();
    this.stopWatch();
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
