<script lang="ts">
  import SelectionDeviceStatus from "$lib/components/SelectionDeviceStatus.svelte";
  import { wizard } from "$lib/stores/wizard.svelte";
  import { TriangleAlert, Download, LoaderCircle } from "@lucide/svelte/icons";
  import { updateProblem } from "$lib/domain/workflow";
  wizard.ensureFirmwareVersions();
  let checking = $state(true);
  const reason = $derived(updateProblem(wizard.device));
  const versions = $derived([...new Map((wizard.fwVersions?.versions ?? []).filter(v => v.version !== wizard.device?.firmware).map(v => [v.version, v])).values()]);
</script>

<div class="flex-1 min-h-0 overflow-y-auto p-6 space-y-4">
  <h1 class="text-lg font-semibold flex items-center gap-2"><Download size={20} class="text-primary" />펌웨어 업데이트</h1>
  <SelectionDeviceStatus bind:checking />
  {#if reason}<p role="alert" class="rounded-xl bg-warning-container p-3 text-xs text-warning">{reason}</p>{/if}
  <div class="rounded-xl bg-card elev-1 p-4 space-y-2 text-xs leading-relaxed">
    <p>현재 기기·지역의 새 버전을 준비하고, 기존 VoLTE 설정과 부트로더 잠금 상태를 유지하는 경로입니다.</p>
    <p>모뎀·DSP·TA·사용자 데이터를 제외하고 기록하는 정책입니다. 데이터 유지나 VoLTE 작동을 보장하지 않으며 Android 판올림도 기기별 확인이 필요합니다.</p>
    <p>다음 화면에서 자동 진행과 같은 백업 항목을 선택할 수 있습니다. 백업을 선택하면 완료 뒤 직접 다음 과정을 시작합니다.</p>
    {#if wizard.device?.rooted === true}<p class="text-warning">루팅된 기기는 새 버전 이미지의 패치·후속 기록이 필요합니다. 기존 패치 이미지를 재사용하지 않습니다. 현재 루팅 유지 실행은 준비 중입니다.</p>{/if}
    <label class="flex items-center gap-2 pt-2"><input type="checkbox" bind:checked={wizard.riskAck} />손실 가능성과 백업 안내를 확인했습니다</label>
  </div>
  <div class="rounded-xl bg-card elev-1 p-4 space-y-2">
    <h2 class="text-sm font-semibold">목표 펌웨어</h2>
    <p class="text-xs text-muted-foreground">현재 {wizard.device?.firmware}</p>
    {#each versions as version}<button type="button" class="w-full rounded-lg p-3 text-left text-xs {wizard.volteConfig.firmware === version.version ? 'bg-primary/10 text-primary' : 'bg-muted hover:bg-accent'}" disabled={checking || !!reason} onclick={() => wizard.volteConfig.firmware = version.version}>{version.version} · Android {version.android}</button>{/each}
    {#if wizard.fwVersionsState === "loading"}<p class="text-xs flex items-center gap-2"><LoaderCircle size={13} class="animate-spin" />새 버전 확인 중…</p>{:else if versions.length === 0}<p class="text-xs text-muted-foreground">선택할 새 버전을 확인하지 못했습니다.</p>{/if}
  </div>
  <p class="rounded-xl bg-warning-container p-3 text-xs text-warning flex items-start gap-2"><TriangleAlert size={15} class="shrink-0" />전체 펌웨어의 Newflasher 기록 연결과 기기별 검증을 준비 중입니다. 업데이트 실행은 아직 사용할 수 없습니다.</p>
</div>
