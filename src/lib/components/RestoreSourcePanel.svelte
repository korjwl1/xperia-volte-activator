<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import { Checkbox } from "$lib/components/ui/checkbox";
  import { CircleCheck, FolderOpen, LoaderCircle, OctagonX, Info } from "@lucide/svelte/icons";
  import { api } from "$lib/api";
  import { onMount } from "svelte";
  import { wizard } from "$lib/stores/wizard.svelte";

  // 복원: 폰이 아니라 고른 백업 폴더가 기준이다 — 폴더를 고르면 그 안에서 인식한 항목만 보여 주고 사용자가 고른다
  const fmtBytes = (b: number) => {
    const gb = b / 1024 ** 3;
    if (gb >= 1) return `${gb.toFixed(1)} GB`;
    const mb = b / 1024 ** 2;
    if (mb >= 1) return `${mb.toFixed(0)} MB`;
    return `${Math.max(1, Math.round(b / 1024))} KB`;
  };
  const found = $derived(new Map(wizard.restoreItems.filter(i => i.status === "done").map(i => [i.id, i])));
  const groups = $derived(wizard.groups
    .map(g => ({ ...g, items: g.items.filter(item => found.has(item.id)) }))
    .filter(g => g.items.length > 0));
  const selected = $derived(groups.flatMap(g => g.items).filter(i => i.checked));
  const selectedBytes = $derived(selected.reduce((sum, i) => sum + (found.get(i.id)?.bytes ?? 0), 0));
  const loading = $derived(wizard.restoreSourceState === "loading");
  const ready = $derived(wizard.restoreSourceState === "done" && !!wizard.backupDir);

  // 이 폰의 마지막 백업 폴더를 기억하고 있으면 먼저 골라 둔다(다른 폴더로 바꿀 수 있다)
  onMount(async () => {
    if (wizard.backupDir || wizard.restoreSourceState === "loading") return;
    const record = await wizard.loadDeviceRecord();
    if (record?.lastBackupDir && !wizard.backupDir) await wizard.loadRestoreSource(record.lastBackupDir);
  });

  async function pick() {
    const path = await api.pickFolder();
    if (path) await wizard.loadRestoreSource(path);
  }
  function setGroup(items: { checked: boolean }[], value: boolean) {
    for (const item of items) item.checked = value;
  }
</script>

<div class="space-y-5">
  <!-- 1. 백업 폴더 -->
  <section class="rounded-2xl bg-card elev-1 p-5">
    <div class="flex items-center gap-4">
      <span class="inline-flex size-12 shrink-0 items-center justify-center rounded-xl {ready ? 'bg-success-container text-success' : 'bg-primary/10 text-primary'}">
        {#if loading}<LoaderCircle size={22} class="animate-spin" />{:else if ready}<CircleCheck size={22} />{:else}<FolderOpen size={22} />{/if}
      </span>
      <div class="min-w-0 flex-1">
        <p class="text-sm font-semibold">복원할 백업 폴더</p>
        <p class="mt-0.5 truncate font-mono text-xs text-muted-foreground" title={wizard.backupDir}>
          {#if loading}백업 목록을 읽는 중…{:else if ready}{wizard.backupDir}{:else}이 앱으로 만든 백업 폴더(xva-&lt;모델&gt;-backup)를 선택하세요{/if}
        </p>
      </div>
      <Button size="sm" variant={ready ? "outline" : "default"} disabled={loading} onclick={pick}>{ready ? "다른 폴더 선택" : "폴더 선택"}</Button>
    </div>
    {#if wizard.restoreSourceError}
      <p role="alert" class="mt-3 flex items-start gap-1.5 rounded-lg bg-danger-container px-3 py-2 text-xs text-destructive"><OctagonX size={14} class="mt-px shrink-0" />{wizard.restoreSourceError}</p>
    {/if}
    {#if ready && wizard.backupSummary}
      <p class="mt-3 text-xs text-muted-foreground">같은 폰의 백업 · 항목 {found.size}개 · 파일 {wizard.backupSummary.files.toLocaleString()}개 · {fmtBytes(wizard.backupSummary.bytes)}</p>
    {/if}
  </section>

  <!-- 2. 백업에서 인식한 항목 -->
  {#if ready}
    <section class="space-y-4">
      <div class="flex items-baseline justify-between px-1">
        <h2 class="text-sm font-semibold">복원할 항목</h2>
        <span class="text-xs text-muted-foreground">{selected.length}개 선택 · {fmtBytes(selectedBytes)}</span>
      </div>
      {#each groups as group (group.id)}
        {@const all = group.items.every(i => i.checked)}
        <div class="space-y-1.5">
          <div class="flex items-center justify-between px-1 text-xs">
            <span class="font-medium text-muted-foreground">{group.label}</span>
            <button type="button" class="text-muted-foreground hover:text-foreground" onclick={() => setGroup(group.items, !all)}>{all ? "전체 해제" : "전체 선택"}</button>
          </div>
          {#each group.items as item (item.id)}
            {@const info = found.get(item.id)}
            <label class="flex cursor-pointer items-center gap-3 rounded-xl border px-4 py-3 transition-colors {item.checked ? 'border-primary/30 bg-primary/5' : 'border-border bg-card hover:bg-muted/50'}">
              <Checkbox checked={item.checked} onCheckedChange={(v: boolean | "indeterminate") => (item.checked = v === true)} />
              <span class="flex-1 text-sm">{item.label}</span>
              <span class="font-mono text-xs text-muted-foreground">{info ? `${info.files.toLocaleString()}개 · ${fmtBytes(info.bytes)}` : ""}</span>
            </label>
          {/each}
        </div>
      {/each}
      <p class="flex items-start gap-1.5 px-1 text-[11px] leading-relaxed text-muted-foreground"><Info size={13} class="mt-px shrink-0" />실행하면 먼저 백업 파일 전체의 크기·해시를 확인한 뒤 폰에 복원합니다. 용량이 크면 확인에만 수십 분이 걸릴 수 있습니다.</p>
    </section>
  {/if}
</div>
