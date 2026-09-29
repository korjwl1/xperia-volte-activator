<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "$lib/components/ui/card";
  import { Badge } from "$lib/components/ui/badge";
  import { Switch } from "$lib/components/ui/switch";
  import { Alert, AlertDescription, AlertTitle } from "$lib/components/ui/alert";
  import { wizard } from "$lib/stores/wizard.svelte";
  import { PROFILE_INFO } from "$lib/mock/plan";
  import type { Profile } from "$lib/types";

  const profiles: Profile[] = ["clean-return", "keep-root", "unroot-only"];
  const wipeCount = $derived(
    wizard.steps.filter((s) => s.enabled && s.wipe).length,
  );
  const estTotal = $derived(
    wizard.steps.filter((s) => s.enabled).reduce((a, s) => a + s.estSec, 0),
  );
</script>

<div class="space-y-4">
  <Card>
    <CardHeader>
      <CardTitle class="text-base">프로파일</CardTitle>
      <CardDescription>§3-3 — 선택 후 개별 토글로 조정할 수 있습니다</CardDescription>
    </CardHeader>
    <CardContent class="grid gap-3 md:grid-cols-3">
      {#each profiles as p (p)}
        <button
          class="rounded-lg border p-3 text-left transition-colors hover:bg-accent {wizard.profile === p ? 'border-primary ring-1 ring-primary' : ''}"
          onclick={() => wizard.applyProfile(p)}
        >
          <div class="flex items-center justify-between">
            <span class="font-medium text-sm">{PROFILE_INFO[p].label}</span>
            <Badge variant={PROFILE_INFO[p].wipes === 2 ? "destructive" : "secondary"}>
              초기화 {PROFILE_INFO[p].wipes}회
            </Badge>
          </div>
          <p class="mt-1 text-xs text-muted-foreground leading-snug">{PROFILE_INFO[p].desc}</p>
        </button>
      {/each}
    </CardContent>
  </Card>

  {#if wizard.lastDepNotice}
    <Alert>
      <AlertTitle>의존성 규칙 적용됨</AlertTitle>
      <AlertDescription>{wizard.lastDepNotice}</AlertDescription>
    </Alert>
  {/if}

  <Card>
    <CardHeader>
      <div class="flex items-center justify-between">
        <div>
          <CardTitle class="text-base">작업 계획 ({wizard.steps.length}단계)</CardTitle>
          <CardDescription>
            예상 총 {Math.round(estTotal / 60)}분 · 데이터 초기화 {wipeCount}회
          </CardDescription>
        </div>
        <div class="text-xs text-muted-foreground text-right">
          파괴 단계 실행은<br />완결 백업 상태에서만 활성됩니다 (§3-3)
        </div>
      </div>
    </CardHeader>
    <CardContent class="space-y-2">
      {#each wizard.steps as step, i (step.id)}
        <div class="flex items-center gap-3 rounded-lg border px-3 py-2 {!step.enabled ? 'opacity-50' : ''}">
          <div class="w-6 text-center text-sm text-muted-foreground">{i + 1}</div>
          <div class="min-w-0 flex-1">
            <div class="flex flex-wrap items-center gap-1.5">
              <span class="text-sm font-medium">{step.title}</span>
              {#if step.wipe}
                <Badge variant="destructive">★ 초기화</Badge>
              {/if}
              {#if step.risk === "danger"}
                <Badge variant="destructive">위험</Badge>
              {:else if step.risk === "warn"}
                <Badge variant="secondary">주의</Badge>
              {/if}
              {#if step.manual}
                <Badge variant="outline">수동 개입</Badge>
              {/if}
            </div>
            <div class="text-xs text-muted-foreground truncate" title={step.desc}>{step.desc}</div>
          </div>
          <div class="text-xs text-muted-foreground w-14 text-right">≈{Math.max(1, Math.round(step.estSec / 60))}분</div>
          {#if step.optional}
            <Switch checked={step.enabled} onCheckedChange={(v: boolean) => wizard.toggleStep(step.id, v)} />
          {:else}
            <Badge variant="outline">필수</Badge>
          {/if}
        </div>
      {/each}
    </CardContent>
  </Card>
</div>
