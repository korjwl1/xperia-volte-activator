<script lang="ts">
  import { Switch } from "$lib/components/ui/switch";
  import { Button } from "$lib/components/ui/button";
  import { Badge } from "$lib/components/ui/badge";
  import { Input } from "$lib/components/ui/input";
  import { TriangleAlert, FolderOpen, Play } from "@lucide/svelte/icons";
  import { wizard } from "$lib/stores/wizard.svelte";
  import { buildPlan } from "$lib/mock/plan";
  import { mockBackupGroups } from "$lib/mock/apps";

  let activeTab = $state<"backup" | "rooting">("backup");

  // 백업 그룹 (recovery.md 설계 — 요소별 선택)
  let backupGroups = $state(
    mockBackupGroups.map((g) => ({
      id: g.id,
      label: g.label,
      desc: g.desc,
      checked: true,
    })),
  );

  // 옵션
  let opts = $state({
    restore: true,
    backup2: true,
    unroot: true,
    relock: true,
  });

  // 선택에 따라 동적으로 재구성되는 실행 목록
  const planSteps = $derived.by(() => {
    const steps: { title: string; warn?: boolean }[] = [];
    const d = wizard.device;
    if (!d) return steps;

    if (backupGroups.some((g) => g.checked)) {
      steps.push({ title: "백업" });
    }
    if (d.bootloader === "locked") {
      steps.push({ title: "부트로더 언락", warn: true });
      steps.push({ title: "기본 설정" });
    }
    if (d.rooted !== true) {
      steps.push({ title: "루팅" });
    }
    steps.push({ title: "연결 안정성 검사" });
    steps.push({ title: "VoLTE 적용" });
    steps.push({ title: "적용 확인" });
    if (d.bootloader === "locked") {
      if (opts.unroot) steps.push({ title: "언루팅" });
      if (opts.backup2 && backupGroups.some((g) => g.checked)) steps.push({ title: "2차 백업" });
      if (opts.relock) steps.push({ title: "부트로더 리락", warn: true });
      steps.push({ title: "최종 확인" });
      if (opts.restore && backupGroups.some((g) => g.checked)) steps.push({ title: "복구" });
    }
    return steps;
  });

  // step3로 전달
  function confirm() {
    // 의존성: 리락 ⟹ 언루팅
    const effUnroot = opts.unroot || opts.relock;

    // 백업 그룹 설정
    wizard.groups = mockBackupGroups.map((g) => ({
      ...g,
      items: g.items.map((i) => ({ ...i, checked: backupGroups.find((bg) => bg.id === g.id)?.checked ?? false })),
    }));

    // PlanStep[] 구성
    const steps: import("$lib/types").PlanStep[] = [];
    const d = wizard.device;
    if (!d) return;

    const push = (id: string, title: string, risk: "safe" | "warn" | "danger" = "safe", wipe = false, manual?: string, estSec = 120) =>
      steps.push({ id, kind: id as any, title, desc: "", optional: false, enabled: true, risk, wipe, estSec, manual: manual as any });

    const anyBackup = backupGroups.some((g) => g.checked);
    const needsUnlock = d.bootloader === "locked";

    if (anyBackup) push("backup-1", "백업", "warn", false, undefined, 1800);
    if (needsUnlock) {
      push("unlock", "부트로더 언락", "danger", true, "mode-wait", 120);
      push("setup-min", "기본 설정", "safe", false, "usb-debug", 300);
    }
    if (d.rooted !== true) push("root", "루팅", "warn", false, "magisk-patch", 600);
    push("efs-preflight", "연결 안정성 검사", "safe", false, undefined, 60);
    push("efs", "VoLTE 적용", "danger", false, undefined, 420);
    push("verify", "적용 확인", "safe", false, undefined, 120);
    if (needsUnlock) {
      if (effUnroot) push("unroot", "언루팅", "warn", false, undefined, 180);
      if (opts.backup2 && anyBackup) push("backup-2", "2차 백업", "warn", false, undefined, 900);
      if (opts.relock) push("relock", "부트로더 리락", "danger", true, "mode-wait", 120);
      push("final-verify", "최종 확인", "safe", false, "ims-check", 300);
      if (opts.restore && anyBackup) push("restore", "복구", "safe", false, undefined, 1500);
    }

    wizard.steps = steps;
    wizard.view = "step3";
    wizard.prepareRun();
    wizard.begin();
  }
</script>

<div class="flex-1 min-h-0 flex gap-4 p-4 lg:p-6">
  <!-- 좌: 옵션 선택 (더 넓게) -->
  <div class="flex-[3] min-w-0 flex flex-col gap-4">
    <!-- 탭 -->
    <div class="flex gap-1 rounded-lg bg-muted p-1">
      <button
        class="flex-1 rounded-md px-4 py-2 text-sm font-medium transition-colors
          {activeTab === 'backup' ? 'bg-background elev-1 text-foreground' : 'text-muted-foreground hover:text-foreground'}"
        onclick={() => (activeTab = "backup")}
      >
        백업 및 복구
      </button>
      <button
        class="flex-1 rounded-md px-4 py-2 text-sm font-medium transition-colors
          {activeTab === 'rooting' ? 'bg-background elev-1 text-foreground' : 'text-muted-foreground hover:text-foreground'}"
        onclick={() => (activeTab = "rooting")}
      >
        루팅
      </button>
    </div>

    <!-- 탭 콘텐츠 -->
    <div class="flex-1 min-h-0 overflow-y-auto space-y-2">
      {#if activeTab === "backup"}
        {#each backupGroups as group (group.id)}
          <div class="flex items-center gap-3 rounded-lg border bg-card px-4 py-3">
            <Switch
              checked={group.checked}
              onCheckedChange={(v: boolean) => (group.checked = v)}
              id="bg-{group.id}"
            />
            <div class="min-w-0 flex-1">
              <div class="text-sm font-medium">{group.label}</div>
              <div class="text-[11px] text-muted-foreground truncate">{group.desc}</div>
            </div>
          </div>
        {/each}

        <div class="h-px bg-border my-2"></div>

        <div class="flex items-center gap-3 rounded-lg border bg-card px-4 py-3">
          <Switch checked={opts.restore} onCheckedChange={(v: boolean) => (opts.restore = v)} id="opt-restore" />
          <div class="flex-1">
            <div class="text-sm font-medium">복구 자동 실행</div>
            <div class="text-[11px] text-muted-foreground">모든 작업 완료 후 백업한 데이터를 자동으로 복원합니다</div>
          </div>
        </div>
        <div class="flex items-center gap-3 rounded-lg border bg-card px-4 py-3">
          <Switch checked={opts.backup2} onCheckedChange={(v: boolean) => (opts.backup2 = v)} id="opt-b2" />
          <div class="flex-1">
            <div class="text-sm font-medium">2차 백업</div>
            <div class="text-[11px] text-muted-foreground">리락 직전에 언락 이후 생성된 데이터를 백업합니다</div>
          </div>
        </div>

      {:else}
        <!-- 루팅 탭 -->
        {#if wizard.device?.bootloader === "locked"}
          <div class="flex items-center gap-3 rounded-lg border bg-card px-4 py-3">
            <TriangleAlert size={16} class="text-warning shrink-0" />
            <div class="flex-1">
              <div class="text-sm font-medium">부트로더 언락 · 루팅 · VoLTE 적용</div>
              <div class="text-[11px] text-muted-foreground">부트로더가 잠겨 있어 자동으로 포함됩니다</div>
            </div>
            <Badge variant="secondary" class="text-[10px]">필수</Badge>
          </div>
        {:else if wizard.device?.rooted !== true}
          <div class="flex items-center gap-3 rounded-lg border bg-card px-4 py-3">
            <div class="flex-1">
              <div class="text-sm font-medium">루팅 · VoLTE 적용</div>
              <div class="text-[11px] text-muted-foreground">VoLTE 적용을 위해 필요합니다</div>
            </div>
            <Badge variant="secondary" class="text-[10px]">필수</Badge>
          </div>
        {:else}
          <div class="flex items-center gap-3 rounded-lg border bg-card px-4 py-3">
            <div class="flex-1">
              <div class="text-sm font-medium">VoLTE 적용</div>
              <div class="text-[11px] text-muted-foreground">이미 루팅되어 있어 언락/루팅 단계를 건너뜁니다</div>
            </div>
            <Badge variant="secondary" class="text-[10px]">필수</Badge>
          </div>
        {/if}

        {#if wizard.device?.bootloader === "locked"}
          <div class="h-px bg-border my-2"></div>
          <div class="flex items-center gap-3 rounded-lg border bg-card px-4 py-3">
            <Switch checked={opts.unroot} onCheckedChange={(v: boolean) => (opts.unroot = v)} id="opt-unroot" />
            <div class="flex-1">
              <div class="text-sm font-medium">언루팅</div>
              <div class="text-[11px] text-muted-foreground">시스템을 원래대로 되돌립니다 — 리락하려면 필요합니다</div>
            </div>
          </div>
          <div class="flex items-center gap-3 rounded-lg border bg-card px-4 py-3">
            <Switch checked={opts.relock} onCheckedChange={(v: boolean) => (opts.relock = v)} id="opt-relock" />
            <div class="flex-1">
              <div class="text-sm font-medium">부트로더 리락</div>
              <div class="text-[11px] text-muted-foreground">기기가 다시 초기화됩니다</div>
            </div>
            {#if opts.relock}
              <Badge variant="destructive" class="text-[10px]">초기화</Badge>
            {/if}
          </div>
          {#if !opts.unroot && opts.relock}
            <div class="rounded-lg bg-warning-container/60 px-4 py-2 text-xs text-warning">
              리락하려면 언루팅이 필요합니다 — 언루팅이 자동으로 포함됩니다
            </div>
          {/if}
        {/if}
      {/if}

      <!-- 백업 경로 (백업 항목이 하나라도 선택된 경우) -->
      {#if backupGroups.some((g) => g.checked)}
        <div class="h-px bg-border my-2"></div>
        <div class="rounded-lg border bg-card px-4 py-3 space-y-2">
          <div class="text-sm font-medium">백업 저장 위치</div>
          <div class="flex items-center gap-2 rounded-lg border bg-background px-3">
            <FolderOpen size={15} class="shrink-0 text-primary" />
            <input
              class="w-full bg-transparent py-2 font-mono text-[12.5px] outline-none placeholder:text-muted-foreground"
              bind:value={wizard.backupPath}
              placeholder="D:\backup\xperia"
            />
          </div>
        </div>
      {/if}
    </div>

    <!-- 실행 버튼 -->
    <div class="shrink-0 pt-2">
      <Button class="w-full" size="lg" onclick={confirm}>
        <Play size={16} class="mr-2" />
        실행 시작 ({planSteps.length}단계)
      </Button>
    </div>
  </div>

  <!-- 우: 실행 순서 미리보기 (좁게) -->
  <div class="flex-[2] min-w-0 rounded-xl border bg-muted/30 flex flex-col overflow-hidden">
    <div class="shrink-0 px-4 py-3 border-b">
      <div class="text-sm font-semibold">실행 순서</div>
      <div class="text-[11px] text-muted-foreground">{planSteps.length}단계</div>
    </div>
    <div class="flex-1 overflow-y-auto p-2">
      {#each planSteps as step, i (i)}
        <div class="flex items-center gap-2.5 rounded-md px-2 py-1.5 text-[13px] {step.warn ? 'text-destructive' : ''}">
          <span class="w-5 h-5 shrink-0 rounded-full border flex items-center justify-center text-[10px]
            {step.warn ? 'border-destructive/40 text-destructive' : 'border-border text-muted-foreground'}">
            {i + 1}
          </span>
          <span class="truncate">{step.title}</span>
          {#if step.warn}
            <TriangleAlert size={12} class="shrink-0 text-destructive/70" />
          {/if}
        </div>
      {/each}
    </div>
  </div>
</div>
