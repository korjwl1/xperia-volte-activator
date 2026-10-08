<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import SelectionDeviceStatus from "$lib/components/SelectionDeviceStatus.svelte";
  import { ArrowLeft, HardDrive, FolderOpen, LockOpen, Lock, ShieldCheck, ShieldOff, Signal, Phone, TriangleAlert, RefreshCw, Package } from "@lucide/svelte/icons";
  import { wizard } from "$lib/stores/wizard.svelte";
  import { MANUAL_TASKS, manualTaskProblem } from "$lib/domain/workflow";
  const icons = { backup: HardDrive, restore: FolderOpen, unlock: LockOpen, relock: Lock, root: ShieldCheck, unroot: ShieldOff, volte: Signal, verify: Phone, "root-manager": RefreshCw, "root-modules": Package };
  let checking = $state(false);
  let deviceStatus: SelectionDeviceStatus;
</script>

<div class="flex-1 min-h-0 flex flex-col">
  <div class="flex-1 min-h-0 overflow-y-auto p-6 space-y-4">
    <div><h1 class="text-xl font-semibold">수동 진행</h1><p class="mt-1 text-xs text-muted-foreground">필요한 기능 하나를 선택하세요. 선택한 기능의 준비·검증을 거쳐 진행합니다.</p></div>
    <SelectionDeviceStatus bind:checking bind:this={deviceStatus} />
    <div class="grid grid-cols-3 gap-3" aria-label="수동 작업">
      {#each MANUAL_TASKS as task}
        {@const reason = manualTaskProblem(task.id, wizard.device)}
        {@const Icon = icons[task.id]}
        <button type="button" class="min-w-0 rounded-xl bg-card elev-1 p-4 text-left hover:bg-accent disabled:opacity-50 disabled:cursor-not-allowed" disabled={checking || !!reason} onclick={async () => { if (await deviceStatus.refresh()) await wizard.chooseManualTask(task.id); }}>
          <span class="flex items-center gap-2 font-semibold text-sm"><Icon size={19} class={task.id === "unlock" || task.id === "relock" ? "text-destructive" : "text-primary"} />{task.title}</span>
          <span class="block mt-2 text-xs text-muted-foreground">{task.detail}</span>
          {#if reason}<span class="flex items-start gap-1 mt-2 text-[11px] text-warning"><TriangleAlert size={12} class="shrink-0" />{reason}</span>{/if}
        </button>
      {/each}
    </div>
  </div>
  <footer class="h-14 shrink-0 border-t px-6 flex items-center"><Button variant="ghost" size="sm" onclick={() => wizard.view = "mode-select"}><ArrowLeft size={14} />진행 방법 선택</Button></footer>
</div>
