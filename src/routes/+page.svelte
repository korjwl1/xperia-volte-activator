<script lang="ts">
  import { onMount } from "svelte";
  import { Button } from "$lib/components/ui/button";
  import { Badge } from "$lib/components/ui/badge";
  import {
    Smartphone, ListChecks, FolderDown, HardDrive, CirclePlay,
  } from "@lucide/svelte/icons";
  import { wizard, type WizardView } from "$lib/stores/wizard.svelte";
  import DeviceStatusView from "$lib/views/DeviceStatusView.svelte";
  import PlanReviewView from "$lib/views/PlanReviewView.svelte";
  import BackupSelectView from "$lib/views/BackupSelectView.svelte";
  import BackupTargetView from "$lib/views/BackupTargetView.svelte";
  import RunProgressView from "$lib/views/RunProgressView.svelte";

  const steps: { id: WizardView; label: string; icon: typeof Smartphone }[] = [
    { id: "device", label: "디바이스", icon: Smartphone },
    { id: "plan", label: "계획 확인", icon: ListChecks },
    { id: "backup-select", label: "백업 선택", icon: FolderDown },
    { id: "backup-target", label: "저장 위치", icon: HardDrive },
    { id: "run", label: "실행", icon: CirclePlay },
  ];

  const stepIndex = $derived(steps.findIndex((s) => s.id === wizard.view));
  const showBackupSteps = $derived(
    wizard.view === "backup-select" || wizard.view === "backup-target" || wizard.hasWipeRoute,
  );

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
    : wizard.backupPath.trim().length > 0,
  );

  function onNext() {
    switch (wizard.view) {
      case "device": wizard.goPlan(); break;
      case "plan": wizard.confirmPlan(); break;
      case "backup-select": wizard.view = "backup-target"; break;
      case "backup-target": wizard.startRun(); break;
    }
  }

  function gotoStep(id: WizardView) {
    // 사이드바는 현재까지 도달한 단계만 이동 허용 (뒤로가기 + 현재)
    wizard.view = id;
  }

  onMount(() => {
    wizard.refreshDevice();
  });
</script>

<div class="h-screen flex flex-col bg-background text-foreground overflow-hidden">
  <!-- 타이틀 바 -->
  <header class="h-10 shrink-0 border-b flex items-center px-3 gap-2 select-none">
    <span class="text-sm font-semibold tracking-tight">xperia-volte-activator</span>
    <Badge variant="outline" class="text-[10px] px-1.5 py-0">mock 모드</Badge>
    <span class="ml-auto text-[11px] text-muted-foreground">v0.1.0</span>
  </header>

  <div class="flex-1 flex min-h-0">
    <!-- 사이드바 -->
    <aside class="w-60 shrink-0 border-r bg-muted/40 flex flex-col">
      <nav class="p-2 space-y-0.5">
        {#each steps as s, i (s.id)}
          {#if (s.id !== "backup-select" && s.id !== "backup-target") || showBackupSteps}
            {@const reachable = i <= stepIndex}
            <button
              class="w-full flex items-center gap-2.5 rounded-md px-2 py-1.5 text-left text-[13px] transition-colors
                {i === stepIndex ? "bg-primary text-primary-foreground font-medium" : reachable ? "hover:bg-accent" : "opacity-40 cursor-default"}"
              disabled={!reachable}
              onclick={() => reachable && gotoStep(s.id)}
            >
              <span class="w-[18px] h-[18px] shrink-0 rounded-full border flex items-center justify-center text-[10px]
                {i < stepIndex ? "bg-primary border-primary text-primary-foreground" : i === stepIndex ? "border-current" : "text-muted-foreground"}">
                {i < stepIndex ? "✓" : i + 1}
              </span>
              <s.icon size={14} class="shrink-0 opacity-80" />
              {s.label}
            </button>
          {/if}
        {/each}
      </nav>

      <div class="mt-auto border-t p-3 space-y-1 text-[11px] leading-snug">
        {#if wizard.device}
          <div class="font-medium text-[12px] text-foreground">{wizard.device.productName}</div>
          <div class="font-mono text-muted-foreground">{wizard.device.model} · {wizard.device.serialMasked}</div>
          <div class="text-muted-foreground">{wizard.device.firmware} · Android {wizard.device.android}</div>
          <div class="pt-1 flex flex-wrap gap-1">
            <Badge variant="outline" class="text-[10px] px-1.5 py-0">
              {wizard.device.bootloader === "locked" ? "🔒 부트로더 잠김" : wizard.device.bootloader === "unlocked" ? "🔓 언락" : "? 언락상태"}
            </Badge>
            <Badge variant="outline" class="text-[10px] px-1.5 py-0">
              {wizard.device.sims.some((s) => s.volteEnabled) ? "VoLTE 사용 중" : "VoLTE 미사용"}
            </Badge>
          </div>
        {:else}
          <div class="text-muted-foreground">디바이스 대기 중…</div>
        {/if}
      </div>
    </aside>

    <!-- 콘텐츠 -->
    <main class="flex-1 min-w-0 overflow-y-auto">
      {#if wizard.view === "run"}
        <div class="h-full p-5">
          <RunProgressView />
        </div>
      {:else}
        <div class="p-5 max-w-3xl">
          {#if wizard.view === "device"}
            <DeviceStatusView />
          {:else if wizard.view === "plan"}
            <PlanReviewView />
          {:else if wizard.view === "backup-select"}
            <BackupSelectView />
          {:else}
            <BackupTargetView />
          {/if}
        </div>
      {/if}
    </main>
  </div>

  <!-- 액션 바 -->
  {#if wizard.view !== "run"}
    <footer class="h-12 shrink-0 border-t bg-muted/40 flex items-center justify-between px-4">
      <Button variant="ghost" size="sm" disabled={wizard.view === "device"} onclick={() => {
        if (wizard.view === "plan") wizard.view = "device";
        else if (wizard.view === "backup-select") wizard.view = "plan";
        else if (wizard.view === "backup-target") wizard.view = "backup-select";
      }}>← 이전</Button>
      <Button size="sm" disabled={!canNext} onclick={onNext}>{nextLabel}</Button>
    </footer>
  {/if}
</div>
