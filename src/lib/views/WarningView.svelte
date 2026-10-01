<script lang="ts">
  import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "$lib/components/ui/card";
  import { Checkbox } from "$lib/components/ui/checkbox";
  import { Signal, TriangleAlert, OctagonX } from "@lucide/svelte/icons";
  import { wizard } from "$lib/stores/wizard.svelte";
  import { omdInfo } from "$lib/data/omd";
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
          VoLTE 패치 전에 사용할 SIM의 통신사에 외산폰 VoLTE(OMD) 등록이 되어 있어야 합니다
        </CardDescription>
      </CardHeader>
      <CardContent class="space-y-3">
        <div class="grid grid-cols-1 sm:grid-cols-3 gap-3">
          {#each omdInfo as o (o.carrier)}
            <div class="rounded-xl border p-4 space-y-2">
              <div class="text-sm font-bold">{o.label}</div>
              <div class="space-y-1">
                {#each o.codes as c (c.code)}
                  <div class="text-[11px]">
                    <span class="text-muted-foreground">{c.use}</span>
                    <div class="font-mono text-[12px] font-medium break-all">{c.code}</div>
                  </div>
                {/each}
              </div>
              <div class="text-[11px] text-muted-foreground leading-relaxed">{o.how}</div>
              {#if o.note}
                <div class="text-[11px] text-warning leading-relaxed">{o.note}</div>
              {/if}
            </div>
          {/each}
        </div>
        <label class="flex items-center gap-2.5 rounded-lg border px-4 py-2.5 cursor-pointer {wizard.omdAck ? 'border-primary/30 bg-primary/5' : 'border-border'}">
          <Checkbox checked={wizard.omdAck} onCheckedChange={(v: boolean | "indeterminate") => (wizard.omdAck = v === true)} />
          <span class="text-[13px] font-medium">사용할 SIM의 OMD 등록을 완료했습니다</span>
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
