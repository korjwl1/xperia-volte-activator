<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import { Checkbox } from "$lib/components/ui/checkbox";
  import { CircleCheck, Info, LoaderCircle, RefreshCw, TriangleAlert } from "@lucide/svelte/icons";
  import { CALL_ITEMS, cellularReady, type CallItem } from "$lib/domain/communication";
  import type { CallCheck, CommunicationSnapshot, SimInfo } from "$lib/types";

  let { snapshot, loading = false, error = "", slots = [], calls = [], showCalls = false, onRefresh, onCallChange }:
    { snapshot: CommunicationSnapshot | null; loading?: boolean; error?: string; slots?: (1 | 2)[]; calls?: CallCheck[]; showCalls?: boolean; onRefresh?: () => void; onCallChange?: (slot: 1 | 2, item: CallItem, checked: boolean) => void } = $props();
  const rows = $derived(slots.length ? slots.map(slot => snapshot?.sims.find(s => s.slot === slot) ?? ({ slot, type: "unknown", carrier: null, state: "UNKNOWN", volte: "unknown" } satisfies SimInfo)) : snapshot?.sims ?? []);
  const observed = $derived(snapshot?.outcome === "observed");
</script>

<div class="rounded-xl bg-card elev-1 p-4 space-y-3 text-[12px]">
  <div class="flex items-center justify-between gap-3">
    <div class="flex items-center gap-2 font-medium"><Info size={15} class="text-info shrink-0" />통신 상태 확인</div>
    {#if onRefresh}<Button variant="outline" size="sm" disabled={loading} onclick={onRefresh}>{#if loading}<LoaderCircle size={13} class="mr-1 animate-spin" />{:else}<RefreshCw size={13} class="mr-1" />{/if}통신 상태 다시 확인</Button>{/if}
  </div>
  <p class="text-muted-foreground">읽기 전용 확인입니다. 패치 기록과 실제 통화 결과는 별도로 확인합니다.</p>
  {#if error || (snapshot && !observed)}
    <div class="flex gap-2 rounded-lg bg-warning-container p-2 text-warning"><TriangleAlert size={14} class="shrink-0" />{error || (snapshot?.outcome === "query-failed" ? "기기 상태 조회 실패" : "선택한 기기 연결 안 됨")}</div>
  {/if}
  {#each rows as sim (sim.slot)}
    <div class="space-y-2 border-t pt-2">
      <div class="flex items-center justify-between gap-2">
        <span class="font-medium">SIM{sim.slot} · {sim.carrier || "통신사 미확인"}</span>
        <span class="flex items-center gap-1 rounded-md px-2 py-1 {observed && cellularReady(sim) ? 'bg-success-container text-success' : 'bg-warning-container text-warning'}">
          {#if observed && cellularReady(sim)}<CircleCheck size={13} />{:else}<TriangleAlert size={13} />{/if}
          {observed ? (cellularReady(sim) ? "VoLTE 활성" : "VoLTE 비활성") : "현재 상태 미확인"}
        </span>
      </div>
      {#if showCalls && onCallChange}
        <div class="flex flex-wrap gap-x-4 gap-y-2">
          {#each CALL_ITEMS as item (item.id)}
            <label class="flex items-center gap-1.5 cursor-pointer"><Checkbox checked={calls.find(c => c.slot === sim.slot)?.[item.id] ?? false} onCheckedChange={value => onCallChange?.(sim.slot, item.id, value === true)} /><span>{item.label}</span></label>
          {/each}
        </div>
      {/if}
    </div>
  {/each}
  {#if !snapshot}<p class="text-muted-foreground">통신 상태를 확인하면 슬롯별 등록 상태와 확인 시각을 표시합니다.</p>{/if}
  {#if showCalls}<p class="text-[11px] text-muted-foreground">선택 사항 — Wi-Fi를 잠시 끄고 대상 SIM을 골라 발신·수신과 양쪽 목소리를 직접 확인해 주세요. 재부팅·대기 후 확인은 별도 기록합니다. 문자·MMS·데이터·로밍은 별도 확인이 필요합니다.</p>{/if}
  {#if snapshot}
    <div class="border-t pt-2 text-[11px] text-muted-foreground space-y-1">
      <p>확인 시각: {new Date(snapshot.checkedAt).toLocaleString()} · {snapshot.model} · {snapshot.firmware} · Android {snapshot.android}</p>
      <p class="break-all">베이스밴드: {snapshot.baseband || "미확인"}</p>
      {#if snapshot.presets.length}<p>선택 프리셋: {snapshot.presets.map(p => `SIM${p.slot} ${p.carrier} / ${p.version}`).join(" · ")}</p>{/if}
    </div>
  {/if}
</div>
