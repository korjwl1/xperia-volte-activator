<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import { Card, CardContent, CardHeader, CardTitle } from "$lib/components/ui/card";
  import { Switch } from "$lib/components/ui/switch";
  import { Label } from "$lib/components/ui/label";
  import { Alert, AlertDescription, AlertTitle } from "$lib/components/ui/alert";
  import { Play, Pause, Square, Usb, ChevronsRight } from "@lucide/svelte/icons";
  import { wizard } from "$lib/stores/wizard.svelte";

  let consoleEl: HTMLDivElement | undefined = $state();

  const allLogs = $derived(
    wizard.runSteps.flatMap((s) => s.logs.map((l) => ({ step: s.title, text: l }))),
  );

  $effect(() => {
    allLogs.length;
    if (consoleEl) consoleEl.scrollTop = consoleEl.scrollHeight;
  });

  const lineColor = (t: string) =>
    t.startsWith("[오류]") ? "text-red-400"
    : t.startsWith("[완료]") || t.startsWith("[재개]") ? "text-emerald-400"
    : t.startsWith("[대기]") ? "text-amber-400"
    : t.startsWith("[시작]") ? "text-sky-400"
    : "text-zinc-400";

  const overallPct = $derived(Math.round(wizard.overall * 100));
  const currentStep = $derived(wizard.runSteps.find((s) => s.status === "running" || s.status === "manual-wait"));
</script>

<div class="flex-1 min-h-0 flex flex-col gap-3 p-4 lg:p-6">
  {#if wizard.usbError}
    <Alert variant="destructive" class="shrink-0">
      <Usb size={16} />
      <AlertTitle>USB 연결이 불안정합니다</AlertTitle>
      <AlertDescription class="flex flex-col gap-2">
        <span>
          {wizard.usbErrorCount === 1
            ? "같은 포트에 다시 연결하거나, 다른 포트(본체 뒷면 권장)로 바꿔 꽂아주세요."
            : "반복 실패 — 케이블 교체나 허브 제거를 권장합니다."}
          작업 상태는 보존되어 재연결 시 이어서 진행됩니다.
        </span>
        <div>
          <Button size="sm" onclick={() => wizard.dismissUsbError()}>재연결 완료 — 이어서 진행</Button>
        </div>
      </AlertDescription>
    </Alert>
  {/if}

  <!-- 상단: 현재 단계 + 전체 진행률 + 컨트롤 -->
  <Card class="elev-1 shrink-0">
    <CardContent class="py-3 space-y-2">
      <div class="flex items-center justify-between gap-4">
        <div class="min-w-0">
          <div class="text-sm font-semibold truncate">
            {currentStep?.title ?? (wizard.finished ? "완료" : "대기 중")}
          </div>
          {#if currentStep}
            <div class="text-[11px] text-muted-foreground">{Math.round(currentStep.progress * 100)}%</div>
          {/if}
        </div>
        <div class="flex items-center gap-2 shrink-0">
          <div class="flex items-center gap-1.5 mr-1">
            <Switch id="sim-err" checked={wizard.simulateUsbError} onCheckedChange={(v: boolean) => (wizard.simulateUsbError = v)} />
            <Label for="sim-err" class="text-[11px] text-muted-foreground cursor-pointer">USB 오류 시뮬</Label>
          </div>
          {#if !wizard.finished}
            {#if wizard.running}
              <Button size="sm" variant="outline" onclick={() => wizard.pause()}><Pause size={13} class="mr-1" />일시정지</Button>
            {:else}
              <Button size="sm" onclick={() => wizard.begin()}><Play size={13} class="mr-1" />{wizard.runSteps.some((s) => s.status !== "pending") ? "이어서" : "실행"}</Button>
            {/if}
            <Button size="sm" variant="destructive" onclick={() => wizard.abort()}><Square size={12} class="mr-1" />중단</Button>
          {:else}
            <Button size="sm" onclick={() => wizard.goFinish()}>다음 단계 →</Button>
          {/if}
        </div>
      </div>
      <div class="h-2.5 rounded-full bg-muted overflow-hidden">
        <div class="h-full grad-hero transition-all" style="width: {overallPct}%"></div>
      </div>
    </CardContent>
  </Card>

  <!-- 콘솔 -->
  <Card class="elev-1 flex-1 min-h-0 flex flex-col overflow-hidden">
    <CardHeader class="py-3 pb-2 shrink-0 border-b flex-row items-center justify-between">
      <CardTitle class="text-xs font-semibold text-muted-foreground tracking-wide">로그</CardTitle>
    </CardHeader>
    <CardContent class="flex-1 p-0 min-h-0">
      <div bind:this={consoleEl} class="console-bg h-full overflow-y-auto px-4 py-3 font-mono text-[11.5px] leading-relaxed">
        {#each allLogs as entry, li (li)}
          <div class="flex gap-2">
            <span class="text-zinc-600 shrink-0 select-none">{String(li + 1).padStart(3, "0")}</span>
            <span class="text-zinc-500 shrink-0 hidden md:inline">[{entry.step}]</span>
            <span class={lineColor(entry.text)}>{entry.text}</span>
          </div>
        {/each}
        {#if wizard.running && !wizard.manualCurrent}
          <div class="flex gap-2 text-primary">
            <ChevronsRight size={13} class="animate-pulse" />
          </div>
        {/if}
      </div>
    </CardContent>
  </Card>
</div>

<!-- 수동 개입 모달 -->
{#if wizard.manualCurrent}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-sm p-4" role="dialog">
    <Card class="w-full max-w-lg elev-3">
      <CardHeader>
        <CardTitle class="text-base">✋ {wizard.manualCurrent.title}</CardTitle>
      </CardHeader>
      <CardContent class="space-y-4">
        <ol class="space-y-2.5">
          {#each wizard.manualCurrent.steps as s, i (i)}
            <li class="flex items-start gap-3 rounded-lg bg-muted/60 px-3 py-2.5 text-sm">
              <span class="flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-primary text-primary-foreground text-[11px] font-bold">{i + 1}</span>
              {s}
            </li>
          {/each}
        </ol>
        <div class="flex items-center justify-between">
          <span class="text-[11px] text-muted-foreground">완료하면 자동으로 다음 단계로 진행됩니다</span>
          <Button onclick={() => wizard.ackManual()}>폰에서 완료했어요</Button>
        </div>
      </CardContent>
    </Card>
  </div>
{/if}
