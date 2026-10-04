<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "$lib/api";
  import { Input } from "$lib/components/ui/input";
  import { Button } from "$lib/components/ui/button";
  import { FolderOpen, CheckCircle2, TriangleAlert, LoaderCircle } from "@lucide/svelte/icons";

  let { dirty = $bindable(true) }: { dirty?: boolean } = $props();

  let port = $state("");
  let presetRoot = $state("");
  let snapshotRoot = $state("");
  let busy = $state(false);
  let error = $state("");
  let saved = $state(false);
  let alive = true;
  onMount(() => {
    busy = true;
    void api.efsConfiguration().then(result => {
      if (!alive) return;
      if (!result.ok) error = result.error;
      else if (result.value) {
        ({ port, presetRoot, snapshotRoot } = result.value);
        saved = true;
        dirty = false;
      }
    }).finally(() => { if (alive) busy = false; });
    return () => { alive = false; };
  });
  function changed() { saved = false; dirty = true; error = ""; }
  async function pick(which: "preset" | "snapshot") {
    const path = await api.pickFolder();
    if (!alive || !path) return;
    if (which === "preset") presetRoot = path; else snapshotRoot = path;
    changed();
  }
  async function save() {
    if (busy) return;
    busy = true; error = ""; saved = false;
    try {
      const result = await api.efsConfigure({ port: port.trim().toUpperCase(), presetRoot, snapshotRoot });
      if (!alive) return;
      if (result.ok) { port = port.trim().toUpperCase(); saved = true; dirty = false; }
      else error = result.error;
    } finally { if (alive) busy = false; }
  }
</script>

<div class="mb-4 rounded-xl bg-muted/40 elev-1 p-3 space-y-2">
  <div class="text-[13px] font-semibold">VoLTE 적용 위치</div>
  <p class="text-[11px] text-muted-foreground">선택한 폰의 Qualcomm DIAG 포트를 직접 지정하세요. 프리셋 위치는 util 폴더가 들어 있는 원본 배포 폴더입니다.</p>
  <label for="efs-port" class="block text-[11px]">DIAG 포트</label>
  <Input id="efs-port" bind:value={port} oninput={changed} placeholder="COM 번호" disabled={busy} class="h-8 font-mono text-xs" />
  <label for="efs-preset" class="block text-[11px]">원본 프리셋 위치</label>
  <div class="flex gap-2">
    <Input id="efs-preset" bind:value={presetRoot} oninput={changed} disabled={busy} class="h-8 text-xs" />
    <Button size="sm" variant="outline" disabled={busy} onclick={() => pick("preset")} aria-label="원본 프리셋 폴더 선택"><FolderOpen size={14} /></Button>
  </div>
  <label for="efs-snapshot" class="block text-[11px]">변경 전 복원본 저장 위치</label>
  <div class="flex gap-2">
    <Input id="efs-snapshot" bind:value={snapshotRoot} oninput={changed} disabled={busy} class="h-8 text-xs" />
    <Button size="sm" variant="outline" disabled={busy} onclick={() => pick("snapshot")} aria-label="복원본 저장 폴더 선택"><FolderOpen size={14} /></Button>
  </div>
  <div class="flex items-center gap-2">
    <Button size="sm" variant="outline" onclick={save} disabled={busy || !port.trim() || !presetRoot.trim() || !snapshotRoot.trim()}>설정 저장</Button>
    {#if busy}<LoaderCircle size={14} class="animate-spin text-primary" />
    {:else if saved}<span class="inline-flex items-center gap-1 text-[11px] text-success"><CheckCircle2 size={14} />저장됨</span>
    {:else}<span class="text-[11px] text-muted-foreground">변경한 설정을 저장해 주세요</span>{/if}
  </div>
  {#if error}<p class="flex gap-1 text-[11px] text-destructive" role="alert"><TriangleAlert size={14} class="shrink-0" />{error}</p>{/if}
</div>
