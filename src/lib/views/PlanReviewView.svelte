<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "$lib/components/ui/card";
  import { Badge } from "$lib/components/ui/badge";
  import { Switch } from "$lib/components/ui/switch";
  import { Alert, AlertDescription, AlertTitle } from "$lib/components/ui/alert";
  import {
    ShieldCheck, Cpu, RotateCcw, TriangleAlert, OctagonX, Hand, CircleCheck,
  } from "@lucide/svelte/icons";
  import { wizard } from "$lib/stores/wizard.svelte";
  import { PROFILE_INFO } from "$lib/mock/plan";
  import type { Profile, PlanStep } from "$lib/types";

  const profiles: { id: Profile; icon: typeof ShieldCheck }[] = [
    { id: "clean-return", icon: ShieldCheck },
    { id: "keep-root", icon: Cpu },
    { id: "unroot-only", icon: RotateCcw },
  ];

  const wipeCount = $derived(wizard.steps.filter((s) => s.enabled && s.wipe).length);
  const estTotal = $derived(wizard.steps.filter((s) => s.enabled).reduce((a, s) => a + s.estSec, 0));

  const dotStyle = (s: PlanStep) =>
    s.wipe || s.risk === "danger" ? "bg-destructive" : s.risk === "warn" ? "bg-warning" : "bg-primary";
</script>

<div class="space-y-4">
  <div class="grid gap-3 md:grid-cols-3">
    {#each profiles as p (p.id)}
      <button
        class="rounded-xl border-2 bg-card p-4 text-left transition-all elev-1
          {wizard.profile === p.id ? 'border-primary ring-2 ring-primary/25' : 'border-transparent hover:border-border'}"
        onclick={() => wizard.applyProfile(p.id)}
      >
        <div class="flex items-center justify-between">
          <span class="flex h-9 w-9 items-center justify-center rounded-lg {wizard.profile === p.id ? 'bg-primary text-primary-foreground' : 'bg-secondary text-secondary-foreground'}">
            <p.icon size={18} />
          </span>
          {#if wizard.profile === p.id}
            <CircleCheck size={16} class="text-primary" />
          {/if}
        </div>
        <div class="mt-3 font-medium text-[13px]">{PROFILE_INFO[p.id].label}</div>
        <div class="mt-0.5 text-[11px] text-muted-foreground leading-snug">{PROFILE_INFO[p.id].desc}</div>
        <Badge variant={PROFILE_INFO[p.id].wipes === 2 ? "destructive" : "secondary"} class="mt-2 text-[10px]">
          <TriangleAlert size={11} class="mr-1" />초기화 {PROFILE_INFO[p.id].wipes}회
        </Badge>
      </button>
    {/each}
  </div>

  {#if wizard.lastDepNotice}
    <Alert class="border-warning/40 bg-warning-container/60">
      <TriangleAlert size={16} class="text-warning" />
      <AlertTitle>의존성 규칙 적용됨</AlertTitle>
      <AlertDescription>{wizard.lastDepNotice}</AlertDescription>
    </Alert>
  {/if}

  <Card class="elev-1">
    <CardHeader class="pb-3">
      <div class="flex items-center justify-between">
        <div>
          <CardTitle class="text-sm">작업 계획 <span class="text-muted-foreground font-normal">{wizard.steps.length}단계</span></CardTitle>
          <CardDescription class="text-xs">예상 {Math.round(estTotal / 60)}분 · 데이터 초기화 {wipeCount}회</CardDescription>
        </div>
        <div class="text-[11px] text-muted-foreground text-right leading-tight">
          파괴 단계는 완결 백업 상태에서만<br />활성화됩니다 (§3-3)
        </div>
      </div>
    </CardHeader>
    <CardContent>
      <div class="relative pl-1">
        <!-- 타임라인 연결선 -->
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
