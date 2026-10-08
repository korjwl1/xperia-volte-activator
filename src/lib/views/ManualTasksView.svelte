<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import SelectionDeviceStatus from "$lib/components/SelectionDeviceStatus.svelte";
  import { ArrowLeft, HardDrive, FolderOpen, LockOpen, Lock, ShieldCheck, ShieldOff, Signal, Phone, TriangleAlert, RefreshCw, Package, Undo2 } from "@lucide/svelte/icons";
  import { onMount } from "svelte";
  import { wizard } from "$lib/stores/wizard.svelte";
  import { MANUAL_TASKS, MANUAL_TASK_GROUPS, manualTaskProblem } from "$lib/domain/workflow";
  import { REAL_STEPS } from "$lib/data/runMode";
  import type { ManualTask } from "$lib/types";
  // 실전 기능이 꺼진 작업은 끝까지 진행한 뒤 막히지 않게 카드에서 먼저 막는다
  const live: Record<ManualTask, boolean> = {
    backup: REAL_STEPS.backup, restore: REAL_STEPS.restore, unlock: REAL_STEPS.fastboot,
    relock: REAL_STEPS.fastboot && REAL_STEPS.relock, root: REAL_STEPS.root && REAL_STEPS.fastboot,
    unroot: REAL_STEPS.root && REAL_STEPS.fastboot, volte: REAL_STEPS.efs, "volte-rollback": REAL_STEPS.efs && REAL_STEPS.volteRollback, verify: REAL_STEPS.efs,
    "root-manager": REAL_STEPS.root && REAL_STEPS.fastboot && REAL_STEPS.rootTools, "root-modules": REAL_STEPS.rootTools,
  };
  const icons = { backup: HardDrive, restore: FolderOpen, unlock: LockOpen, relock: Lock, root: ShieldCheck, unroot: ShieldOff, volte: Signal, "volte-rollback": Undo2, verify: Phone, "root-manager": RefreshCw, "root-modules": Package };
  const groups = MANUAL_TASK_GROUPS.map(g => ({ ...g, items: g.tasks.map(id => MANUAL_TASKS.find(t => t.id === id)!).filter(Boolean) }));
  let checking = $state(false);
  // 되돌리기 카드는 이 폰의 패치 기록이 있어야 쓸 수 있다
  onMount(() => { void wizard.loadDeviceRecord(); });
  let deviceStatus: SelectionDeviceStatus;
</script>

<div class="flex-1 min-h-0 flex flex-col">
  <div class="flex-1 min-h-0 overflow-y-auto p-6 space-y-6">
    <div><h1 class="text-xl font-semibold">수동 진행</h1><p class="mt-1 text-xs text-muted-foreground">필요한 기능 하나를 선택하세요. 선택한 기능의 준비·검증을 거쳐 진행합니다.</p></div>
    <SelectionDeviceStatus bind:checking bind:this={deviceStatus} />
    {#each groups as group (group.title)}
      <section class="space-y-3" aria-label={group.title}>
        <div class="flex items-baseline gap-3 px-1">
          <h2 class="text-sm font-semibold">{group.title}</h2>
          <p class="text-xs text-muted-foreground">{group.detail}</p>
        </div>
        <div class="grid grid-cols-4 gap-3">
          {#each group.items as task (task.id)}
            {@const reason = !live[task.id] ? "실기기 검증 전이라 아직 사용할 수 없습니다"
              : task.id === "volte-rollback" && wizard.rollbackTargets().length === 0 ? "되돌릴 VoLTE 패치 기록이 없습니다"
              : manualTaskProblem(task.id, wizard.device)}
            {@const Icon = icons[task.id]}
            {@const danger = task.id === "unlock" || task.id === "relock"}
            <button type="button" class="group min-w-0 rounded-2xl bg-card elev-1 p-4 text-left flex items-start gap-3 transition duration-150 hover:-translate-y-0.5 hover:elev-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary disabled:opacity-50 disabled:cursor-not-allowed disabled:hover:translate-y-0 disabled:hover:elev-1" disabled={checking || !!reason} onclick={async () => { if (await deviceStatus.ensure()) await wizard.chooseManualTask(task.id); }}>
              <span class="inline-flex size-10 shrink-0 items-center justify-center rounded-xl {danger ? 'bg-danger-container text-destructive' : 'bg-primary/10 text-primary'}"><Icon size={20} /></span>
              <span class="min-w-0">
                <span class="block text-sm font-semibold">{task.title}</span>
                <span class="mt-1 block break-keep text-xs leading-relaxed text-muted-foreground">{task.detail}</span>
                {#if reason}<span class="mt-2 flex items-start gap-1 break-keep text-[11px] text-warning"><TriangleAlert size={12} class="mt-px shrink-0" />{reason}</span>{/if}
              </span>
            </button>
          {/each}
        </div>
      </section>
    {/each}
  </div>
  <footer class="h-14 shrink-0 border-t px-6 flex items-center"><Button variant="ghost" size="sm" onclick={() => wizard.view = "mode-select"}><ArrowLeft size={14} />진행 방법 선택</Button></footer>
</div>
