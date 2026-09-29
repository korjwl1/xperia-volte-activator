<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "$lib/components/ui/card";
  import { Badge } from "$lib/components/ui/badge";
  import { Checkbox } from "$lib/components/ui/checkbox";
  import { Alert, AlertDescription, AlertTitle } from "$lib/components/ui/alert";
  import { wizard } from "$lib/stores/wizard.svelte";
  import { mockGoogleBackupAge } from "$lib/mock/apps";
  import type { BackupClass } from "$lib/types";

  let skipStage = $state<0 | 1 | 2>(0);

  const clsVariant = (c: BackupClass) => (c === "full" ? "default" : c === "partial" ? "secondary" : "destructive");
  const clsMark = (c: BackupClass) => (c === "full" ? "✅" : c === "partial" ? "⚠️" : "❌");
  const fmtBytes = (b?: number) => {
    if (!b) return "";
    const gb = b / 1024 ** 3;
    return gb >= 1 ? `${gb.toFixed(1)} GB` : `${Math.round(b / 1024 ** 2)} MB`;
  };
</script>

<div class="space-y-4">
  <Alert>
    <AlertTitle>데이터 초기화 경로가 감지되었습니다</AlertTitle>
    <AlertDescription>
      이 계획에는 초기화를 유발하는 단계가 포함되어 있어 백업을 권장합니다.
      <span class="ml-1 text-muted-foreground">구글 백업: {mockGoogleBackupAge} (§6-4 최신성 표시)</span>
    </AlertDescription>
  </Alert>

  {#each wizard.groups as group (group.id)}
    <Card>
      <CardHeader class="pb-3">
        <div class="flex items-center justify-between gap-2">
          <div>
            <CardTitle class="text-base">{group.label}</CardTitle>
            <CardDescription>{group.desc}</CardDescription>
          </div>
          <div class="flex items-center gap-2">
            <span class="text-xs text-muted-foreground">{fmtBytes(group.items.reduce((a, i) => a + (i.bytes ?? 0), 0))}</span>
            <Button size="sm" variant="ghost" onclick={() => wizard.setGroupAll(group.id, true)}>전체</Button>
            <Button size="sm" variant="ghost" onclick={() => wizard.setGroupAll(group.id, false)}>해제</Button>
          </div>
        </div>
      </CardHeader>
      <CardContent class="space-y-1">
        {#each group.items as item (item.id)}
          <label
            class="flex items-center gap-3 rounded-md px-2 py-1.5 hover:bg-accent {item.cls === 'none' ? 'opacity-70' : 'cursor-pointer'}"
          >
            <Checkbox
              checked={item.checked}
              disabled={item.cls === "none"}
              onCheckedChange={(v: boolean | "indeterminate") => (item.checked = v === true)}
            />
            <span class="text-sm flex-1 min-w-0 truncate" title={item.label}>{item.label}</span>
            {#if item.bytes}
              <span class="text-xs text-muted-foreground">{fmtBytes(item.bytes)}</span>
            {/if}
            <Badge variant={clsVariant(item.cls)} class="whitespace-nowrap">
              {clsMark(item.cls)} {item.cls === "full" ? "완전" : item.cls === "partial" ? "불완전" : "불가"}
            </Badge>
            {#if item.note}
              <span class="text-xs text-muted-foreground hidden md:block max-w-[280px] truncate" title={item.note}>{item.note}</span>
            {/if}
          </label>
        {/each}
      </CardContent>
    </Card>
  {/each}

  <Alert variant="destructive">
    <AlertTitle>❌ 항목은 체크할 수 없습니다</AlertTitle>
    <AlertDescription>
      앱 데이터가 구조적으로 백업되지 않는 항목입니다 — 각 항목의 비고(인증서 내보내기, 인계코드 등)를 지금 미리 처리해 두세요.
    </AlertDescription>
  </Alert>

  <div class="flex justify-end">
    {#if wizard.backupSkippable}
      <Button variant="ghost" size="sm" class="text-destructive" onclick={() => (skipStage = 1)}>백업 건너뛰기…</Button>
    {/if}
  </div>

  {#if skipStage === 1}
    <Alert variant="destructive">
      <AlertTitle>백업을 건너뛰시겠습니까? (확인 1/2)</AlertTitle>
      <AlertDescription class="flex items-center gap-2">
        <span>이 계획에는 데이터 초기화 단계가 포함되어 있습니다.</span>
        <Button size="sm" variant="destructive" onclick={() => (skipStage = 2)}>계속</Button>
        <Button size="sm" variant="outline" onclick={() => (skipStage = 0)}>취소</Button>
      </AlertDescription>
    </Alert>
  {:else if skipStage === 2}
    <Alert variant="destructive">
      <AlertTitle>정말 백업 없이 진행할까요? (확인 2/2)</AlertTitle>
      <AlertDescription class="flex items-center gap-2">
        <span>초기화 시 모든 데이터(사진·앱·설정)가 영구 삭제되며 복구할 수 없습니다.</span>
        <Button size="sm" variant="destructive" onclick={() => wizard.skipBackupFlow()}>백업 건너뛰고 진행</Button>
        <Button size="sm" variant="outline" onclick={() => (skipStage = 0)}>취소</Button>
      </AlertDescription>
    </Alert>
  {/if}
</div>
