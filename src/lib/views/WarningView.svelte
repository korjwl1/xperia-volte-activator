<script lang="ts">
  import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "$lib/components/ui/card";
  import { Checkbox } from "$lib/components/ui/checkbox";
  import { Signal, TriangleAlert, OctagonX, ExternalLink, Smartphone } from "@lucide/svelte/icons";
  import { wizard } from "$lib/stores/wizard.svelte";
  import { api } from "$lib/api";
  import { omdInfo, omdCommonGuide, omdDisclaimer, type OmdRole } from "$lib/data/omd";

  const roleClass: Record<OmdRole, string> = {
    기본: "bg-primary/10 text-primary",
    대안: "bg-warning-container text-warning",
    예외: "bg-muted text-muted-foreground",
  };
</script>

<div class="flex-1 overflow-y-auto flex">
  <div class="m-auto w-full max-w-2xl p-6 space-y-4">
    <!-- 1. OMD 등록 확인 -->
    <Card class="elev-1">
      <CardHeader>
        <CardTitle class="text-base flex items-center gap-2">
          <Signal size={18} class="text-primary" />
          OMD 등록 확인
        </CardTitle>
        <CardDescription>
          사용할 통신사와 망(5G/LTE)에 따라 외산폰 VoLTE(OMD) 등록이 필요할 수 있습니다
        </CardDescription>
      </CardHeader>
      <CardContent class="space-y-3">
        <p class="text-[13px] leading-relaxed">{omdCommonGuide}</p>
        <div class="space-y-3">
          {#each omdInfo as o (o.carrier)}
            <div class="rounded-xl border p-4 space-y-3">
              <div class="flex flex-wrap items-baseline gap-x-2 gap-y-0.5">
                <span class="text-sm font-bold">{o.label}</span>
                <span class="text-[12px] text-muted-foreground">{o.summary}</span>
              </div>
              <div class="grid grid-cols-1 sm:grid-cols-2 gap-2">
                {#each o.codes as c (c.code)}
                  <div class="rounded-lg bg-muted/50 px-3 py-2 space-y-1">
                    <div class="flex items-start justify-between gap-2">
                      <span class="font-mono text-[12.5px] font-semibold break-all">{c.code}</span>
                      <span class="shrink-0 rounded px-1.5 py-0.5 text-[10px] font-medium {roleClass[c.role]}">{c.role}</span>
                    </div>
                    <div class="text-[11px] text-muted-foreground">{c.net} · {c.sim}{c.when ? ` — ${c.when}` : ""}</div>
                    {#if c.aliases?.length}
                      <div class="text-[11px] text-muted-foreground">다른 표기: <span class="font-mono">{c.aliases.join(", ")}</span></div>
                    {/if}
                    <button type="button" class="inline-flex items-center gap-1 text-[11px] text-primary hover:underline" onclick={() => api.openExternal(c.source)}>
                      <ExternalLink size={11} />출처
                    </button>
                  </div>
                {/each}
              </div>
              {#if o.simPlacement}
                <div class="flex items-start gap-1.5 text-[12px]">
                  <Smartphone size={13} class="mt-0.5 shrink-0 text-muted-foreground" />
                  <span><b class="font-medium">등록 시 SIM 위치</b> — {o.simPlacement}</span>
                </div>
              {/if}
              {#each o.notes as n, i (i)}
                <p class="text-[11px] text-muted-foreground leading-relaxed">
                  * {n.text}
                  {#if n.source}
                    <button type="button" class="ml-1 text-primary hover:underline" onclick={() => api.openExternal(n.source!)}>출처</button>
                  {/if}
                </p>
              {/each}
            </div>
          {/each}
        </div>
        <p class="text-[11px] text-muted-foreground leading-relaxed">{omdDisclaimer}</p>
        <label class="flex items-center gap-2.5 rounded-lg border px-4 py-2.5 cursor-pointer {wizard.omdAck ? 'border-primary/30 bg-primary/5' : 'border-border'}">
          <Checkbox checked={wizard.omdAck} onCheckedChange={(v: boolean | "indeterminate") => (wizard.omdAck = v === true)} />
          <span class="text-[13px] font-medium">사용할 망의 OMD 등록 조건을 확인했습니다</span>
        </label>
      </CardContent>
    </Card>

    <!-- 2. 데이터 손실 경고 -->
    <Card class="elev-1">
      <CardHeader>
        <CardTitle class="text-base flex items-center gap-2">
          <TriangleAlert size={18} class="text-warning" />
          데이터 초기화 경고
        </CardTitle>
      </CardHeader>
      <CardContent>
        <ul class="space-y-1.5 text-[13px] leading-relaxed list-disc pl-5">
          <li>부트로더 언락/리락 과정에서 핸드폰이 <b>초기화</b>되며, 내부 저장소의 모든 데이터가 삭제됩니다.</li>
          <li>공동인증서·OTP·금융 앱 인증 등 일부 데이터는 백업하더라도 복구되지 않습니다. 작업 전 각 앱에서 직접 내보내기/이전을 해 두세요.</li>
          <li>작업 전 반드시 백업을 진행해 주세요. 다음 단계에서 백업 항목과 저장 위치를 지정할 수 있습니다.</li>
        </ul>
      </CardContent>
    </Card>

    <!-- 3. 책임 고지 -->
    <Card class="elev-1 border-destructive/30">
      <CardHeader>
        <CardTitle class="text-base flex items-center gap-2">
          <OctagonX size={18} class="text-destructive" />
          책임 고지
        </CardTitle>
      </CardHeader>
      <CardContent class="space-y-3">
        <ul class="space-y-1.5 text-[13px] leading-relaxed list-disc pl-5">
          <li>이 프로그램은 완벽하지 않으며, 알려지지 않은 문제로 기기 또는 데이터가 손상될 수 있습니다.</li>
          <li>프로그램 사용 중 발생한 기기 손상, 데이터 손실 등 <b>모든 책임은 사용자에게 있습니다.</b></li>
        </ul>
        <label class="flex items-center gap-2.5 rounded-lg border px-4 py-2.5 cursor-pointer {wizard.riskAck ? 'border-primary/30 bg-primary/5' : 'border-border'}">
          <Checkbox checked={wizard.riskAck} onCheckedChange={(v: boolean | "indeterminate") => (wizard.riskAck = v === true)} />
          <span class="text-[13px] font-medium">위 내용을 이해했으며 동의합니다</span>
        </label>
      </CardContent>
    </Card>
  </div>
</div>
