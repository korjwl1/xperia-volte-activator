<script lang="ts">
  import { onMount } from "svelte";
  import { Button } from "$lib/components/ui/button";
  import { Checkbox } from "$lib/components/ui/checkbox";
  import Modal from "$lib/components/Modal.svelte";
  import { ArrowLeft, LoaderCircle, OctagonX, RefreshCw, Trash2 } from "@lucide/svelte/icons";
  import { api } from "$lib/api";
  import { wizard } from "$lib/stores/wizard.svelte";
  import type { RecordItem } from "$lib/types";

  // 설정 → 기록 관리: 앱 데이터 폴더에 쌓인 기록을 보고, 여러 개를 골라 지운다(2026-10-08 사용자 요청)
  const GROUPS: { kind: RecordItem["kind"]; title: string; note: string }[] = [
    { kind: "patch", title: "VoLTE 패치 기록", note: "패치 전 모뎀 설정 사본 포함 — 지우면 그 패치는 되돌릴 수 없습니다" },
    { kind: "backup-location", title: "마지막 백업 위치", note: "복원 화면이 먼저 고르는 폴더 기억 — 백업 파일은 지우지 않습니다" },
    { kind: "journal", title: "진행 기록", note: "끊긴 작업을 이어서 하기 위한 기록" },
    { kind: "firmware", title: "순정 부트 이미지 캐시", note: "지우면 다음 작업 때 Sony 서버에서 다시 받습니다" },
    { kind: "snapshot", title: "기록 없는 모뎀 설정 사본", note: "어떤 패치 기록에도 연결되지 않은 사본" },
  ];
  let items = $state<RecordItem[]>([]);
  let selected = $state<Set<string>>(new Set());
  let loading = $state(true);
  let error = $state("");
  let confirmOpen = $state(false);
  let deleting = $state(false);

  const fmtBytes = (b: number) => b >= 1024 ** 3 ? `${(b / 1024 ** 3).toFixed(1)} GB` : b >= 1024 ** 2 ? `${(b / 1024 ** 2).toFixed(0)} MB` : b > 0 ? `${Math.max(1, Math.round(b / 1024))} KB` : "";
  const fmtDate = (at: string) => { const d = new Date(at); return Number.isNaN(d.getTime()) ? "" : d.toLocaleString("ko-KR", { dateStyle: "medium", timeStyle: "short" }); };
  const picked = $derived(items.filter(i => selected.has(i.id)));
  const pickedBytes = $derived(picked.reduce((sum, i) => sum + i.bytes, 0));

  async function load() {
    loading = true; error = "";
    const r = await api.recordsList();
    loading = false;
    if (!r.ok) { error = r.error; return; }
    items = r.value;
    selected = new Set([...selected].filter(id => items.some(i => i.id === id)));
  }
  function toggle(id: string, on: boolean) {
    const next = new Set(selected);
    if (on) next.add(id); else next.delete(id);
    selected = next;
  }
  function toggleGroup(kind: string, on: boolean) {
    const next = new Set(selected);
    for (const i of items.filter(i => i.kind === kind)) if (on) next.add(i.id); else next.delete(i.id);
    selected = next;
  }
  async function remove() {
    deleting = true; error = "";
    const r = await api.recordsDelete(picked.map(i => i.id));
    deleting = false; confirmOpen = false;
    if (!r.ok) error = r.error;
    selected = new Set();
    wizard.deviceRecord = null;
    await load();
  }
  onMount(() => { void load(); });
</script>

<div class="flex-1 min-h-0 flex flex-col">
  <div class="flex-1 min-h-0 overflow-y-auto p-6">
    <div class="mx-auto w-full max-w-4xl space-y-5">
      <div class="flex items-end justify-between gap-4">
        <div>
          <h1 class="text-xl font-semibold">설정 · 기록 관리</h1>
          <p class="mt-1 text-xs text-muted-foreground">이 PC의 앱 데이터 폴더에 저장된 기록입니다. 여러 개를 골라 한 번에 지울 수 있습니다.</p>
        </div>
        <Button variant="outline" size="sm" disabled={loading} onclick={load}><RefreshCw size={13} />다시 읽기</Button>
      </div>
      {#if error}<p role="alert" class="flex items-start gap-1.5 rounded-lg bg-danger-container px-3 py-2 text-xs text-destructive"><OctagonX size={14} class="mt-px shrink-0" />{error}</p>{/if}
      {#if loading}
        <div class="flex items-center gap-2 text-xs text-muted-foreground"><LoaderCircle size={14} class="animate-spin text-primary" />기록을 읽는 중…</div>
      {:else if items.length === 0}
        <div class="rounded-2xl bg-card elev-1 p-6 text-sm text-muted-foreground">저장된 기록이 없습니다.</div>
      {:else}
        {#each GROUPS as group (group.kind)}
          {@const rows = items.filter(i => i.kind === group.kind)}
          {#if rows.length}
            {@const all = rows.every(i => selected.has(i.id))}
            <section class="rounded-2xl bg-card elev-1">
              <label class="flex cursor-pointer items-center gap-3 border-b px-4 py-3">
                <Checkbox checked={all} onCheckedChange={(v: boolean | "indeterminate") => toggleGroup(group.kind, v === true)} />
                <span class="min-w-0 flex-1">
                  <span class="block text-sm font-semibold">{group.title} <span class="font-normal text-muted-foreground">{rows.length}</span></span>
                  <span class="block text-[11px] text-muted-foreground">{group.note}</span>
                </span>
              </label>
              <ul class="divide-y">
                {#each rows as row (row.id)}
                  <li>
                    <label class="flex cursor-pointer items-center gap-3 px-4 py-2.5 hover:bg-muted/40">
                      <Checkbox checked={selected.has(row.id)} onCheckedChange={(v: boolean | "indeterminate") => toggle(row.id, v === true)} />
                      <span class="min-w-0 flex-1">
                        <span class="block text-[13px]">{row.title}</span>
                        <span class="block truncate font-mono text-[11px] text-muted-foreground" title={row.detail}>{row.detail}</span>
                      </span>
                      <span class="shrink-0 text-right text-[11px] text-muted-foreground">{fmtDate(row.at)}<br />{fmtBytes(row.bytes)}</span>
                    </label>
                  </li>
                {/each}
              </ul>
            </section>
          {/if}
        {/each}
      {/if}
    </div>
  </div>
  <footer class="h-14 shrink-0 border-t px-6 flex items-center justify-between gap-4">
    <Button variant="ghost" size="sm" onclick={() => (wizard.view = "device")}><ArrowLeft size={14} />처음 화면</Button>
    <div class="flex items-center gap-3">
      {#if picked.length}<span class="text-xs text-muted-foreground">{picked.length}개 선택{pickedBytes ? ` · ${fmtBytes(pickedBytes)}` : ""}</span>{/if}
      <Button size="sm" variant="destructive" disabled={!picked.length || deleting} onclick={() => (confirmOpen = true)}><Trash2 size={13} />선택 삭제</Button>
    </div>
  </footer>
</div>

{#if confirmOpen}
  <Modal title="기록 삭제 확인" onClose={() => { if (!deleting) confirmOpen = false; }} class="fixed inset-0 z-50 flex items-center justify-center bg-black/45 p-6">
    <div class="w-full max-w-md rounded-2xl border-2 border-destructive/40 bg-background elev-3 p-6 space-y-4">
      <h2 class="text-base font-semibold">기록 {picked.length}개를 지웁니다</h2>
      <ul class="max-h-48 space-y-1 overflow-y-auto text-xs text-muted-foreground">
        {#each picked as row (row.id)}<li>· {row.title} — <span class="font-mono">{row.detail}</span></li>{/each}
      </ul>
      {#if picked.some(i => i.kind === "patch")}<p class="rounded-lg bg-danger-container/60 px-3 py-2 text-xs text-destructive">VoLTE 패치 기록을 지우면 그 패치는 앱에서 되돌릴 수 없습니다.</p>{/if}
      <div class="flex justify-end gap-2">
        <Button variant="outline" size="sm" disabled={deleting} onclick={() => (confirmOpen = false)}>취소</Button>
        <Button variant="destructive" size="sm" disabled={deleting} onclick={remove}>{#if deleting}<LoaderCircle size={13} class="animate-spin" />{/if}삭제</Button>
      </div>
    </div>
  </Modal>
{/if}
