<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import { Card, CardContent, CardHeader } from "$lib/components/ui/card";
  import { Badge } from "$lib/components/ui/badge";
  import { Checkbox } from "$lib/components/ui/checkbox";
  import { Alert, AlertDescription, AlertTitle } from "$lib/components/ui/alert";
  import {
    FolderDown, Settings2, LayoutGrid, HardDrive, EyeOff, MessageSquareText,
    CircleCheck, TriangleAlert, OctagonX, CloudCheck,
  } from "@lucide/svelte/icons";
  import { wizard } from "$lib/stores/wizard.svelte";
  import { mockGoogleBackupAge } from "$lib/mock/apps";
  import type { BackupClass } from "$lib/types";

  let skipStage = $state<0 | 1 | 2>(0);

  const groupIcons: Record<string, typeof FolderDown> = {
    settings: Settings2,
    apps: LayoutGrid,
    storage: HardDrive,
    hidden: EyeOff,
    sms: MessageSquareText,
  };

  const clsChip = (c: BackupClass) =>
    c === "full" ? "bg-success-container text-success"
    : c === "partial" ? "bg-warning-container text-warning"
    : "bg-danger-container text-destructive";
  const clsLabel = (c: BackupClass) => (c === "full" ? "완전" : c === "partial" ? "불완전" : "불가");

  const fmtBytes = (b?: number) => {
    if (!b) return "";
    const gb = b / 1024 ** 3;
    return gb >= 1 ? `${gb.toFixed(1)} GB` : `${Math.round(b / 1024 ** 2)} MB`;
  };
</script>

<div class="space-y-4">
  <Alert class="border-warning/40 bg-warning-container/60">
    <TriangleAlert size={16} class="text-warning" />
    <AlertTitle>데이터 초기화 경로가 감지되었습니다</AlertTitle>
    <AlertDescription class="flex items-center gap-2 flex-wrap">
      이 계획에는 기기 초기화 단계가 포함되어 있어 백업을 권장합니다.
      <span class="inline-flex items-center gap-1 rounded-full bg-info-container px-2 py-0.5 text-[11px] text-info font-medium">
        <CloudCheck size={11} />구글 백업: {mockGoogleBackupAge}
      </span>
    </AlertDescription>
  </Alert>

  {#each wizard.groups as group (group.id)}
    {@const Icon = groupIcons[group.id] ?? FolderDown}
    <Card class="elev-1">
      <CardHeader class="sticky top-0 z-10 rounded-t-xl bg-card/95 backdrop-blur py-3 pb-2">
        <div class="flex items-center justify-between gap-2">
          <div class="flex items-center gap-3 min-w-0">
            <span class="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary">
              <Icon size={16} />
            </span>
            <div class="min-w-0">
              <div class="text-[13px] font-semibold leading-tight">{group.label}</div>
              <div class="text-[11px] text-muted-foreground truncate">{group.desc}</div>
            </div>
          </div>
          <div class="flex shrink-0 items-center gap-1.5">
            <span class="text-[11px] text-muted-foreground font-mono">{fmtBytes(group.items.reduce((a, i) => a + (i.bytes ?? 0), 0))}</span>
            <Button size="sm" variant="ghost" class="h-7 px-2 text-[11px]" onclick={() => wizard.setGroupAll(group.id, true)}>전체</Button>
            <Button size="sm" variant="ghost" class="h-7 px-2 text-[11px]" onclick={() => wizard.setGroupAll(group.id, false)}>해제</Button>
          </div>
        </div>
      </CardHeader>
      <CardContent class="space-y-0.5 pt-1">
        {#each group.items as item (item.id)}
          <label class="flex items-center gap-2.5 rounded-md px-2 py-1.5 hover:bg-accent/50 {item.cls === 'none' ? 'opacity-65' : 'cursor-pointer'}">
            <Checkbox
              checked={item.checked}
              disabled={item.cls === "none"}
              onCheckedChange={(v: boolean | "indeterminate") => (item.checked = v === true)}
            />
            <span class="text-[12.5px] flex-1 min-w-0 truncate" title={item.label}>{item.label}</span>
            {#if item.bytes}
              <span class="text-[11px] text-muted-foreground font-mono shrink-0">{fmtBytes(item.bytes)}</span>
            {/if}
            <span class="inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[10px] font-semibold shrink-0 {clsChip(item.cls)}">
              {#if item.cls === "full"}<CircleCheck size={11} />{:else if item.cls === "partial"}<TriangleAlert size={11} />{:else}<OctagonX size={11} />{/if}
              {clsLabel(item.cls)}
            </span>
            {#if item.note}
              <span class="hidden lg:block max-w-[240px] truncate text-[11px] text-muted-foreground shrink-0" title={item.note}>{item.note}</span>
            {/if}
          </label>
        {/each}
      </CardContent>
    </Card>
  {/each}

  <Alert variant="destructive" class="bg-danger-container/60 border-destructive/30">
    <OctagonX size={16} />
    <AlertTitle>❌ 항목은 체크할 수 없습니다</AlertTitle>
    <AlertDescription>
      앱 데이터가 구조적으로 백업되지 않는 항목입니다 — 비고(인증서 내보내기, 인계코드 등)를 지금 미리 처리해 두세요.
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
        <span>초기화 시 모든 데이터가 영구 삭제되며 복구할 수 없습니다.</span>
        <Button size="sm" variant="destructive" onclick={() => wizard.skipBackupFlow()}>백업 건너뛰고 진행</Button>
        <Button size="sm" variant="outline" onclick={() => (skipStage = 0)}>취소</Button>
      </AlertDescription>
    </Alert>
  {/if}
</div>
