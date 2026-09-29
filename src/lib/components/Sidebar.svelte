<script lang="ts">
  import { Badge } from "$lib/components/ui/badge";
  import { CircleCheck, LoaderCircle, ChevronUp, ChevronDown } from "@lucide/svelte/icons";
  import { wizard, MACRO_STEPS } from "$lib/stores/wizard.svelte";

  // 서브스텝 가시 창: 실행 중 기준 3개
  const subWindow = $derived.by(() => {
    if (wizard.view !== "step3" || wizard.runSteps.length === 0) return null;
    const steps = wizard.runSteps;
    const activeIdx = steps.findIndex((s) => s.status === "running" || s.status === "manual-wait");
    const doneCount = steps.filter((s) => s.status === "done").length;
    // 실행 중이 없고 전부 완료면 마지막 3개
    const focus = activeIdx >= 0 ? activeIdx : Math.max(0, doneCount - 1);
    const start = Math.max(0, Math.min(focus - 1, steps.length - 3));
    return {
      items: steps.slice(start, start + 3),
      hasBefore: start > 0,
      hasAfter: start + 3 < steps.length,
      activeIdx,
    };
  });
</script>

<aside class="w-60 shrink-0 border-r bg-muted/40 flex flex-col overflow-hidden">
  <nav class="flex-1 p-2 space-y-0.5 overflow-y-auto">
    {#each MACRO_STEPS as step, i (step.id)}
      <div>
        <button
          class="w-full flex items-center gap-2.5 rounded-md px-2 py-1.5 text-left text-[13px] transition-colors
            {step.view === wizard.view ? 'bg-primary text-primary-foreground font-medium' : 'opacity-50 cursor-default'}"
          disabled={true}
        >
          <span class="w-[18px] h-[18px] shrink-0 rounded-full border flex items-center justify-center text-[10px]
            {step.view === wizard.view ? 'border-current' : 'border-muted-foreground/40'}">
            {#if wizard.macroStepIdx > i}
              <CircleCheck size={11} class="text-success" />
            {:else}
              {step.id}
            {/if}
          </span>
          {step.label}
        </button>

        <!-- 3단계 서브스텝 -->
        {#if step.id === 3 && subWindow}
          <div class="ml-8 mt-1 space-y-0.5 border-l border-border pl-3">
            {#if subWindow.hasBefore}
              <div class="flex items-center gap-1 text-[10px] text-muted-foreground/50 py-0.5">
                <ChevronUp size={10} />…
              </div>
            {/if}
            {#each subWindow.items as sub (sub.id)}
              <div class="flex items-center gap-1.5 text-[11px] py-0.5 {sub.status === 'running' || sub.status === 'manual-wait' ? 'text-foreground font-medium' : sub.status === 'done' ? 'text-muted-foreground' : 'text-muted-foreground/50'}">
                {#if sub.status === "done"}
                  <CircleCheck size={11} class="text-success shrink-0" />
                {:else if sub.status === "running" || sub.status === "manual-wait"}
                  <LoaderCircle size={11} class="text-primary shrink-0 animate-spin" />
                {:else}
                  <span class="w-[11px] shrink-0"></span>
                {/if}
                <span class="truncate">{sub.title}</span>
              </div>
            {/each}
            {#if subWindow.hasAfter}
              <div class="flex items-center gap-1 text-[10px] text-muted-foreground/50 py-0.5">
                <ChevronDown size={10} />…
              </div>
            {/if}
          </div>
        {/if}
      </div>
    {/each}
  </nav>

  <!-- 디바이스 요약 -->
  {#if wizard.device}
    <div class="shrink-0 border-t p-3 space-y-1 text-[11px] leading-snug">
      <div class="font-medium text-[12px] text-foreground">{wizard.device.productName}</div>
      <div class="font-mono text-muted-foreground">{wizard.device.model} · {wizard.device.serialMasked}</div>
      <div class="text-muted-foreground">{wizard.device.firmware} · Android {wizard.device.android}</div>
      <div class="pt-1 flex flex-wrap gap-1">
        <Badge variant="outline" class="text-[10px] px-1.5 py-0">
          {wizard.device.bootloader === "locked" ? "🔒 잠김" : wizard.device.bootloader === "unlocked" ? "🔓 언락" : "?"}
        </Badge>
        <Badge variant="outline" class="text-[10px] px-1.5 py-0">
          {wizard.device.sims.some((s) => s.volteEnabled) ? "VoLTE ✓" : "VoLTE ✗"}
        </Badge>
      </div>
    </div>
  {/if}
</aside>
