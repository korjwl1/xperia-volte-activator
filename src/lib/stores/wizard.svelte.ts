// 위자드 상태 머신 + 실행 시뮬레이션 러너 (mock)
import type { BackupGroup, DeviceStatus, EnvCheckItem, PlanStep, RunStep, VolteConfig } from "$lib/types";
import { api } from "$lib/api";
import { mockBackupGroups } from "$lib/mock/apps";
import { buildPlan } from "$lib/mock/plan";

export type WizardView = "device" | "step1" | "step2" | "step3" | "step4";

export const MACRO_STEPS = [
  { id: 1, view: "step1" as const, label: "SIM 및 통신사 선택" },
  { id: 2, view: "step2" as const, label: "작업 옵션 선택" },
  { id: 3, view: "step3" as const, label: "VoLTE 패치 진행" },
  { id: 4, view: "step4" as const, label: "점검 및 마무리" },
] as const;

const MANUAL_TEXT: Record<string, { title: string; steps: string[] }> = {
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
    steps: ["PC에서 준비한 시스템 파일을 폰으로 전송합니다 (자동)", "폰의 Magisk 앱 → 설치 → 파일 선택 및 패치", "패치가 완료되면 자동으로 감지됩니다"],
  },
  "ims-check": {
    title: "최종 IMS 등록 확인",
    steps: ["폰이 완전히 부팅될 때까지 대기 (2~3분)", "전화 앱 → *#*#4636#*#* → 휴대전화 정보 → IMS 서비스 상태", "VoLTE 사용 가능으로 표시되는지 확인"],
  },
};

export class Wizard {
  view = $state<WizardView>("device");
  device: DeviceStatus | null = $state(null);
  env: EnvCheckItem[] = $state([]);
  volteConfig = $state<VolteConfig>({ simSlot: 2, carrier: "SKT", mode: "balance" });
  steps: PlanStep[] = $state([]);
  groups: BackupGroup[] = $state([]);
  backupPath = $state("");
  skipBackup = $state(false);
  lastDepNotice = $state("");

  // 실행 상태
  runSteps: RunStep[] = $state([]);
  running = $state(false);
  finished = $state(false);
  manualCurrent: { title: string; steps: string[] } | null = $state(null);
  usbError = $state(false);
  usbErrorCount = $state(0);
  simulateUsbError = $state(false);
  private erroredOnce = false;
  private timer: ReturnType<typeof setInterval> | undefined;
  private cursor = 0;

  get macroStepIdx(): number {
    return MACRO_STEPS.findIndex((s) => s.view === this.view);
  }

  async refreshDevice() {
    // contract: device_list / env_check (데스크톱=실측, 브라우저 dev=mock 폴백)
    const list = (await api.deviceList()) ?? [];
    this.device = list.length === 1 ? list[0] : null;
    this.env = await api.envCheck();
  }

  goStep1() {
    this.view = "step1";
  }

  applyVolteConfig() {
    this.steps = buildPlan(this.device, this.volteConfig);
    this.lastDepNotice = "";
  }

  toggleStep(id: string, on: boolean) {
    const overrides: Record<string, boolean> = {};
    for (const s of this.steps) if (s.optional) overrides[s.id] = s.enabled;
    overrides[id] = on;
    const before = this.steps.find((s) => s.id === "relock")?.enabled ?? false;
    this.steps = buildPlan(this.device, this.volteConfig, overrides);
    const after = this.steps.find((s) => s.id === "relock")?.enabled ?? false;
    if (before && !after) {
      this.lastDepNotice = "언루팅을 끄면 리락도 함께 해제됩니다 — 수정된 시스템으로 잠그면 부팅 불능 위험이 있습니다";
    } else {
      this.lastDepNotice = "";
    }
  }

  get hasWipeRoute(): boolean {
    return this.steps.some((s) => s.enabled && s.wipe);
  }

  get backupSkippable(): boolean {
    return this.steps.some((s) => s.id === "backup-1" && s.enabled);
  }

  confirmStep2() {
    this.groups = $state.snapshot(mockBackupGroups.map((g) => ({ ...g, items: g.items.map((i) => ({ ...i })) })));
    this.skipBackup = false;
    this.view = "step3";
    this.prepareRun();
  }

  setGroupAll(gid: string, on: boolean) {
    const g = this.groups.find((x) => x.id === gid);
    if (!g) return;
    for (const it of g.items) if (it.cls !== "none") it.checked = on;
  }

  get selectedBytes(): number {
    return this.groups.reduce((acc, g) => acc + g.items.reduce((a, i) => a + (i.checked ? i.bytes ?? 0 : 0), 0), 0);
  }

  get anyChecked(): boolean {
    return this.groups.some((g) => g.items.some((i) => i.checked));
  }

  skipBackupFlow() {
    this.skipBackup = true;
    this.backupPath = "";
    this.view = "step3";
    this.prepareRun();
  }

  startRun() {
    this.view = "step3";
    this.prepareRun();
    this.begin();
  }

  // ── 실행 시뮬레이션 ───────────────────
  prepareRun() {
    this.runSteps = this.steps
      .filter((s) => s.enabled)
      .map((s) => ({ id: s.id, title: s.title, status: "pending", progress: 0, logs: [] }));
    this.cursor = 0;
    this.finished = false;
    this.usbError = false;
    this.erroredOnce = false;
  }

  begin() {
    if (this.running) return;
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
    if (this.simulateUsbError && !this.erroredOnce && (cur.id.startsWith("backup") || cur.id === "efs")) {
      this.erroredOnce = true;
      this.usbErrorCount++;
      this.usbError = true;
      this.pause();
      cur.logs.push("[오류] DIAG 전송 타임아웃 — USB 연결이 불안정합니다 (재시도 카운트 3/3)");
      return;
    }
    const step = this.steps.find((s) => s.id === cur.id);
    if (step?.manual && cur.progress === 0) {
      cur.status = "manual-wait";
      this.manualCurrent = MANUAL_TEXT[step.manual] ?? { title: "폰 조작 필요", steps: [] };
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
      case "backup-1": return `파일 복사 중… ${pct}%`;
      case "unlock": return `잠금 해제 중… ${pct}%`;
      case "root": return `시스템 패치 중… ${pct}%`;
      case "efs-preflight": return ["USB 연결 확인", "드라이버 확인", "전원 관리 일시 해제", "연결 안정성 테스트 통과"][Math.floor(p * 4) % 4];
      case "efs": return `프로파일 적용 중… (${Math.floor(p * 46)}/46 파일)`;
      case "verify": return `무결성 검증 중… ${pct}%`;
      case "unroot": return `시스템 복원 중… ${pct}%`;
      case "relock": return `잠금 중… ${pct}%`;
      case "final-verify": return ["재부팅 대기 중…", "네트워크 등록 대기 중…", "VoLTE 활성 확인됨"][Math.floor(p * 3) % 3];
      case "restore": return `데이터 복원 중… ${pct}%`;
      default: return `진행 ${pct}%`;
    }
  }

  ackManual() {
    const cur = this.runSteps[this.cursor];
    if (cur) { cur.status = "running"; cur.progress = 0.01; }
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
    this.runSteps.forEach((s) => { if (s.status === "running") s.status = "pending"; });
    this.manualCurrent = null;
    this.usbError = false;
  }

  goFinish() {
    this.view = "step4";
  }

  restart() {
    this.view = "device";
    this.device = null;
    this.env = [];
    this.steps = [];
    this.groups = [];
    this.runSteps = [];
    this.finished = false;
    this.running = false;
    this.backupPath = "";
    this.skipBackup = false;
    this.cursor = 0;
  }

  private complete() {
    this.pause();
    this.finished = true;
  }

  get currentIdx(): number { return this.cursor; }

  get overall(): number {
    if (!this.runSteps.length) return 0;
    const total = this.runSteps.length;
    const done = this.runSteps.filter((s) => s.status === "done").length;
    const cur = this.runSteps.find((s) => s.status === "running" || s.status === "manual-wait");
    return (done + (cur?.progress ?? 0)) / total;
  }
}

export const wizard = new Wizard();
