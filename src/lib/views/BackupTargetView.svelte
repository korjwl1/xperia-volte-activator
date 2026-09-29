<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "$lib/components/ui/card";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Alert, AlertDescription, AlertTitle } from "$lib/components/ui/alert";
  import { FolderOpen, TriangleAlert, CircleCheck } from "@lucide/svelte/icons";
  import { wizard } from "$lib/stores/wizard.svelte";
  import { mockDiskFree } from "$lib/mock/apps";

  let suggested = "D:\\backup\\xperia-1v";

  const segColors = ["bg-primary", "bg-success", "bg-info", "bg-warning", "bg-chart-5"];
  const selectedGroups = $derived(
    wizard.groups
      .map((g) => ({
        ...g,
        bytes: g.items.reduce((a, i) => a + (i.checked ? i.bytes ?? 0 : 0), 0),
        count: g.items.filter((i) => i.checked).length,
      }))
      .filter((g) => g.count > 0),
  );
  const total = $derived(selectedGroups.reduce((a, g) => a + g.bytes, 0));
  const enough = $derived(total <= mockDiskFree);
  const usedRatio = $derived(Math.min(1, total / mockDiskFree));
  const fmtGB = (b: number) => `${(b / 1024 ** 3).toFixed(1)} GB`;
</script>

<div class="space-y-4">
  <Card class="elev-1">
    <CardHeader class="pb-3">
      <CardTitle class="text-sm">백업 저장 위치</CardTitle>
      <CardDescription class="text-xs">복구가 끝날 때까지 백업 파일은 보존됩니다 · 한글/공백 경로 허용</CardDescription>
    </CardHeader>
    <CardContent class="space-y-4">
      <div class="space-y-2">
        <Label for="backup-path" class="text-xs">경로</Label>
        <div class="flex gap-2">
          <div class="flex flex-1 items-center gap-2 rounded-lg border bg-background px-3 focus-within:ring-1 focus-within:ring-ring">
            <FolderOpen size={15} class="shrink-0 text-primary" />
            <input
              id="backup-path"
              class="w-full bg-transparent py-2 font-mono text-[12.5px] outline-none placeholder:text-muted-foreground"
              bind:value={wizard.backupPath}
              placeholder={suggested}
            />
          </div>
          <Button variant="outline" onclick={() => (wizard.backupPath = suggested)} title="M2에서 네이티브 폴더 다이얼로그 연결 (dialog:open)">
            폴더 선택…
          </Button>
        </div>
        {#if wizard.backupPath}
          <p class="text-[11px] text-muted-foreground font-mono">{wizard.backupPath}\backup-20260929-XQ_DQ44\</p>
        {/if}
      </div>

      <!-- 용량 스택 바 -->
      <div class="space-y-2">
        <div class="flex h-4 w-full overflow-hidden rounded-full bg-muted" role="img" aria-label="용량 분포">
          {#each selectedGroups as g, i (g.id)}
            <div class="{segColors[i % segColors.length]} transition-all" style="width: {(g.bytes / Math.max(total, 1)) * 100}%"></div>
          {/each}
        </div>
        <div class="grid gap-1">
          {#each selectedGroups as g, i (g.id)}
            <div class="flex items-center gap-2 text-[12px]">
              <span class="h-2.5 w-2.5 rounded-sm {segColors[i % segColors.length]}"></span>
              <span class="flex-1">{g.label} <span class="text-muted-foreground text-[11px]">({g.count}항목)</span></span>
              <span class="font-mono text-muted-foreground">{fmtGB(g.bytes)}</span>
            </div>
          {/each}
        </div>
        <div class="flex items-center justify-between border-t pt-2 text-[12px]">
          <span class="font-semibold">합계 <span class="font-normal text-muted-foreground">/ 디스크 여유</span></span>
          <span class="font-mono"><b>{fmtGB(total)}</b> <span class="text-muted-foreground">/ {fmtGB(mockDiskFree)}</span></span>
        </div>
        <!-- 사용률 게이지 -->
        <div class="flex items-center gap-2">
          <div class="h-1.5 flex-1 overflow-hidden rounded-full bg-muted">
            <div class="h-full {enough ? 'bg-success' : 'bg-destructive'} transition-all" style="width: {usedRatio * 100}%"></div>
          </div>
          <span class="flex items-center gap-1 text-[11px] {enough ? 'text-success' : 'text-destructive'} font-medium">
            {#if enough}<CircleCheck size={12} />여유 충분{:else}<TriangleAlert size={12} />용량 부족{/if}
          </span>
        </div>
      </div>

      {#if !enough}
        <Alert variant="destructive">
          <AlertTitle>용량 부족</AlertTitle>
          <AlertDescription>대상 디스크 여유가 예상 백업 용량보다 적습니다 — 항목을 줄이거나 경로를 변경하세요.</AlertDescription>
        </Alert>
      {/if}
    </CardContent>
  </Card>

  <Alert class="border-info/30 bg-info-container/50">
    <AlertTitle class="text-[13px]">백업 형식</AlertTitle>
    <AlertDescription class="text-xs">
      원본 그대로 복사되며, Windows에서 표현 불가능한 파일명은 자동으로 안전하게 보관됩니다
    </AlertDescription>
  </Alert>
</div>
