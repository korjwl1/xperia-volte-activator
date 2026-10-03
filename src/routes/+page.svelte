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
  import ResumeJournal from "$lib/components/ResumeJournal.svelte";

  import { onMount } from "svelte";
  import { observeDesktopWindow } from "$lib/api";
  import { TriangleAlert } from "@lucide/svelte/icons";

  let checkingJournal = $state(false);

  // 작업 중 창 닫기 — 실수로 끄는 경우를 대비해 확인 후, 멈춘 사유와 함께 기록을 저장하고 종료
  let closeAsk = $state(false);
  let closing = $state(false);
  let closeWindow: (() => Promise<void>) | null = null;
  onMount(() => {
    let disposed = false;
    let cleanup: (() => void) | undefined;
    void observeDesktopWindow(
      () => {
        if (!wizard.runUnfinished) return false;
        closeAsk = true;
        return true;
      },
      (kind) => wizard.onSessionEnd(kind),
    ).then((session) => {
      if (!session) return;
      if (disposed) return session.dispose();
      cleanup = session.dispose;
      closeWindow = session.close;
    });
    return () => {
      disposed = true;
      cleanup?.();
    };
  });

  let saveFailed = $state(false);
  async function confirmClose() {
    closing = true;
    // 기록 저장에 실패하면 바로 닫지 않고 알린다 (다시 시도 / 기록 없이 종료)
    if (!(await wizard.closeForExit()) && !saveFailed) {
      saveFailed = true;
      closing = false;
      return;
    }
    try {
      await closeWindow?.();
    } finally {
      closing = false;
    }
  }

  const canNext = $derived(
    wizard.view === "warning" ? wizard.omdAck && wizard.riskAck
    : wizard.view === "step1" ? wizard.hasAnyTask
    : true
  );

  async function onNext() {
    if (!canNext || checkingJournal) return;
    switch (wizard.view) {
      case "warning":
        // 같은 폰의 끝나지 않은 작업이 있으면 불러올지 먼저 묻는다
        checkingJournal = true;
        try {
          if (!(await wizard.checkJournal())) wizard.view = "step1";
        } finally {
          checkingJournal = false;
        }
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
        <Button size="sm" onclick={onNext} disabled={!canNext || checkingJournal}>다음</Button>
      </footer>
    {/if}
    {#if wizard.pendingJournal}
      <ResumeJournal journal={wizard.pendingJournal} />
    {/if}
    {#if closeAsk}
      <div class="fixed inset-0 z-[60] flex items-center justify-center bg-black/45 p-6" role="dialog">
        <div class="w-full max-w-md rounded-2xl border-2 {wizard.runInDanger ? 'border-destructive/40' : 'border-border'} bg-background elev-3 p-6 space-y-4">
          <div class="flex items-center gap-3">
            <span class="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl {wizard.runInDanger ? 'bg-danger-container text-destructive' : 'bg-warning-container text-warning'}">
              <TriangleAlert size={20} />
            </span>
            <div class="space-y-0.5">
              <h2 class="text-base font-semibold">{wizard.runInDanger ? "지금은 프로그램을 닫을 수 없습니다" : "작업이 아직 끝나지 않았습니다"}</h2>
              <p class="text-xs text-muted-foreground">
                {wizard.runInDanger
                  ? "이 단계가 끝나면 닫을 수 있습니다"
                  : "지금 종료하면 현재 단계에서 멈추고, 같은 폰을 다시 연결하면 이어서 진행할 수 있습니다"}
              </p>
            </div>
          </div>
          {#if saveFailed}
            <div class="rounded-lg bg-danger-container/60 px-4 py-2 text-xs text-destructive">
              진행 기록을 저장하지 못했습니다 — 지금 종료하면 다음에 이어서 진행할 수 없습니다. 그래도 종료하려면 [종료]를 한 번 더 누르세요.
            </div>
          {/if}
          {#if wizard.runInDanger}
            <div class="rounded-lg bg-danger-container/60 px-4 py-2 text-xs text-destructive">
              되돌리기 어려운 작업이 진행 중입니다. 중간에 끊기면 폰이 정상적으로 켜지지 않을 수 있습니다.
            </div>
          {/if}
          <div class="flex justify-end gap-2">
            <Button variant="outline" size="sm" disabled={closing} onclick={() => { closeAsk = false; saveFailed = false; }}>계속 작업</Button>
            {#if !wizard.runInDanger}
              <Button variant="destructive" size="sm" disabled={closing} onclick={confirmClose}>{closing ? "기록 저장 중…" : "종료"}</Button>
            {/if}
          </div>
        </div>
      </div>
    {/if}
  {/if}
</div>
