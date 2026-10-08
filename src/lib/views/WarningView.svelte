<script lang="ts">
  import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "$lib/components/ui/card";
  import { Checkbox } from "$lib/components/ui/checkbox";
  import { Signal, TriangleAlert, OctagonX, ExternalLink, Smartphone, HardDrive } from "@lucide/svelte/icons";
  import { Button } from "$lib/components/ui/button";
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
  <div class="m-auto w-full max-w-5xl p-6 space-y-4">
    {#if wizard.mode !== "manual" || wizard.manualTask === "volte"}
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
        <div class="@container space-y-3">
          {#each omdInfo as o (o.carrier)}
            <div class="rounded-xl border p-4 space-y-3">
              <div class="flex flex-wrap items-baseline gap-x-2 gap-y-0.5">
                <span class="text-sm font-bold">{o.label}</span>
                <span class="text-[12px] text-muted-foreground">{o.summary}</span>
              </div>
              <!-- 카드 폭 기준: 넓으면 한 줄에 3개(SKT 5G 코드 3개가 같은 줄) -->
              <div class="grid grid-cols-1 @md:grid-cols-2 @3xl:grid-cols-3 gap-2">
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

    {/if}
    <!-- 2. 데이터 손실 경고 -->
    <Card class="elev-1">
      <CardHeader>
        <CardTitle class="text-base flex items-center gap-2">
          <TriangleAlert size={18} class="text-warning" />
          {wizard.mode === "manual" ? `${wizard.taskTitle} 전 확인` : wizard.device?.bootloader === "unlocked" ? "진행 전 확인" : "데이터 초기화 경고"}
        </CardTitle>
      </CardHeader>
      <CardContent>
        <ul class="space-y-1.5 text-[13px] leading-relaxed list-disc pl-5">
          <!-- 작업별로 해당하는 안내만 (2026-10-08 사용자 요청) -->
          {#if wizard.mode === "manual" && wizard.manualTask === "backup"}
            <li>폰의 데이터를 PC로 복사합니다. 폰의 데이터는 바뀌지 않습니다.</li>
            <li>공동인증서·OTP·금융 앱 인증 등 일부 데이터는 백업하더라도 복원되지 않습니다. 초기화 전에 각 앱에서 직접 내보내기/이전을 해 두세요.</li>
            <li>다음 화면에서 백업 항목과 저장 위치를 지정합니다.</li>
          {:else if wizard.mode === "manual" && wizard.manualTask === "restore"}
            <li>선택한 백업의 데이터를 현재 기기에 복원합니다. 기존 파일·앱·설정이 덮어써질 수 있습니다.</li>
            <li>앱을 다시 설치하는 동안 Play 프로텍트 확인 창이 뜰 수 있습니다. 폰 화면을 켜 두고 확인 창이 뜨면 눌러 주세요.</li>
            <li>앱 데이터는 루팅된 기기에서만 복원합니다. 루팅되지 않았으면 앱 데이터는 건너뜁니다.</li>
            <li>다음 화면에서 원본 백업 폴더와 복원할 항목을 검증·선택합니다.</li>
          {:else if wizard.mode !== "manual" && wizard.device?.bootloader === "unlocked"}
            <li>이미 언락된 폰이라 이번 작업에서는 초기화되지 않습니다. 마지막에 <b>부트로더 리락</b>을 선택하면 그때 초기화됩니다.</li>
            <li>부트 이미지 기록 중 USB가 끊기면 부팅되지 않을 수 있습니다. 작업이 끝날 때까지 케이블을 건드리지 마세요.</li>
            <li>다음 단계에서 백업 항목과 저장 위치를 지정할 수 있습니다.</li>
          {:else if wizard.mode !== "manual" || wizard.manualTask === "unlock" || wizard.manualTask === "relock"}
            <li>부트로더 {wizard.manualTask === "relock" ? "리락" : "언락"} 과정에서 핸드폰이 <b>초기화</b>되며, 내부 저장소의 모든 데이터가 삭제됩니다.</li>
            {#if wizard.manualTask === "relock"}<li>리락 전에 루팅을 해제하며, 설치된 루팅 모듈과 매니저 설정도 모두 지웁니다.</li>{/if}
            <li>공동인증서·OTP·금융 앱 인증 등 일부 데이터는 백업하더라도 복원되지 않습니다. 작업 전 각 앱에서 직접 내보내기/이전을 해 두세요.</li>
            {#if wizard.mode !== "manual"}<li>다음 단계에서 백업 항목과 저장 위치를 지정할 수 있습니다.</li>{/if}
          {:else if wizard.manualTask === "root"}
            <li>부트 이미지를 루팅용으로 새로 기록합니다. 폰 데이터는 초기화되지 않습니다.</li>
            <li>기록 중 USB가 끊기면 부팅되지 않을 수 있습니다. 작업이 끝날 때까지 케이블을 건드리지 마세요.</li>
            <li>중간에 폰 화면에서 루트 권한을 허용해야 합니다. 안내가 뜨면 폰을 확인해 주세요.</li>
          {:else if wizard.manualTask === "root-manager"}
            <li>지금 쓰는 루팅 엔진을 해제하고 다른 엔진(Magisk ↔ ReSukiSU)으로 다시 루팅합니다. 폰 데이터는 초기화되지 않습니다.</li>
            <li><b>설치된 루팅 모듈과 매니저 설정(모듈 설정·DenyList·슈퍼유저 권한 등)을 모두 지웁니다.</b> 엔진마다 동작이 달라 옮기지 않습니다. 필요한 설정은 미리 직접 백업해 두세요.</li>
            <li>ReSukiSU로 바꾸면 중간에 폰의 ReSukiSU 앱에서 패치를 한 번 직접 눌러야 합니다.</li>
            <li>부트 이미지 기록 중 USB가 끊기면 부팅되지 않을 수 있습니다. 작업이 끝날 때까지 케이블을 건드리지 마세요.</li>
          {:else if wizard.manualTask === "unroot"}
            <li>순정 부트 이미지를 다시 기록해 루팅을 해제합니다. 폰 데이터는 초기화되지 않습니다.</li>
            <li><b>설치된 루팅 모듈과 매니저 설정을 모두 지웁니다.</b> 필요한 설정은 미리 직접 백업해 두세요.</li>
            <li>루팅이 필요한 앱과 모듈은 더 이상 동작하지 않습니다. Magisk 앱은 해제를 확인한 뒤 자동으로 지웁니다.</li>
            <li>기록 중 USB가 끊기면 부팅되지 않을 수 있습니다. 작업이 끝날 때까지 케이블을 건드리지 마세요.</li>
          {:else if wizard.manualTask === "volte-rollback"}
            <li>VoLTE 패치 직전에 저장해 둔 모뎀 설정 사본으로 되돌립니다. 되돌리면 패치한 VoLTE가 꺼집니다.</li>
            <li>모뎀 연결(DIAG)에 루트 권한이 필요합니다. 언락·초기화는 하지 않습니다.</li>
            <li>기록 중 USB가 끊기면 통신이 되지 않을 수 있습니다. 작업이 끝날 때까지 케이블을 건드리지 마세요.</li>
          {:else if wizard.manualTask === "volte"}
            <li>선택한 SIM의 통신사 VoLTE 설정을 폰의 모뎀 설정에 기록합니다. 폰 데이터는 초기화되지 않습니다.</li>
            <li>기록 전 원래 모뎀 설정을 PC에 저장해 둡니다. 기록 중 USB가 끊기면 통신이 되지 않을 수 있으니 케이블을 건드리지 마세요.</li>
            <li>적용 뒤 폰이 다시 시작되며, 통화가 되는지 직접 확인해 주세요.</li>
          {/if}
        </ul>
        {#if wizard.mode === "manual" && (wizard.manualTask === "unlock" || wizard.manualTask === "relock")}
          <!-- 수동 모드는 백업을 함께 묶지 않는다 — 필요하면 백업 작업으로 바로 보낸다 -->
          <div class="mt-4 flex items-center gap-3 rounded-lg bg-warning-container/60 px-4 py-3">
            <p class="flex-1 text-xs text-warning">이 작업에는 백업 단계가 없습니다. 아직 백업하지 않았다면 먼저 백업을 끝내 주세요.</p>
            <Button size="sm" onclick={() => wizard.chooseManualTask("backup")}><HardDrive size={14} />백업부터 하러 가기</Button>
          </div>
        {/if}
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
