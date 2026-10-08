<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import SelectionDeviceStatus from "$lib/components/SelectionDeviceStatus.svelte";
  import { ArrowLeft, CircleCheck, TriangleAlert, Phone } from "@lucide/svelte/icons";
  import { wizard } from "$lib/stores/wizard.svelte";
  import { cellularReady, imsLabel, imsDetail } from "$lib/domain/communication";
  let checking = $state(false);
</script>

<div class="flex-1 min-h-0 flex flex-col">
  <div class="flex-1 min-h-0 overflow-y-auto p-6 space-y-4">
    <h1 class="text-lg font-semibold flex items-center gap-2"><Phone size={20} class="text-primary" />통신 확인</h1>
    <SelectionDeviceStatus bind:checking />
    {#each wizard.device?.sims ?? [] as sim}
      <div class="rounded-xl bg-card elev-1 p-4 space-y-2 text-xs">
        <h2 class="text-sm font-semibold">SIM{sim.slot} · {sim.carrier ?? "미삽입"}</h2>
        <p class="flex items-center gap-2 {cellularReady(sim) ? 'text-success' : 'text-warning'}">{#if cellularReady(sim)}<CircleCheck size={16} />{:else}<TriangleAlert size={16} />{/if}{imsLabel(sim)}</p>
        <p class="text-muted-foreground">{imsDetail(sim)}</p>
        <label class="flex items-center gap-2"><input type="checkbox" disabled={checking || !cellularReady(sim)} />이 SIM에서 실제 발신·수신과 양쪽 음성을 확인했습니다</label>
      </div>
    {/each}
    <p class="text-xs text-muted-foreground">IMS 등록 표시와 실제 통화 성공은 별도로 확인합니다.</p>
  </div>
  <footer class="h-14 shrink-0 border-t px-6 flex items-center"><Button variant="ghost" size="sm" onclick={() => wizard.returnToTasks()}><ArrowLeft size={14} />수동 작업 목록으로</Button></footer>
</div>
