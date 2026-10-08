<script lang="ts">
  import { Checkbox } from "$lib/components/ui/checkbox";
  import { CornerDownRight } from "@lucide/svelte/icons";
  import { MODULE_SETS, MODULE_PRESET, selectedModuleSets, toggleModuleSet } from "$lib/domain/moduleSets";
  import type { RootModuleSelection, RootState } from "$lib/types";
  let { selection, onChange, disabled = false, reason = null, engine = "unknown" }: {
    selection: RootModuleSelection; onChange: (selection: RootModuleSelection) => void;
    disabled?: boolean; reason?: string | null; engine?: RootState["engine"] | "resukisu" | "magisk";
  } = $props();
  const sets = $derived(selectedModuleSets(selection));
  // KernelSU 계열(ReSukiSU)만 OverlayFS를 먼저 설치한다 — 엔진을 모르면 조건부로 표시
  const kernelsu = $derived(engine === "kernelsu-family" || engine === "resukisu");
  const knownEngine = $derived(engine !== "unknown");
</script>

<div class="space-y-2">
  {#if reason}<p class="text-xs text-muted-foreground">{reason}</p>{/if}
  {#each MODULE_SETS as set (set.id)}
    {@const checked = sets.includes(set.id)}
    {@const required = MODULE_SETS.some(other => sets.includes(other.id) && other.depends.some(dependency => dependency === set.id))}
    <div class="rounded-xl border transition-colors {checked ? 'border-primary/30 bg-primary/5' : 'border-border bg-card'}">
      <label class="flex items-center gap-3 px-4 py-3 {disabled || required ? 'cursor-not-allowed' : 'cursor-pointer'}">
        <Checkbox {checked} disabled={disabled || required} onCheckedChange={(v: boolean | "indeterminate") => onChange(toggleModuleSet(selection, set.id, v === true))} />
        <span class="min-w-0 flex-1">
          <span class="block text-[13px] font-medium">{set.name}</span>
          <span class="block text-[11px] text-muted-foreground">{set.detail}</span>
        </span>
        {#if required}<span class="rounded-full bg-muted px-2 py-0.5 text-[10px] text-muted-foreground">Set B에 포함</span>{/if}
      </label>
      {#if checked}
        <!-- 하위 구성 — 엔진별로 고정, 위에서부터 설치 순서 -->
        <ul class="mx-4 mb-3 space-y-1 border-l-2 border-primary/20 pl-3">
          {#each MODULE_PRESET[set.id].filter(m => !m.kernelsuOnly || kernelsu || !knownEngine) as module (module.id)}
            <li class="flex items-center gap-2 text-[12px]">
              <CornerDownRight size={12} class="shrink-0 text-muted-foreground" />
              <span class="font-medium">{module.name}</span>
              <span class="text-muted-foreground">· {module.role}{module.kernelsuOnly && !knownEngine ? " (ReSukiSU일 때만)" : ""}</span>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  {/each}
  <p class="text-[11px] leading-relaxed text-muted-foreground">모듈은 하나씩 설치하고 재부팅해 적용을 확인합니다. 설치 전에 매니저 설정, 중간에 PlayIntegrityFork·TrickyAddon·HMA 설정을 폰에서 안내합니다. 금융 앱 동작이나 Integrity 통과를 보장하지는 않습니다.</p>
</div>
