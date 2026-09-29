<script lang="ts">
  import { onMount } from "svelte";
  import { Button } from "$lib/components/ui/button";
  import { wizard } from "$lib/stores/wizard.svelte";
  import DeviceStatusView from "$lib/views/DeviceStatusView.svelte";
  import PlanReviewView from "$lib/views/PlanReviewView.svelte";
  import BackupSelectView from "$lib/views/BackupSelectView.svelte";
  import BackupTargetView from "$lib/views/BackupTargetView.svelte";
  import RunProgressView from "$lib/views/RunProgressView.svelte";

  const steps = [
    { id: "device", label: "디바이스" },
    { id: "plan", label: "계획" },
    { id: "backup-select", label: "백업 선택" },
    { id: "backup-target", label: "저장 위치" },
    { id: "run", label: "실행" },
  ];
  const stepIndex = $derived(steps.findIndex((s) => s.id === wizard.view));
  const showBackupSteps = $derived(wizard.view === "backup-select" || wizard.view === "backup-target" || wizard.view === "run");

  const nextLabel = $derived(
    wizard.view === "device" ? "작업 시작"
    : wizard.view === "plan" ? "이 계획으로 진행"
    : wizard.view === "backup-select" ? "다음"
    : "백업 시작",
  );
  const canNext = $derived(
    wizard.view === "device" ? wizard.device !== null
    : wizard.view === "plan" ? true
    : wizard.view === "backup-select" ? wizard.anyChecked
    : wizard.backupPath.length > 0 && wizard.backupPath.trim().length > 0,
  );

  function onNext() {
    switch (wizard.view) {
      case "device": wizard.goPlan(); break;
      case "plan": wizard.confirmPlan(); break;
      case "backup-select": wizard.view = "backup-target"; break;
      case "backup-target": wizard.startRun(); break;
    }
  }

  onMount(() => {
    wizard.refreshDevice();
  });
</script>

<div class="min-h-screen flex flex-col">
  <header class="border-b px-6 py-3 flex items-center justify-between">
    <div class="flex items-center gap-3">
      <span class="font-semibold">xperia-volte-activator</span>
      <span class="text-xs text-muted-foreground">mock 모드 · 프론트 개발 단계</span>
    </div>
    <nav class="flex items-center gap-1 text-xs">
      {#each steps as s, i}
        {#if s.id !== "backup-select" && s.id !== "backup-target" || showBackupSteps}
          <span class="px-2 py-1 rounded-md {i === stepIndex ? 'bg-primary text-primary-foreground' : i < stepIndex ? 'text-muted-foreground' : 'text-muted-foreground/50'}">
            {i + 1}. {s.label}
          </span>
        {/if}
      {/each}
    </nav>
  </header>

  <main class="flex-1 px-6 py-5 max-w-4xl w-full mx-auto">
    {#if wizard.view === "device"}
      <DeviceStatusView />
    {:else if wizard.view === "plan"}
      <PlanReviewView />
    {:else if wizard.view === "backup-select"}
      <BackupSelectView />
    {:else if wizard.view === "backup-target"}
      <BackupTargetView />
    {:else}
      <RunProgressView />
    {/if}
  </main>

  {#if wizard.view !== "run"}
    <footer class="border-t px-6 py-3 flex items-center justify-between">
      <Button variant="ghost" disabled={wizard.view === "device"} onclick={() => {
        if (wizard.view === "plan") wizard.view = "device";
        else if (wizard.view === "backup-select") wizard.view = "plan";
        else if (wizard.view === "backup-target") wizard.view = "backup-select";
      }}>← 이전</Button>
      <div class="flex items-center gap-2">
        {#if wizard.view === "backup-select" && wizard.backupSkippable}
          <p class="text-xs text-muted-foreground">하단 "백업 건너뛰기…"는 목록 아래에서 확인할 수 있습니다</p>
        {/if}
        <Button disabled={!canNext} onclick={onNext}>{nextLabel}</Button>
      </div>
    </footer>
  {/if}
</div>
