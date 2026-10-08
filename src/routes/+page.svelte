<script lang="ts">
  import Modal from "$lib/components/Modal.svelte";
  import { Button } from "$lib/components/ui/button";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import { wizard } from "$lib/stores/wizard.svelte";
  import { rootToolsState } from "$lib/stores/rootTools.svelte";
  import DeviceStatusView from "$lib/views/DeviceStatusView.svelte";
  import VolteConfigView from "$lib/views/VolteConfigView.svelte";
  import WarningView from "$lib/views/WarningView.svelte";
  import PlanReviewView from "$lib/views/PlanReviewView.svelte";
  import RunProgressView from "$lib/views/RunProgressView.svelte";
  import FinishView from "$lib/views/FinishView.svelte";
  import ResumeJournal from "$lib/components/ResumeJournal.svelte";
  import ModeSelectView from "$lib/views/ModeSelectView.svelte";
  import ManualTasksView from "$lib/views/ManualTasksView.svelte";
  import UpdateConfigView from "$lib/views/UpdateConfigView.svelte";
  import CommunicationView from "$lib/views/CommunicationView.svelte";
  import RootToolsView from "$lib/views/RootToolsView.svelte";

  import { onMount } from "svelte";
  import { observeDesktopWindow } from "$lib/api";
  import { TriangleAlert, ArrowLeft } from "@lucide/svelte/icons";
  import SplashScreen from "$lib/components/SplashScreen.svelte";
  import SettingsView from "$lib/views/SettingsView.svelte";

  let checkingJournal = $state(false);

  // 시작 스플래시 — 첫 기기 조회가 끝나면 닫힌다. 조회가 멈춰도 갇히지 않게 15초 뒤에는 닫는다(첫 화면이 상태를 안내)
  let splashDone = $state(false);
  let splashTimedOut = $state(false);
  $effect(() => {
    if (splashDone) return;
    const timer = setTimeout(() => (splashTimedOut = true), 15000);
    return () => clearTimeout(timer);
  });
  const splashReady = $derived(wizard.startupReady || wizard.view !== "device" || splashTimedOut);
  // 스플래시가 끝나고 폰이 연결돼 있으면(늦게 연결되면 그때) 끝나지 않은 작업을 한 번 확인한다
  $effect(() => {
    if (splashDone && wizard.view === "device" && wizard.device?.state === "device") void wizard.checkStartupJournal();
  });

  // 작업 중 창 닫기 — 실수로 끄는 경우를 대비해 확인 후, 멈춘 사유와 함께 기록을 저장하고 종료
  let closeAsk = $state(false);
  let closing = $state(false);
  let closeWindow: (() => Promise<void>) | null = null;
  onMount(() => {
    let disposed = false;
    let cleanup: (() => void) | undefined;
    void observeDesktopWindow(
      () => {
        if (rootToolsState.busy) return true;
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
  let closeError = $state("");
  async function confirmClose() {
    if (closing || wizard.runInDanger || rootToolsState.busy) return;
    closing = true;
    closeError = "";
    try {
      // 기록 저장에 실패하면 바로 닫지 않고 알린다 (다시 시도 / 기록 없이 종료)
      if (!(await wizard.closeForExit()) && !saveFailed) {
        saveFailed = true;
        return;
      }
      if (!closeWindow) throw new Error("창 종료 기능을 사용할 수 없습니다");
      await closeWindow();
    } catch (error) {
      closeError = error instanceof Error ? error.message : String(error);
    } finally {
      closing = false;
    }
  }

  const canNext = $derived(
    wizard.view === "warning" ? (wizard.mode === "manual" && wizard.manualTask !== "volte" || wizard.omdAck) && wizard.riskAck
    : wizard.view === "step1" ? wizard.hasAnyTask && (wizard.mode !== "update" || wizard.riskAck)
    : true
  );

  const nextReason = $derived(
    wizard.view === "warning" ? (!wizard.riskAck ? "책임 고지에 동의해 주세요" : "OMD 등록 확인에 동의해 주세요")
    : wizard.view === "step1" ? (!wizard.hasAnyTask ? (wizard.mode === "update" ? "업데이트할 버전을 선택하세요" : "VoLTE를 적용할 통신사를 하나 이상 선택하세요") : "업데이트 안내에 동의해 주세요")
    : ""
  );

  async function onNext() {
    if (!canNext || checkingJournal) return;
    switch (wizard.view) {
      case "warning":
        // 시작 팝업에서 [이어서 진행]을 고른 경우 — 동의를 받았으니 바로 이어 간다
        if (wizard.resumeAfterWarning) {
          wizard.pendingJournal = wizard.resumeAfterWarning;
          wizard.resumeJournal();
          break;
        }
        wizard.view = wizard.mode === "manual" && wizard.manualTask !== "volte" ? "step2" : "step1";
        break;
      case "step1":
        wizard.view = "step2"; // step2의 [실행]은 뷰 내부 버튼
        break;
    }
  }

  function onPrev() {
    if (wizard.view === "step2") wizard.view = wizard.optionsPrevious;
    else if (wizard.view === "step1") wizard.view = wizard.mode === "update" ? "mode-select" : "warning";
    else if (wizard.view === "warning") wizard.view = wizard.mode === "manual" ? "manual-tasks" : "mode-select";
  }
</script>

{#if !splashDone}<SplashScreen ready={splashReady} onDone={() => (splashDone = true)} />{/if}
<!-- 끝나지 않은 작업은 스플래시가 끝난 뒤 한 번만 묻는다 -->
{#if splashDone && wizard.pendingJournal}<ResumeJournal journal={wizard.pendingJournal} />{/if}
<div class="h-screen flex flex-col bg-background text-foreground overflow-hidden">
  {#if wizard.view === "device"}
    <!-- 1페이지: 풀스크린 (헤더/사이드바/푸터 없음) -->
    <main class="flex-1 min-h-0 flex flex-col">
      <DeviceStatusView />
    </main>
  {:else if wizard.view === "settings"}
    <main class="flex-1 min-h-0 flex flex-col">
      <SettingsView />
    </main>

  {:else}
    <!-- 2~4페이지 -->
    <!-- 헤더 -->
    <header class="h-10 shrink-0 border-b flex items-center px-4 gap-2 select-none">
      <span class="text-sm font-semibold tracking-tight">Xperia VoLTE Activator</span>
      {#if wizard.mode && wizard.view !== "mode-select"}<span class="text-xs text-muted-foreground">· {wizard.taskTitle}</span>{/if}
      <span class="ml-auto text-[11px] text-muted-foreground">v0.1.0</span>
    </header>
    {#if wizard.journalError}
      <div role="alert" class="shrink-0 flex items-center gap-2 bg-warning-container text-warning px-6 py-2 text-xs"><TriangleAlert size={14} />{wizard.journalError}
        {#if wizard.journalMismatch}<Button size="sm" variant="outline" onclick={() => wizard.discardJournal()}>기존 기록 보관 후 새로 시작</Button>{/if}
      </div>
    {/if}
    <!-- 사이드바 + 콘텐츠 (경고 페이지는 1~4단계 시작 전이라 사이드바 없음) -->
    <div class="flex-1 min-h-0 flex">
      {#if ["step1", "step2", "step3", "step4"].includes(wizard.view)}
        <Sidebar />
      {/if}

      <div class="flex-1 min-w-0 flex flex-col">
        {#if wizard.view === "mode-select"}
          <ModeSelectView />
        {:else if wizard.view === "manual-tasks"}
          <ManualTasksView />
        {:else if wizard.view === "communication"}
          <CommunicationView />
        {:else if wizard.view === "root-tools" && wizard.device}
          <RootToolsView device={wizard.device} section={wizard.manualTask === "root-manager" ? "manager" : "modules"} onClose={() => wizard.returnToTasks()} />
        {:else if wizard.view === "warning"}
          <WarningView />
        {:else if wizard.view === "step1"}
          {#if wizard.mode === "update"}<UpdateConfigView />{:else}<VolteConfigView />{/if}
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
        <Button variant="ghost" size="sm" onclick={onPrev}><ArrowLeft size={14} />이전</Button>
        <div class="flex items-center gap-4">
          <!-- [다음]이 왜 막혔는지 — 동의·선택을 하지 않은 경우 -->
          {#if !canNext && !checkingJournal}<span role="status" class="text-[11px] text-warning">{nextReason}</span>{/if}
          <Button size="sm" onclick={onNext} disabled={!canNext || checkingJournal}>다음</Button>
        </div>
      </footer>
    {/if}
    {#if closeAsk}
      <Modal title="작업 종료 확인" onClose={() => { if (!closing) { closeAsk = false; saveFailed = false; closeError = ""; } }} class="fixed inset-0 z-[60] flex items-center justify-center bg-black/45 p-6">
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
          {#if closeError}
            <div class="rounded-lg bg-danger-container/60 px-4 py-2 text-xs text-destructive">
              종료하지 못했습니다 — {closeError}
            </div>
          {/if}
          {#if wizard.runInDanger}
            <div class="rounded-lg bg-danger-container/60 px-4 py-2 text-xs text-destructive">
              되돌리기 어려운 작업이 진행 중입니다. 중간에 끊기면 폰이 정상적으로 켜지지 않을 수 있습니다.
            </div>
          {/if}
          <div class="flex justify-end gap-2">
            <Button variant="outline" size="sm" disabled={closing} onclick={() => { closeAsk = false; saveFailed = false; closeError = ""; }}>작업 화면으로 돌아가기</Button>
            {#if !wizard.runInDanger}
              <Button variant="destructive" size="sm" disabled={closing} onclick={confirmClose}>{closing ? "기록 저장 중…" : "종료"}</Button>
            {/if}
          </div>
        </div>
      </Modal>
    {/if}
  {/if}
</div>
