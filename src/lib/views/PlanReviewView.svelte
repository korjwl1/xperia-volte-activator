<script lang="ts">
  import { Switch } from "$lib/components/ui/switch";
  import { Button } from "$lib/components/ui/button";
  import { Badge } from "$lib/components/ui/badge";
  import { TriangleAlert, FolderOpen, Settings2, LayoutGrid, HardDrive } from "@lucide/svelte/icons";
  import { wizard } from "$lib/stores/wizard.svelte";
  import { mockBackupGroups } from "$lib/mock/apps";
  import type { PlanStep } from "$lib/types";

  let activeTab = $state<"backup" | "rooting">("backup");

  // 초기화 단계가 있으면 백업 전부 ON, 없으면 OFF가 기본값이어도 사용자가 고르게
  const hasWipe = wizard.device?.bootloader === "locked";
  const defaultBackupOn = hasWipe;

  let backupGroups = $state(
    mockBackupGroups.map((g) => ({ id: g.id, label: g.label, desc: g.desc, checked: defaultBackupOn })),
  );

  let opts = $state({
    restore: defaultBackupOn,
    backup2: defaultBackupOn,
    unroot: true,
    relock: true,
  });

  // 백업 카테고리 정의
  const backupCategories = [
    {
      id: "settings",
      label: "설정",
      icon: Settings2,
      groupIds: ["settings"],
    },
    {
      id: "apps",
      label: "앱",
      icon: LayoutGrid,
      groupIds: ["apps"],
    },
    {
      id: "files",
      label: "파일",
      icon: HardDrive,
      groupIds: ["storage", "hidden", "sms"],
    },
  ];

  // 카테고리별 그룹 가져오기
  function groupsFor(categoryId: string) {
    const cat = backupCategories.find((c) => c.id === categoryId);
    if (!cat) return [];
    return backupGroups.filter((g) => cat.groupIds.includes(g.id));
  }

  // 카테고리 전체 온오프
  function setCategoryAll(categoryId: string, on: boolean) {
    for (const g of groupsFor(categoryId)) g.checked = on;
  }

  // 카테고리 전체 선택 여부
  function isCategoryAll(categoryId: string) {
    return groupsFor(categoryId).every((g) => g.checked);
  }

  // 우측 실행 순서 (실시간)
  const planSteps = $derived.by(() => {
    const steps: { title: string; warn?: boolean }[] = [];
    const d = wizard.device;
    if (!d) return steps;
    const anyBackup = backupGroups.some((g) => g.checked);
    const needsUnlock = d.bootloader === "locked";
    const effUnroot = opts.unroot || opts.relock;

    if (anyBackup) steps.push({ title: "백업" });
    if (needsUnlock) {
      steps.push({ title: "부트로더 언락", warn: true });
      steps.push({ title: "기본 설정" });
    }
    if (d.rooted !== true) steps.push({ title: "루팅" });
    steps.push({ title: "연결 안정성 검사" });
    steps.push({ title: "VoLTE 적용" });
    steps.push({ title: "적용 확인" });
    if (needsUnlock) {
      if (effUnroot) steps.push({ title: "언루팅" });
      if (opts.backup2 && anyBackup) steps.push({ title: "2차 백업" });
      if (opts.relock) steps.push({ title: "부트로더 리락", warn: true });
      steps.push({ title: "최종 확인" });
      if (opts.restore && anyBackup) steps.push({ title: "복구" });
    }
    return steps;
  });

  function confirm() {
    const effUnroot = opts.unroot || opts.relock;
    wizard.groups = mockBackupGroups.map((g) => ({
      ...g,
      items: g.items.map((i) => ({ ...i, checked: backupGroups.find((bg) => bg.id === g.id)?.checked ?? false })),
    }));
    const steps: PlanStep[] = [];
    const d = wizard.device;
    if (!d) return;
    const push = (id: string, title: string, risk: PlanStep["risk"] = "safe", wipe = false, manual?: string, estSec = 120) =>
      steps.push({ id, kind: id as PlanStep["kind"], title, desc: "", optional: false, enabled: true, risk, wipe, estSec, manual: manual as PlanStep["manual"] });
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

<div class="flex-1 min-h-0 flex flex-col">
  <div class="flex-1 min-h-0 flex gap-4 p-4 lg:p-6">
    <!-- 좌: 옵션 -->
    <div class="flex-[3] min-w-0 flex flex-col gap-4">
      <!-- 탭 -->
      <div class="shrink-0 flex gap-1 rounded-lg bg-muted p-1">
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

      <!-- 탭 콘텐츠 (스크롤) -->
      <div class="flex-1 min-h-0 overflow-y-auto">
        {#if activeTab === "backup"}
          <!-- 백업 경로 — 백업 탭 내 상단 sticky -->
          {#if backupGroups.some((g) => g.checked)}
            <div class="sticky top-0 z-10 -mx-1 mb-3 bg-background/95 backdrop-blur border-b pb-3 px-1">
              <div class="flex items-center gap-2.5">
                <FolderOpen size={16} class="text-primary shrink-0" />
                <div class="flex-1 min-w-0 rounded-lg border bg-background px-3 py-1.5 font-mono text-[12px] truncate">
                  {wizard.backupPath || "백업 위치를 지정해 주세요"}
                </div>
                <Button size="sm" variant="outline" class="h-7 text-xs shrink-0" onclick={() => (wizard.backupPath = "D:\\backup\\xperia-1v")}>
                  폴더 지정
                </Button>
              </div>
            </div>
          {/if}

          {#each backupCategories as cat (cat.id)}
            <div class="mb-4">
              <div class="flex items-center gap-2 mb-2 px-1">
                <cat.icon size={14} class="text-muted-foreground" />
                <span class="text-xs font-semibold text-muted-foreground uppercase tracking-wide">{cat.label}</span>
                <button
                  class="ml-auto text-[11px] text-muted-foreground hover:text-foreground"
                  onclick={() => setCategoryAll(cat.id, !isCategoryAll(cat.id))}
                >
                  {isCategoryAll(cat.id) ? "전체 해제" : "전체 선택"}
                </button>
              </div>
              <div class="space-y-1.5">
                {#each groupsFor(cat.id) as group (group.id)}
                  <div class="flex items-center gap-3 rounded-lg border bg-card px-4 py-2.5">
                    <Switch checked={group.checked} onCheckedChange={(v: boolean) => (group.checked = v)} id="bg-{group.id}" />
                    <div class="min-w-0 flex-1">
                      <div class="text-[13px] font-medium">{group.label}</div>
                      <div class="text-[11px] text-muted-foreground truncate">{group.desc}</div>
                    </div>
                  </div>
                {/each}
              </div>
            </div>
          {/each}

          <div class="h-px bg-border mb-4"></div>

          <div class="flex items-center gap-3 rounded-lg border bg-card px-4 py-2.5">
            <Switch checked={opts.restore} onCheckedChange={(v: boolean) => (opts.restore = v)} id="opt-restore" />
            <div class="flex-1">
              <div class="text-[13px] font-medium">복구 자동 실행</div>
              <div class="text-[11px] text-muted-foreground">모든 작업 완료 후 백업한 데이터를 자동으로 복원합니다</div>
            </div>
          </div>
          <div class="mt-1.5 flex items-center gap-3 rounded-lg border bg-card px-4 py-2.5">
            <Switch checked={opts.backup2} onCheckedChange={(v: boolean) => (opts.backup2 = v)} id="opt-b2" />
            <div class="flex-1">
              <div class="text-[13px] font-medium">2차 백업</div>
              <div class="text-[11px] text-muted-foreground">리락 직전에 언락 이후 생성된 데이터를 백업합니다</div>
            </div>
          </div>

        {:else}
          <!-- 루팅 탭 -->
          {#if wizard.device?.bootloader === "locked"}
            <div class="flex items-center gap-3 rounded-lg border bg-card px-4 py-2.5 mb-3">
              <TriangleAlert size={16} class="text-warning shrink-0" />
              <div class="flex-1">
                <div class="text-[13px] font-medium">부트로더 언락 · 루팅 · VoLTE 적용</div>
                <div class="text-[11px] text-muted-foreground">부트로더가 잠겨 있어 자동으로 포함됩니다</div>
              </div>
              <Badge variant="secondary" class="text-[10px]">필수</Badge>
            </div>
            <div class="h-px bg-border mb-3"></div>
            <div class="flex items-center gap-3 rounded-lg border bg-card px-4 py-2.5 mb-1.5">
              <Switch checked={opts.unroot} onCheckedChange={(v: boolean) => (opts.unroot = v)} id="opt-unroot" />
              <div class="flex-1">
                <div class="text-[13px] font-medium">언루팅</div>
                <div class="text-[11px] text-muted-foreground">시스템을 원래대로 되돌립니다 — 리락하려면 필요합니다</div>
              </div>
            </div>
            <div class="flex items-center gap-3 rounded-lg border bg-card px-4 py-2.5">
              <Switch checked={opts.relock} onCheckedChange={(v: boolean) => (opts.relock = v)} id="opt-relock" />
              <div class="flex-1">
                <div class="text-[13px] font-medium">부트로더 리락</div>
                <div class="text-[11px] text-muted-foreground">기기가 다시 초기화됩니다</div>
              </div>
              {#if opts.relock}<Badge variant="destructive" class="text-[10px]">초기화</Badge>{/if}
            </div>
            {#if !opts.unroot && opts.relock}
              <div class="mt-2 rounded-lg bg-warning-container/60 px-4 py-2 text-xs text-warning">
                리락하려면 언루팅이 필요합니다 — 언루팅이 자동으로 포함됩니다
              </div>
            {/if}
          {:else if wizard.device?.rooted !== true}
            <div class="flex items-center gap-3 rounded-lg border bg-card px-4 py-2.5">
              <div class="flex-1">
                <div class="text-[13px] font-medium">루팅 · VoLTE 적용</div>
                <div class="text-[11px] text-muted-foreground">VoLTE 적용을 위해 필요합니다</div>
              </div>
              <Badge variant="secondary" class="text-[10px]">필수</Badge>
            </div>
          {:else}
            <div class="flex items-center gap-3 rounded-lg border bg-card px-4 py-2.5">
              <div class="flex-1">
                <div class="text-[13px] font-medium">VoLTE 적용</div>
                <div class="text-[11px] text-muted-foreground">이미 루팅되어 있어 언락/루팅 단계를 건너뜁니다</div>
              </div>
              <Badge variant="secondary" class="text-[10px]">필수</Badge>
            </div>
          {/if}
        {/if}
      </div>
    </div>

    <!-- 우: 실행 순서 -->
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
            {#if step.warn}<TriangleAlert size={12} class="shrink-0 text-destructive/70" />{/if}
          </div>
        {/each}
      </div>
    </div>
  </div>

  <!-- 하단: 기존 디자인 유지 + 실행 버튼 -->
  <footer class="h-14 shrink-0 border-t bg-muted/40 flex items-center justify-between px-6">
    <Button variant="ghost" size="sm" onclick={() => (wizard.view = "step1")}>← 이전</Button>
    <Button size="sm" onclick={confirm}>실행</Button>
  </footer>
</div>
