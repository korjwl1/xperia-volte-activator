<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "$lib/components/ui/card";
  import { Badge } from "$lib/components/ui/badge";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Alert, AlertDescription, AlertTitle } from "$lib/components/ui/alert";
  import { wizard } from "$lib/stores/wizard.svelte";
  import { mockDiskFree } from "$lib/mock/apps";

  let suggested = "D:\\backup\\xperia-1v";

  const selectedGroups = $derived(
    wizard.groups.map((g) => ({
      ...g,
      bytes: g.items.reduce((a, i) => a + (i.checked ? i.bytes ?? 0 : 0), 0),
      count: g.items.filter((i) => i.checked).length,
    })),
  );
  const total = $derived(selectedGroups.reduce((a, g) => a + g.bytes, 0));
  const enough = $derived(total <= mockDiskFree);
  const fmtGB = (b: number) => `${(b / 1024 ** 3).toFixed(1)} GB`;
</script>

<div class="space-y-4">
  <Card>
    <CardHeader>
      <CardTitle class="text-base">백업 저장 위치</CardTitle>
      <CardDescription>
        복구 승인 전까지 백업 폴더는 보존됩니다 (§6-1) · 한글/공백 경로 허용
      </CardDescription>
    </CardHeader>
    <CardContent class="space-y-3">
      <div class="space-y-2">
        <Label for="backup-path">경로</Label>
        <div class="flex gap-2">
          <Input id="backup-path" bind:value={wizard.backupPath} placeholder={suggested} class="font-mono" />
          <Button
            variant="outline"
            onclick={() => (wizard.backupPath = suggested)}
            title="M2에서 네이티브 폴더 다이얼로그 연결 (dialog:open)"
          >폴더 선택…</Button>
        </div>
        {#if wizard.backupPath}
          <p class="text-xs text-muted-foreground font-mono">
            {wizard.backupPath}\\backup-20260929-XQ_DQ44\\
          </p>
        {/if}
      </div>

      <div class="rounded-lg border divide-y">
        {#each selectedGroups as g (g.id)}
          {#if g.count > 0}
            <div class="flex items-center justify-between px-3 py-2 text-sm">
              <span>{g.label} <span class="text-xs text-muted-foreground">({g.count}항목)</span></span>
              <span class="font-mono text-muted-foreground">{fmtGB(g.bytes)}</span>
            </div>
          {/if}
        {/each}
        <div class="flex items-center justify-between px-3 py-2 font-medium">
          <span>합계</span>
          <span class="font-mono">{fmtGB(total)}</span>
        </div>
        <div class="flex items-center justify-between px-3 py-2 text-sm">
          <span class="text-muted-foreground">대상 디스크 여유</span>
          <span class="font-mono text-muted-foreground">{fmtGB(mockDiskFree)}</span>
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

  <Alert>
    <AlertTitle>백업 형식</AlertTitle>
    <AlertDescription>
      폴더 그대로 + manifest.json(해시·mtime) — Windows 비호환 파일명은 quarantine 세그먼트로 자동 격리 후 복구 시 기기 측에서 원본 이름으로 복원됩니다 (§6-2/6-3)
    </AlertDescription>
  </Alert>
</div>
