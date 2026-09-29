<script lang="ts">
  import { onMount } from "svelte";
  import { Button } from "$lib/components/ui/button";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import { wizard } from "$lib/stores/wizard.svelte";
  import DeviceStatusView from "$lib/views/DeviceStatusView.svelte";
  import VolteConfigView from "$lib/views/VolteConfigView.svelte";
  import PlanReviewView from "$lib/views/PlanReviewView.svelte";
  import BackupSelectView from "$lib/views/BackupSelectView.svelte";
  import BackupTargetView from "$lib/views/BackupTargetView.svelte";
  import RunProgressView from "$lib/views/RunProgressView.svelte";
  import FinishView from "$lib/views/FinishView.svelte";

  // 백업 단계 표시 여부 (step2 다음에 백업이 필요한 경우 별도 서브 뷰)
  let backupSubView = $state<"none" | "select" | "target">("none");

  const canNext = $derived(
    wizard.view === "step1" ? true
    : wizard.view === "step2" ? true
    : true
  );

  function onNext() {
    switch (wizard.view) {
      case "step1":
        wizard.applyVolteConfig();
        wizard.view = "step2";
        break;
      case "step2":
        wizard.confirmStep2();
        break;
    }
  }

  function onPrev() {
    if (wizard.view === "step2") wizard.view = "step1";
  }

  onMount(() => {
    wizard.refreshDevice();
  });
</script>

<div class="h-screen flex flex-col bg-background text-foreground overflow-hidden">
  {#if wizard.view === "device"}
    <!-- 1페이지: 풀스크린 (헤더/사이드바/푸터 없음) -->
    <main class="flex-1 min-h-0 flex flex-col">
      <DeviceStatusView />
    </main>

  {:else}
    <!-- 2~4페이지: 사이드바 + 콘텐츠 -->
    <div class="flex-1 min-h-0 flex">
      <Sidebar />

      <div class="flex-1 min-w-0 flex flex-col">
        {#if wizard.view === "step1"}
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

    <!-- 하단 액션 바 (step3/4 제외) -->
    {#if wizard.view === "step1" || wizard.view === "step2"}
      <footer class="h-14 shrink-0 border-t bg-muted/40 flex items-center justify-between px-6">
        <Button variant="ghost" size="sm" onclick={onPrev}>← 이전</Button>
        <Button size="sm" disabled={!canNext} onclick={onNext}>다음</Button>
      </footer>
    {/if}
  {/if}
</div>
