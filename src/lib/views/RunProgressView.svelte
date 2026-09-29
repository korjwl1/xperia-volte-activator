<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "$lib/components/ui/card";
  import { Badge } from "$lib/components/ui/badge";
  import { Switch } from "$lib/components/ui/switch";
  import { Label } from "$lib/components/ui/label";
  import { Alert, AlertDescription, AlertTitle } from "$lib/components/ui/alert";
  import { wizard } from "$lib/stores/wizard.svelte";
  import type { RunStatus } from "$lib/types";

  const statusMark: Record<RunStatus, string> = {
    pending: "·", running: "▶", done: "✓", failed: "✗", skipped: "–", "manual-wait": "✋",
  };
  const overallPct = $derived(Math.round(wizard.overall * 100));
</script>

<div class="space-y-4">
  {#if wizard.usbError}
    <Alert variant="destructive">
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

  <Card>
    <CardHeader>
      <div class="flex items-center justify-between gap-3">
        <div>
          <CardTitle class="text-base">실행 {wizard.finished ? "완료" : wizard.running ? "중…" : ""}</CardTitle>
          <CardDescription>전체 {overallPct}% · 파괴적 단계는 확인 게이트 후 진행됩니다</CardDescription>
        </div>
        <div class="flex items-center gap-3">
          <div class="flex items-center gap-2">
            <Switch id="sim-err" checked={wizard.simulateUsbError} onCheckedChange={(v: boolean) => (wizard.simulateUsbError = v)} />
            <Label for="sim-err" class="text-xs text-muted-foreground">USB 오류 시뮬레이션</Label>
          </div>
          {#if !wizard.finished}
            {#if wizard.running}
              <Button size="sm" variant="outline" onclick={() => wizard.pause()}>일시정지</Button>
            {:else}
              <Button size="sm" onclick={() => wizard.begin()}>이어서 실행</Button>
            {/if}
            <Button size="sm" variant="destructive" onclick={() => wizard.abort()}>중단</Button>
          {/if}
        </div>
      </div>
      <div class="h-2 rounded-full bg-muted overflow-hidden mt-2">
        <div class="h-full bg-primary transition-all" style="width: {overallPct}%"></div>
      </div>
    </CardHeader>
    <CardContent class="space-y-2">
      {#each wizard.runSteps as step, i (step.id)}
        <div class="rounded-lg border px-3 py-2 {step.status === 'running' ? 'ring-1 ring-primary' : ''} {step.status === 'pending' ? 'opacity-60' : ''}">
          <div class="flex items-center gap-3">
            <span class="w-5 text-center {step.status === 'done' ? 'text-primary' : step.status === 'failed' ? 'text-destructive' : 'text-muted-foreground'}">
              {statusMark[step.status]}
            </span>
            <span class="text-sm font-medium flex-1">{i + 1}. {step.title}</span>
            <Badge variant={step.status === "done" ? "default" : step.status === "manual-wait" ? "secondary" : "outline"}>
              {step.status === "manual-wait" ? "수동 대기" : step.status === "done" ? "완료" : step.status === "running" ? "실행 중" : step.status}
            </Badge>
          </div>
          {#if step.status === "running" || step.status === "manual-wait"}
            <div class="mt-2 h-1.5 rounded-full bg-muted overflow-hidden ml-8">
              <div class="h-full bg-primary transition-all" style="width: {Math.round(step.progress * 100)}%"></div>
            </div>
          {/if}
          {#if step.logs.length}
            <div class="mt-2 ml-8 max-h-24 overflow-y-auto font-mono text-[11px] text-muted-foreground space-y-0.5">
              {#each step.logs.slice(-4) as line, li (li)}
                <div class="truncate" title={line}>{line}</div>
              {/each}
            </div>
          {/if}
        </div>
      {/each}
    </CardContent>
  </Card>

  {#if wizard.finished}
    <Card>
      <CardHeader>
        <CardTitle class="text-base">완료 요약</CardTitle>
        <CardDescription>복구 승인 전까지 백업 폴더가 보존됩니다 (§6-1)</CardDescription>
      </CardHeader>
      <CardContent class="flex flex-wrap gap-2">
        <Button onclick={() => alert("(mock) 백업 승인 — 폴더 보존 해제")}>복구 완료 승인</Button>
        <Button variant="outline" onclick={() => (wizard.view = "device")}>처음으로</Button>
      </CardContent>
    </Card>
  {/if}
</div>

{#if wizard.manualCurrent}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4" role="dialog">
    <Card class="w-full max-w-lg">
      <CardHeader>
        <CardTitle>✋ {wizard.manualCurrent.title}</CardTitle>
        <CardDescription>폰에서 아래 조작을 완료해 주세요 — 자동 감지되면 진행됩니다 (§12)</CardDescription>
      </CardHeader>
      <CardContent class="space-y-3">
        <ol class="list-decimal list-inside space-y-2 text-sm">
          {#each wizard.manualCurrent.steps as s, i (i)}
            <li>{s}</li>
          {/each}
        </ol>
        <div class="flex justify-end gap-2">
          <Button onclick={() => wizard.ackManual()}>폰에서 완료했어요</Button>
        </div>
      </CardContent>
    </Card>
  </div>
{/if}
