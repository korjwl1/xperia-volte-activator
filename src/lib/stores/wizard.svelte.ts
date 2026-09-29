// 위자드 상태 머신 + 실행 시뮬레이션 러너 (mock)
import type { BackupGroup, DeviceStatus, EnvCheckItem, PlanStep, Profile, RunStep } from "$lib/types";
import { mockDeviceStatus, mockEnvChecks } from "$lib/mock/device";
import { mockBackupGroups } from "$lib/mock/apps";
import { buildPlan } from "$lib/mock/plan";

export type WizardView = "device" | "plan" | "backup-select" | "backup-target" | "run";

const MANUAL_TEXT: Record<string, { title: string; steps: string[] }> = {
  "mode-wait": {
    title: "부트로더 모드 진입 대기",
    steps: ["폰에서 재부팅 후 파란색 LED(부트로더) 확인", "USB 연결 유지", "자동 감지되면 다음 단계로 진행됩니다"],
  },
  "usb-debug": {
    title: "USB 디버깅 승인 대기",
    steps: ["초기 세팅 완료 후 개발자 옵션 활성화", "USB 디버깅 켜기", "PC 연결 시 폰 화면의 허용 프롬프트에서 '허용'"],
  },
  "magisk-patch": {
    title: "Magisk 부트 패치 (폰 조작)",
    steps: [
      "PC에서 추출한 init_boot.img를 폰의 Download 폴더로 전송 (자동)",
      "폰의 Magisk 앱 → 설치 → 파일 선택 및 패치",
      "패치 산출물(magisk_patched-*.img)은 자동으로 감지됩니다 — 과거 파일은 거부됩니다",
    ],
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
  profile = $state<Profile>("clean-return");
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

  // ── 탐색 ──────────────────────────────
  async refreshDevice() {
    // contract: device_list / device_status (mock 즉시 반환)
    this.device = mockDeviceStatus;
    this.env = mockEnvChecks;
  }

  goPlan() {
    this.applyProfile(this.profile);
    this.view = "plan";
  }

  applyProfile(p: Profile) {
    this.profile = p;
    this.steps = buildPlan(p);
    this.lastDepNotice = "";
  }

  toggleStep(id: string, on: boolean) {
    const overrides: Record<string, boolean> = {};
    for (const s of this.steps) if (s.optional) overrides[s.id] = s.enabled;
    overrides[id] = on;
    const before = this.steps.find((s) => s.id === "relock")?.enabled ?? false;
    this.steps = buildPlan(this.profile, overrides);
    const after = this.steps.find((s) => s.id === "relock")?.enabled ?? false;
    if (before && !after) {
      this.lastDepNotice = "의존성 규칙: 언루팅을 끄면 리락도 함께 해제됩니다 — 수정된 boot로 oem lock 시 AVB 검증 실패 위험 (§3-3)";
    } else if (!on) {
      this.lastDepNotice = "";
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

  confirmPlan() {
    this.groups = $state.snapshot(mockBackupGroups.map((g) => ({ ...g, items: g.items.map((i) => ({ ...i })) })));
    this.skipBackup = false;
    this.view = this.hasWipeRoute ? "backup-select" : "run";
    if (this.view === "run") this.prepareRun();
  }

  setGroupAll(gid: string, on: boolean) {
    const g = this.groups.find((x) => x.id === gid);
    if (!g) return;
    for (const it of g.items) if (it.cls !== "none") it.checked = on;
  }

  get selectedBytes(): number {
    return this.groups.reduce(
      (acc, g) => acc + g.items.reduce((a, i) => a + (i.checked ? i.bytes ?? 0 : 0), 0),
      0,
    );
  }

  get anyChecked(): boolean {
    return this.groups.some((g) => g.items.some((i) => i.checked));
  }

  skipBackupFlow() {
    this.skipBackup = true;
    this.backupPath = "";
    this.view = "run";
    this.prepareRun();
  }

  startRun() {
    this.view = "run";
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
    // USB 오류 시뮬레이션 (§9-4 데모): 백업/EFS 전송 중 1회
    if (this.simulateUsbError && !this.erroredOnce &&
        (cur.id.startsWith("backup") || cur.id === "efs")) {
      this.erroredOnce = true;
      this.usbErrorCount++;
      this.usbError = true;
      this.pause();
      cur.logs.push("[오류] DIAG 전송 타임아웃 — USB 연결이 불안정합니다 (재시도 카운트 3/3)");
      return;
    }
    // 수동 개입 지점
    const step = this.steps.find((s) => s.id === cur.id);
    if (step?.manual && cur.progress === 0) {
      cur.status = "manual-wait";
      this.manualCurrent = MANUAL_TEXT[step.manual] ?? { title: "폰 조작 필요", steps: [] };
      this.pause();
      cur.logs.push(`[대기] 수동 개입: ${this.manualCurrent.title}`);
      return;
    }
    cur.progress = Math.min(1, cur.progress + 0.04 + Math.random() * 0.05);
    if (Math.random() < 0.35) {
      cur.logs.push(this.mockLog(cur.id, cur.progress));
    }
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
      case "backup-1":
      case "backup-2":
        return `adb pull -a ... ${pct}% (sha256 스트리밍 계산 중)`;
      case "unlock":
        return `fastboot oem unlock 0x{마스킹됨} ... ${pct}%`;
      case "root":
        return `fastboot flash init_boot_a/b ... ${pct}%`;
      case "efs-preflight":
        return ["토폴로지: 루트 허브 직결 ✅", "컨트롤러: Intel xHCI ✅", "전원 관리: 임시 해제 → 종료 시 원복", "DIAG 건전성: 리드백 1.2MB 재시도 0회 ✅"][Math.floor(p * 4) % 4];
      case "efs":
        return `uploadDirectory → / (${Math.floor(p * 46)}/46 파일) 리드백 해시 일치`;
      case "verify":
        return `전수 리드백 sha256 ... ${pct}% (불일치 0건)`;
      case "unroot":
        return `fastboot flash init_boot_a/b (순정 해시 4f3a…c91 일치) ... ${pct}%`;
      case "relock":
        return `fastboot oem lock — 리락 게이트 통과(언루팅+순정 근거) ... ${pct}%`;
      case "final-verify":
        return ["persist.dbg.volte_avail_ovr = (소실 — /data 초기화, 예상 동작)", "모뎀 자체 IMS 등록 대기 중… (재부팅 1/3)", "IMS: REGISTERED ✅ VoLTE 사용 가능"][Math.floor(p * 3) % 3];
      case "restore":
        return `앱 재설치 n/N · 파일 push -a ... ${pct}%`;
      default:
        return `진행 ${pct}%`;
    }
  }

  ackManual() {
    const cur = this.runSteps[this.cursor];
    if (cur) {
      cur.status = "running";
      cur.progress = 0.01;
    }
    this.manualCurrent = null;
    this.begin();
  }

  dismissUsbError() {
    this.usbError = false;
    const cur = this.runSteps[this.cursor];
    if (cur) {
      cur.status = "running";
      cur.logs.push("[재개] 재연결 감지 — 단계 상대 프로브로 이어서 진행 (§9-2)");
    }
    this.begin();
  }

  abort() {
    this.pause();
    this.runSteps.forEach((s) => {
      if (s.status === "running") s.status = "pending";
    });
    this.manualCurrent = null;
    this.usbError = false;
  }

  private complete() {
    this.pause();
    this.finished = true;
  }

  get currentIdx(): number {
    return this.cursor;
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
