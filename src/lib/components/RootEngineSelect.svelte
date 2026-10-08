<script lang="ts">
  import { LoaderCircle } from "@lucide/svelte/icons";
  import { api } from "$lib/api";
  import { wizard } from "$lib/stores/wizard.svelte";
  import { REAL_STEPS } from "$lib/data/runMode";
  import type { RootRelease } from "$lib/types";

  // 처음 루팅할 엔진 — Magisk(기본, PC 자동 패치) 또는 ReSukiSU(폰에서 매니저로 직접 패치, 2026-10-08 사용자 결정)
  let releases = $state<RootRelease[]>([]);
  let loading = $state(false);
  let error = $state("");
  const engine = $derived(wizard.opts.rootEngine ?? "magisk");
  // 매니저 변경이면 지금 쓰는 엔진은 고를 수 없다
  const current = $derived(wizard.opts.manualTask === "root-manager" ? wizard.device?.rootEngine ?? null : null);
  const magiskBlocked = $derived(current === "magisk" ? "지금 쓰는 엔진입니다" : "");
  // ReSukiSU 경로는 init_boot 기종만, 루팅 도구 실전 기능(실기기 검증 전)이 켜져 있을 때만 고를 수 있다
  const blocked = $derived(current === "kernelsu" ? "지금 쓰는 엔진입니다" : wizard.partition !== "init_boot"
    ? "ReSukiSU는 init_boot 기종만 준비됐습니다"
    : !REAL_STEPS.rootTools ? "ReSukiSU 루팅은 실기기 검증 전이라 꺼져 있습니다" : "");

  async function choose(next: "magisk" | "resukisu") {
    if (next === "resukisu" && blocked) return;
    if (next === "magisk" && magiskBlocked) return;
    wizard.opts = { ...wizard.opts, rootEngine: next, resukisuTag: next === "resukisu" ? wizard.opts.resukisuTag : undefined };
    if (next === "resukisu" && releases.length === 0 && !loading) { requested = true; await load(); }
  }

  // 매니저 변경처럼 ReSukiSU가 미리 골라진 채로 열리면 버전 목록을 바로 불러온다(누를 때만 부르던 문제, 2026-10-08)
  let requested = false;
  $effect(() => {
    if (engine === "resukisu" && !blocked && !requested) { requested = true; void load(); }
  });

  async function load() {
    loading = true;
    error = "";
    const result = await api.resukisuReleases();
    loading = false;
    if (!result.ok) { error = `버전 목록을 받지 못했습니다: ${result.error}`; return; }
    releases = result.value;
    // 기본은 가장 최근 릴리스 — 사용자가 바꿀 수 있다
    if (!wizard.opts.resukisuTag && releases[0]) wizard.opts = { ...wizard.opts, resukisuTag: releases[0].tag };
  }
</script>

<div class="space-y-2">
  <h2 class="text-sm font-semibold">루팅 엔진</h2>
  <div class="grid grid-cols-2 gap-2">
    <button type="button" onclick={() => choose("magisk")} disabled={!!magiskBlocked}
      class="rounded-lg border px-4 py-3 text-left transition-colors disabled:opacity-50 {engine === 'magisk' ? 'border-primary bg-primary/5' : 'border-border hover:bg-muted'}">
      <p class="text-sm font-medium">Magisk</p>
      <p class="mt-0.5 text-[11px] text-muted-foreground">PC가 자동으로 패치 · 실기기 검증 완료</p>
    </button>
    <button type="button" onclick={() => choose("resukisu")} disabled={!!blocked}
      class="rounded-lg border px-4 py-3 text-left transition-colors disabled:opacity-50 {engine === 'resukisu' ? 'border-primary bg-primary/5' : 'border-border hover:bg-muted'}">
      <p class="text-sm font-medium">ReSukiSU</p>
      <p class="mt-0.5 text-[11px] text-muted-foreground">폰의 매니저 앱에서 직접 패치 1회 · 실기기 미검증</p>
    </button>
  </div>
  {#if blocked || magiskBlocked}<p class="text-[11px] text-muted-foreground">{magiskBlocked ? `Magisk: ${magiskBlocked}` : `ReSukiSU: ${blocked}`}</p>{/if}
  {#if engine === "resukisu"}
    <div class="flex items-center gap-2 text-xs">
      <span class="shrink-0">버전</span>
      {#if loading}
        <LoaderCircle size={13} class="animate-spin" />
      {:else}
        <select aria-label="ReSukiSU 버전" class="min-w-0 flex-1 rounded border bg-background p-2"
          value={wizard.opts.resukisuTag ?? ""}
          onchange={(e) => (wizard.opts = { ...wizard.opts, resukisuTag: (e.currentTarget as HTMLSelectElement).value || undefined })}>
          <option value="">버전 선택</option>
          {#each releases as r (r.tag)}<option value={r.tag}>{r.tag}{r.prerelease ? " (시험판)" : ""}</option>{/each}
        </select>
      {/if}
    </div>
    {#if error}<p role="alert" class="text-[11px] text-destructive">{error} <button class="underline" onclick={load}>다시 시도</button></p>{/if}
    <p class="text-[11px] text-muted-foreground">
      실행 중에 폰 화면을 보고 있어야 합니다 — 매니저 설치·순정 이미지 전송 뒤 ReSukiSU 앱에서 [설치] → [파일 선택 후 패치]를 한 번 눌러 주세요.
      VoLTE 적용 전에는 ReSukiSU 앱 → 슈퍼유저에서 Shell 루트 권한을 직접 켜야 합니다.
    </p>
  {/if}
</div>
