<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import { wizard } from "$lib/stores/wizard.svelte";
  import DeviceStatusView from "$lib/views/DeviceStatusView.svelte";
  import VolteConfigView from "$lib/views/VolteConfigView.svelte";
  import WarningView from "$lib/views/WarningView.svelte";
  import PlanReviewView from "$lib/views/PlanReviewView.svelte";
  import RunProgressView from "$lib/views/RunProgressView.svelte";
  import FinishView from "$lib/views/FinishView.svelte";

  const canNext = $derived(
    wizard.view === "warning" ? wizard.omdAck && wizard.riskAck
    : wizard.view === "step1" ? wizard.hasAnyTask
    : true
  );

  function onNext() {
    if (!canNext) return;
    switch (wizard.view) {
      case "warning":
        wizard.view = "step1";
        break;
      case "step1":
        wizard.view = "step2"; // step2의 [실행]은 뷰 내부 버튼
        break;
    }
  }

  function onPrev() {
    if (wizard.view === "step2") wizard.view = "step1";
    else if (wizard.view === "step1") wizard.view = "warning";
    else if (wizard.view === "warning") wizard.view = "device";
  }
</script>

<div class="h-screen flex flex-col bg-background text-foreground overflow-hidden">
  {#if wizard.view === "device"}
    <!-- 1페이지: 풀스크린 (헤더/사이드바/푸터 없음) -->
    <main class="flex-1 min-h-0 flex flex-col">
      <DeviceStatusView />
    </main>

  {:else}
    <!-- 2~4페이지 -->
    <!-- 헤더 -->
    <header class="h-10 shrink-0 border-b flex items-center px-4 gap-2 select-none">
      <span class="text-sm font-semibold tracking-tight">Xperia VoLTE Activator</span>
      <span class="ml-auto text-[11px] text-muted-foreground">v0.1.0</span>
    </header>
    <!-- 사이드바 + 콘텐츠 (경고 페이지는 1~4단계 시작 전이라 사이드바 없음) -->
    <div class="flex-1 min-h-0 flex">
      {#if wizard.view !== "warning"}
        <Sidebar />
      {/if}

      <div class="flex-1 min-w-0 flex flex-col">
        {#if wizard.view === "warning"}
          <WarningView />
        {:else if wizard.view === "step1"}
          <VolteConfigView />
        {:else if wizard.view === "step2"}
          <PlanReviewView />
        {:else if wizard.view === "step3"}
          <RunProgressView />
        {:else}
          <FinishView />
        {/if}
      </div>
    </div>

    <!-- 하단 액션 바 (warning·step1만, step2는 뷰 내부 버튼) -->
    {#if wizard.view === "warning" || wizard.view === "step1"}
      <footer class="h-14 shrink-0 border-t bg-muted/40 flex items-center justify-between px-6">
        <Button variant="ghost" size="sm" onclick={onPrev}>← 이전</Button>
        <Button size="sm" onclick={onNext} disabled={!canNext}>다음</Button>
      </footer>
    {/if}
  {/if}
</div>
