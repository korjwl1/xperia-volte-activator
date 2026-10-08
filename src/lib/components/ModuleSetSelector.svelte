<script lang="ts">
  import OptionCard from "$lib/components/OptionCard.svelte";
  import { MODULE_SETS, selectedModuleSets, toggleModuleSet } from "$lib/domain/moduleSets";
  import type { RootModuleSelection, RootState } from "$lib/types";
  let { selection, onChange, disabled = false, reason = null, engine = "unknown" }: {
    selection: RootModuleSelection; onChange: (selection: RootModuleSelection) => void;
    disabled?: boolean; reason?: string | null; engine?: RootState["engine"];
  } = $props();
  const sets = $derived(selectedModuleSets(selection));
  $effect(() => {
    if (engine === "kernelsu-family" && selection.extras.includes("shamiko")) onChange({ ...selection, extras: selection.extras.filter(id => id !== "shamiko"), settingsAck: false });
  });
  function extra(id: RootModuleSelection["extras"][number], checked: boolean) {
    onChange({ ...selection, extras: checked ? [...selection.extras.filter(value => value !== id), id] : selection.extras.filter(value => value !== id), settingsAck: false });
  }
</script>

<div class="space-y-3">
  {#if reason}<p class="text-xs text-muted-foreground">{reason}</p>{/if}
  {#each MODULE_SETS as set}
    {@const required = MODULE_SETS.some(other => sets.includes(other.id) && other.depends.some(dependency => dependency === set.id))}
    <OptionCard checked={sets.includes(set.id)} label={set.name} desc={set.detail} disabled={disabled || required} badge={required ? "의존 세트 · 자동 선택" : undefined} onToggle={checked => onChange(toggleModuleSet(selection, set.id, checked))} />
  {/each}
  <p class="text-[11px] text-muted-foreground">Set B는 Set A가 필요합니다. B를 선택하면 A도 함께 선택됩니다.</p>
  <div class="grid grid-cols-2 gap-3 text-xs">
    <label>Zygisk 구현체
      <select aria-label="Zygisk 구현체" class="mt-1 block w-full rounded-lg border bg-background p-2" value={selection.zygisk} disabled={disabled || !sets.includes("foundation")} onchange={event => onChange({ ...selection, zygisk: event.currentTarget.value as RootModuleSelection["zygisk"], extras: selection.extras.filter(id => id !== "shamiko"), settingsAck: false })}>
        <option value="neozygisk">NeoZygisk · 기본</option><option value="rezygisk">ReZygisk</option><option value="zygisk-next">Zygisk Next</option>
      </select>
    </label>
    <label>Integrity 모듈
      <select aria-label="Integrity 모듈" class="mt-1 block w-full rounded-lg border bg-background p-2" value={selection.integrity} disabled={disabled || !sets.includes("evasion")} onchange={event => onChange({ ...selection, integrity: event.currentTarget.value as RootModuleSelection["integrity"], settingsAck: false })}>
        <option value="play-integrity-fork">PlayIntegrityFork · 기본</option><option value="integrity-box">Integrity Box · 대안</option>
      </select>
    </label>
  </div>
  <div class="space-y-1.5">
    <OptionCard checked={selection.extras.includes("play-store-fix")} label="PlayStoreFix 추가" desc="Set A 선택 항목 · 카페 배포본" disabled={disabled || !sets.includes("foundation")} onToggle={checked => extra("play-store-fix", checked)} />
    <OptionCard checked={selection.extras.includes("zygisk-assistant")} label="Zygisk Assistant 추가" desc="Set B 선택 항목" disabled={disabled || !sets.includes("evasion")} onToggle={checked => extra("zygisk-assistant", checked)} />
    <OptionCard checked={selection.extras.includes("shamiko")} label="Shamiko 추가" desc="Magisk + Zygisk Next에서만 사용" disabled={disabled || !sets.includes("evasion") || selection.zygisk !== "zygisk-next" || engine === "kernelsu-family"} onToggle={checked => extra("shamiko", checked)} />
  </div>
  <p class="text-[11px] leading-relaxed text-muted-foreground">기초 세트에는 부트루프 보호를 포함합니다. KernelSU 계열은 OverlayFS를 먼저 설치합니다. 감지 회피 세트는 TrickyStore·TrickyAddon·HMA를 포함하며, 금융앱 동작이나 Integrity 통과는 보장하지 않습니다. WebUI·MMRL은 별도 앱 설치 안내를 제공합니다.</p>
  <label class="flex gap-2 items-start text-xs"><input type="checkbox" checked={selection.settingsAck} disabled={disabled || sets.length === 0} onchange={event => onChange({ ...selection, settingsAck: event.currentTarget.checked })} />매니저 설정을 확인했습니다 — Magisk는 내장 Zygisk·DenyList 강제 적용 OFF, ReSukiSU는 모듈 마운트 해제 기본값·Hide SELinux Modification ON</label>
</div>
