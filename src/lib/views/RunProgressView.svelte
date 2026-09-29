<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import { Card, CardContent, CardHeader, CardTitle } from "$lib/components/ui/card";
  import { Badge } from "$lib/components/ui/badge";
  import { Switch } from "$lib/components/ui/switch";
  import { Label } from "$lib/components/ui/label";
  import { Alert, AlertDescription, AlertTitle } from "$lib/components/ui/alert";
  import {
    CircleCheck, CircleAlert, TriangleAlert, LoaderCircle, Circle, Hand,
    Play, Pause, Square, Usb, ChevronsRight,
  } from "@lucide/svelte/icons";
  import { wizard } from "$lib/stores/wizard.svelte";
  import type { RunStatus } from "$lib/types";

  let consoleEl: HTMLDivElement | undefined = $state();

  // 콘솔에 뿌릴 통합 로그 (단계 순서 보존)
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

  const statusIcon = (s: RunStatus) =>
    s === "done" ? CircleCheck : s === "running" ? LoaderCircle
    : s === "manual-wait" ? Hand : s === "failed" ? CircleAlert : Circle;
  const statusColor = (s: RunStatus) =>
    s === "done" ? "text-success" : s === "running" ? "text-primary"
    : s === "manual-wait" ? "text-warning" : s === "failed" ? "text-destructive" : "text-muted-foreground/50";

  const overallPct = $derived(Math.round(wizard.overall * 100));
</script>

<div class="flex h-full min-h-0 flex-col gap-3">
  {#if wizard.usbError}
    <Alert variant="destructive" class="shrink-0">
      <Usb size={16} />
      <AlertTitle>USB 연결 불안정 감지 (§9-4)</AlertTitle>
      <AlertDescription class="flex flex-col gap-2">
        <span>
          {wizard.usbErrorCount === 1
            ? "같은 포트에 다시 연결하거나, 다른 포트(뒷면 직결 권장)로 바꿔 꽂아주세요."
            : "반복 실패 — 케이블 교체·외부 허브 제거를 권장합니다. 지난 성공: 뒷면 2번 (Intel 칩셋)"}
          세션은 보존되어 재연결 시 이어서 진행됩니다.
        </span>
        <div>
          <Button size="sm" onclick={() => wizard.dismissUsbError()}>재연결 완료 — 이어서 진행</Button>
        </div>
      </AlertDescription>
    </Alert>
  {/if}

  <!-- 상단: 전체 진행 + 컨트롤 -->
  <Card class="elev-1 shrink-0">
    <CardContent class="flex items-center gap-4 py-3">
      <div class="flex-1 min-w-0">
        <div class="flex items-center justify-between text-xs mb-1.5">
          <span class="font-semibold">
            {#if wizard.finished}완료{:else if wizard.running}실행 중…{:else if wizard.runSteps.some((s) => s.status !== "pending")}일시정지{:else}대기{/if}
          </span>
          <span class="font-mono text-muted-foreground">{overallPct}%</span>
        </div>
        <div class="h-2.5 rounded-full bg-muted overflow-hidden">
          <div class="h-full grad-hero transition-all" style="width: {overallPct}%"></div>
        </div>
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
        {/if}
      </div>
    </CardContent>
  </Card>

  {#if wizard.finished}
    <Card class="elev-1 shrink-0">
      <CardContent class="flex items-center justify-between py-3">
        <div class="flex items-center gap-2 text-sm">
          <CircleCheck size={16} class="text-success" />
          모든 단계 완료 — 복구 승인 전까지 백업 폴더가 보존됩니다 (§6-1)
        </div>
        <div class="flex gap-2">
          <Button size="sm" onclick={() => alert("(mock) 백업 승인 — 폴더 보존 해제")}>복구 완료 승인</Button>
          <Button size="sm" variant="outline" onclick={() => (wizard.view = "device")}>처음으로</Button>
        </div>
      </CardContent>
    </Card>
  {/if}

  <!-- 분할: 단계 타임라인 | 콘솔 -->
  <div class="flex-1 min-h-0 grid grid-cols-[minmax(280px,360px)_1fr] gap-3">
    <!-- 좌: 단계 -->
    <Card class="elev-1 min-h-0 flex flex-col">
      <CardHeader class="py-3 pb-2 shrink-0 border-b">
        <CardTitle class="text-xs font-semibold text-muted-foreground tracking-wide">단계</CardTitle>
      </CardHeader>
      <CardContent class="flex-1 overflow-y-auto p-2 space-y-1">
        {#each wizard.runSteps as step, i (step.id)}
          {@const Icon = statusIcon(step.status)}
          <div class="flex items-start gap-2.5 rounded-lg px-2 py-1.5 {step.status === 'running' ? 'bg-primary/10 ring-1 ring-primary/30' : ''}">
            <Icon size={15} class="mt-0.5 shrink-0 {statusColor(step.status)} {step.status === 'running' ? 'animate-spin' : ''}" />
            <div class="min-w-0 flex-1">
              <div class="text-[12px] font-medium leading-tight {step.status === 'pending' ? 'text-muted-foreground' : ''}">
                {i + 1}. {step.title}
              </div>
              {#if step.status === "running" || step.status === "manual-wait"}
                <div class="mt-1 h-1 rounded-full bg-muted overflow-hidden">
                  <div class="h-full bg-primary transition-all" style="width: {Math.round(step.progress * 100)}%"></div>
                </div>
              {/if}
            </div>
            {#if step.status === "manual-wait"}
              <Badge variant="secondary" class="text-[9px] px-1.5 py-0 shrink-0">수동</Badge>
            {/if}
          </div>
        {/each}
      </CardContent>
    </Card>

    <!-- 우: 콘솔 -->
    <Card class="elev-1 min-h-0 flex flex-col overflow-hidden">
      <CardHeader class="py-3 pb-2 shrink-0 border-b flex-row items-center justify-between">
        <CardTitle class="text-xs font-semibold text-muted-foreground tracking-wide">로그</CardTitle>
        <span class="flex items-center gap-1 text-[10px] text-muted-foreground">
          <TriangleAlert size={10} /> 민감정보 마스킹 적용 (§12.5)
        </span>
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
</div>

<!-- 수동 개입 모달 -->
{#if wizard.manualCurrent}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-sm p-4" role="dialog">
    <Card class="w-full max-w-lg elev-3">
      <CardHeader>
        <CardTitle class="flex items-center gap-2 text-base">
          <span class="flex h-8 w-8 items-center justify-center rounded-lg bg-warning-container text-warning"><Hand size={16} /></span>
          {wizard.manualCurrent.title}
        </CardTitle>
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
          <span class="text-[11px] text-muted-foreground">자동 감지되면 모달이 닫힙니다 (§12)</span>
          <Button onclick={() => wizard.ackManual()}>폰에서 완료했어요</Button>
        </div>
      </CardContent>
    </Card>
  </div>
{/if}
