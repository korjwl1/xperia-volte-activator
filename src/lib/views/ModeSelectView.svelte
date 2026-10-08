<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import SelectionDeviceStatus from "$lib/components/SelectionDeviceStatus.svelte";
  import { ArrowLeft, Zap, LayoutGrid, Download, TriangleAlert } from "@lucide/svelte/icons";
  import { wizard } from "$lib/stores/wizard.svelte";
  import { updateProblem } from "$lib/domain/workflow";
  let checking = $state(false);
  let deviceStatus: SelectionDeviceStatus;
  const updateReason = $derived(updateProblem(wizard.device));
  const modes = [
    { id: "automatic" as const, title: "1. 자동 진행", detail: "백업부터 VoLTE 패치와 선택한 후처리까지 기존 절차로 진행합니다.", icon: Zap },
    { id: "manual" as const, title: "2. 수동 진행", detail: "백업·복구·언락·리락·루팅·언루팅·VoLTE 패치 중 필요한 작업을 선택합니다.", icon: LayoutGrid },
    { id: "update" as const, title: "3. 업데이트", detail: "현재 VoLTE를 유지하는 펌웨어 업데이트를 준비합니다.", icon: Download },
  ];
</script>

<div class="flex-1 min-h-0 flex flex-col">
  <div class="flex-1 min-h-0 overflow-y-auto p-6 space-y-5">
    <div><h1 class="text-xl font-semibold">진행 방법 선택</h1><p class="mt-1 text-xs text-muted-foreground">연결된 기기에서 진행할 작업을 선택하세요.</p></div>
    <SelectionDeviceStatus bind:checking bind:this={deviceStatus} />
    <div class="grid grid-cols-3 gap-4" aria-label="진행 방법">
      {#each modes as mode}
        {@const reason = mode.id === "update" ? updateReason : !wizard.device ? "선택한 기기를 연결하세요" : null}
        <button type="button" class="min-w-0 rounded-2xl bg-card elev-1 p-5 text-left flex flex-col items-start gap-4 hover:bg-accent disabled:opacity-50 disabled:cursor-not-allowed" disabled={checking || !!reason} onclick={async () => { if (await deviceStatus.refresh()) wizard.chooseMode(mode.id); }}>
          <span class="inline-flex rounded-xl bg-primary/10 p-3 text-primary"><mode.icon size={26} /></span>
          <span class="block text-base font-semibold">{mode.title}</span>
          <span class="block text-xs leading-relaxed text-muted-foreground">{mode.detail}</span>
          {#if reason}<span class="flex items-start gap-1 text-xs text-warning"><TriangleAlert size={13} class="shrink-0" />{reason}</span>{/if}
        </button>
      {/each}
    </div>
    <p class="text-xs text-muted-foreground">자동 진행은 백업 완료 뒤 한 번 확인을 받고, 나머지는 필요한 폰 조작·오류 대기를 제외하고 이어서 진행합니다.</p>
  </div>
  <footer class="h-14 shrink-0 border-t px-6 flex items-center"><Button variant="ghost" size="sm" onclick={() => wizard.view = "device"}><ArrowLeft size={14} />기기 화면으로</Button></footer>
</div>
