<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import SelectionDeviceStatus from "$lib/components/SelectionDeviceStatus.svelte";
  import { ArrowLeft, Zap, LayoutGrid, Download, TriangleAlert } from "@lucide/svelte/icons";
  import { wizard } from "$lib/stores/wizard.svelte";
  let checking = $state(false);
  let deviceStatus: SelectionDeviceStatus;
  const modes = [
    { id: "automatic" as const, title: "자동 진행", detail: "백업부터 VoLTE 패치와 선택한 후처리까지 이어서 진행합니다. 백업이 끝나면 한 번 확인을 받고, 이후에는 폰 조작이 필요하거나 오류가 날 때만 멈춥니다.", icon: Zap },
    { id: "manual" as const, title: "수동 진행", detail: "백업·복원·언락·리락·루팅·언루팅·VoLTE 패치 중 필요한 작업을 선택합니다.", icon: LayoutGrid },
    { id: "update" as const, title: "업데이트", detail: "현재 VoLTE를 유지하는 펌웨어 업데이트를 준비합니다.", icon: Download },
  ];
</script>

<div class="flex-1 min-h-0 flex flex-col">
  <div class="flex-1 min-h-0 overflow-y-auto p-6 flex flex-col gap-5">
    <div><h1 class="text-xl font-semibold">진행 방법 선택</h1><p class="mt-1 text-xs text-muted-foreground">연결된 기기에서 진행할 작업을 선택하세요.</p></div>
    <SelectionDeviceStatus bind:checking bind:this={deviceStatus} />
    <!-- 남는 세로 공간을 카드가 채운다(창이 작으면 최소 높이 뒤 스크롤) -->
    <div class="flex-1 min-h-[20rem] grid grid-cols-3 gap-5" aria-label="진행 방법">
      {#each modes as mode}
        {@const reason = mode.id === "update" ? "펌웨어 업데이트는 아직 준비 중입니다" : !wizard.device ? "선택한 기기를 연결하세요" : null}
        <!-- 세로로 긴 선택 카드: 아이콘·제목·설명을 가로·세로 가운데 정렬 -->
        <button type="button" class="group h-full min-w-0 rounded-3xl bg-card elev-1 px-10 py-12 flex flex-col items-center justify-center text-center transition duration-200 ease-out hover:-translate-y-1 hover:elev-3 hover:bg-primary/[0.04] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary disabled:opacity-50 disabled:cursor-not-allowed disabled:hover:translate-y-0 disabled:hover:elev-1 disabled:hover:bg-card" disabled={checking || !!reason} onclick={async () => { if (await deviceStatus.ensure()) wizard.chooseMode(mode.id); }}>
          <span class="inline-flex size-28 items-center justify-center rounded-[2rem] bg-primary/10 text-primary transition-transform duration-200 group-hover:scale-105"><mode.icon size={52} strokeWidth={1.6} /></span>
          <span class="mt-10 block text-xl font-semibold">{mode.title}</span>
          <span class="mt-3 h-0.5 w-10 rounded-full bg-primary/30"></span>
          <span class="mt-6 block w-full max-w-md break-keep text-sm leading-7 text-muted-foreground">{mode.detail}</span>
          {#if reason}<span class="mt-6 flex items-start gap-1 break-keep text-xs text-warning"><TriangleAlert size={13} class="shrink-0" />{reason}</span>{/if}
        </button>
      {/each}
    </div>
  </div>
  <footer class="h-14 shrink-0 border-t px-6 flex items-center"><Button variant="ghost" size="sm" onclick={() => wizard.view = "device"}><ArrowLeft size={14} />기기 화면으로</Button></footer>
</div>
