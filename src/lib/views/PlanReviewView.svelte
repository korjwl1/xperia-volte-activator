<script lang="ts">
  import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "$lib/components/ui/card";
  import { Badge } from "$lib/components/ui/badge";
  import { Switch } from "$lib/components/ui/switch";
  import { Alert, AlertDescription, AlertTitle } from "$lib/components/ui/alert";
  import { TriangleAlert, OctagonX, Hand } from "@lucide/svelte/icons";
  import { wizard } from "$lib/stores/wizard.svelte";
  import type { PlanStep } from "$lib/types";

  const wipeCount = $derived(wizard.steps.filter((s) => s.enabled && s.wipe).length);
  const estTotal = $derived(wizard.steps.filter((s) => s.enabled).reduce((a, s) => a + s.estSec, 0));
  const carrierLabel = $derived(wizard.volteConfig.carrier === "LGU_V" ? "LG U+" : wizard.volteConfig.carrier);

  const dotStyle = (s: PlanStep) =>
    s.wipe || s.risk === "danger" ? "bg-destructive" : s.risk === "warn" ? "bg-warning" : "bg-primary";
</script>

<div class="space-y-4">
  {#if wizard.lastDepNotice}
    <Alert class="border-warning/40 bg-warning-container/60">
      <TriangleAlert size={16} class="text-warning" />
      <AlertTitle>자동 조정됨</AlertTitle>
      <AlertDescription>{wizard.lastDepNotice}</AlertDescription>
    </Alert>
  {/if}

  <Card class="elev-1">
    <CardHeader class="pb-3">
      <div class="flex items-center justify-between">
        <div>
          <CardTitle class="text-sm">작업 내역</CardTitle>
          <CardDescription class="text-xs">
            {carrierLabel} · SIM{wizard.volteConfig.simSlot} · {wizard.volteConfig.mode === "balance" ? "균형" : "실내 우선"} 모드
          </CardDescription>
        </div>
        <div class="text-[11px] text-muted-foreground text-right leading-tight">
          {wizard.steps.length}단계 · 약 {Math.round(estTotal / 60)}분<br />
          {#if wipeCount > 0}<span class="text-destructive font-medium">초기화 {wipeCount}회</span>{/if}
        </div>
      </div>
    </CardHeader>
    <CardContent>
      <div class="relative pl-1">
        <div class="absolute left-[13px] top-3 bottom-3 w-px bg-border" aria-hidden="true"></div>
        {#each wizard.steps as step, i (step.id)}
          <div class="relative flex items-start gap-3 py-1.5 {!step.enabled ? 'opacity-45' : ''}">
            <span class="relative z-10 mt-0.5 flex h-[22px] w-[22px] shrink-0 items-center justify-center rounded-full border-2 border-card {dotStyle(step)}">
              <span class="text-[10px] font-bold text-white">{i + 1}</span>
            </span>
            <div class="flex-1 min-w-0 rounded-lg border bg-background/60 px-3 py-2">
              <div class="flex flex-wrap items-center gap-1.5">
                <span class="text-[13px] font-medium">{step.title}</span>
                {#if step.wipe}
                  <span class="inline-flex items-center gap-1 rounded-full bg-danger-container px-2 py-0.5 text-[10px] font-medium text-destructive">
                    <OctagonX size={10} />초기화
                  </span>
                {/if}
                {#if step.risk === "danger"}
                  <Badge variant="destructive" class="text-[10px] px-1.5">위험</Badge>
                {:else if step.risk === "warn"}
                  <span class="rounded-full bg-warning-container px-2 py-0.5 text-[10px] font-medium text-warning">주의</span>
                {/if}
                {#if step.manual}
                  <span class="inline-flex items-center gap-1 rounded-full bg-info-container px-2 py-0.5 text-[10px] font-medium text-info">
                    <Hand size={10} />수동
                  </span>
                {/if}
              </div>
              <div class="text-[11px] text-muted-foreground leading-snug mt-0.5">{step.desc}</div>
            </div>
            <div class="flex shrink-0 items-center gap-2 self-center">
              <span class="text-[11px] text-muted-foreground w-12 text-right">≈{Math.max(1, Math.round(step.estSec / 60))}분</span>
              {#if step.optional}
                <Switch checked={step.enabled} onCheckedChange={(v: boolean) => wizard.toggleStep(step.id, v)} />
              {:else}
                <Badge variant="outline" class="text-[10px] px-1.5">필수</Badge>
              {/if}
            </div>
          </div>
        {/each}
      </div>
    </CardContent>
  </Card>
</div>
