<script lang="ts">
  import { onMount } from "svelte";
  import { Button } from "$lib/components/ui/button";
  import { wizard, type WizardView } from "$lib/stores/wizard.svelte";
  import DeviceStatusView from "$lib/views/DeviceStatusView.svelte";
  import VolteConfigView from "$lib/views/VolteConfigView.svelte";
  import PlanReviewView from "$lib/views/PlanReviewView.svelte";
  import BackupSelectView from "$lib/views/BackupSelectView.svelte";
  import BackupTargetView from "$lib/views/BackupTargetView.svelte";
  import RunProgressView from "$lib/views/RunProgressView.svelte";

  const viewOrder: WizardView[] = ["device", "volte-config", "plan", "backup-select", "backup-target", "run"];
  const viewIndex = $derived(viewOrder.indexOf(wizard.view));

  const nextLabel = $derived(
    wizard.view === "device" ? "다음"
    : wizard.view === "volte-config" ? "작업 내역 확인"
    : wizard.view === "plan" ? (wizard.hasWipeRoute ? "백업 항목 선택" : "실행")
    : wizard.view === "backup-select" ? "다음"
    : "백업 시작"
  );

  const canNext = $derived(
    wizard.view === "device" ? wizard.device !== null
    : wizard.view === "volte-config" ? true
    : wizard.view === "plan" ? true
    : wizard.view === "backup-select" ? wizard.anyChecked
    : wizard.backupPath.trim().length > 0
  );

  function onNext() {
    switch (wizard.view) {
      case "device": wizard.view = "volte-config"; break;
      case "volte-config": wizard.goPlan(); break;
      case "plan": wizard.confirmPlan(); break;
      case "backup-select": wizard.view = "backup-target"; break;
      case "backup-target": wizard.startRun(); break;
    }
  }

  function onPrev() {
    if (viewIndex <= 0) return;
    const prev = viewOrder[viewIndex - 1];
    wizard.view = prev;
  }

  onMount(() => {
    wizard.refreshDevice();
  });
</script>

<div class="h-screen flex flex-col bg-background text-foreground overflow-hidden">
  <!-- 헤더 -->
  <header class="h-10 shrink-0 border-b flex items-center px-4 gap-2 select-none">
    <span class="text-sm font-semibold tracking-tight">Xperia VoLTE Activator</span>
    <span class="ml-auto text-[11px] text-muted-foreground">v0.1.0</span>
  </header>

  <!-- 콘텐츠 — 전체 화면 -->
  <main class="flex-1 min-h-0 overflow-y-auto flex items-start justify-center">
    <div class="w-full max-w-2xl {wizard.view === 'run' ? 'h-full p-5' : 'p-6'}">
      {#if wizard.view === "device"}
        <DeviceStatusView />
      {:else if wizard.view === "volte-config"}
        <VolteConfigView />
      {:else if wizard.view === "plan"}
        <PlanReviewView />
      {:else if wizard.view === "backup-select"}
        <BackupSelectView />
      {:else if wizard.view === "backup-target"}
        <BackupTargetView />
      {:else}
        <RunProgressView />
      {/if}
    </div>
  </main>

  <!-- 하단 액션 바 -->
  {#if wizard.view !== "run"}
    <footer class="h-14 shrink-0 border-t bg-muted/40 flex items-center justify-between px-6">
      <Button variant="ghost" size="sm" disabled={viewIndex <= 0} onclick={onPrev}>← 이전</Button>
      <div class="flex items-center gap-3">
        {#if viewIndex > 0 && viewIndex < viewOrder.length - 1}
          <span class="text-[11px] text-muted-foreground">{viewIndex + 1} / {viewOrder.length - 1}</span>
        {/if}
        <Button size="sm" disabled={!canNext} onclick={onNext}>{nextLabel}</Button>
      </div>
    </footer>
  {/if}
</div>
