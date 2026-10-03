<script lang="ts">
  import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "$lib/components/ui/card";
  import { Button } from "$lib/components/ui/button";
  import { Badge } from "$lib/components/ui/badge";
  import { CircleCheck, CircleX, RotateCcw, HardDrive, TriangleAlert } from "@lucide/svelte/icons";
  import { wizard } from "$lib/stores/wizard.svelte";

  const succeeded = $derived(wizard.runSteps.filter((s) => s.status === "done").length);
  const failed = $derived(wizard.runSteps.filter((s) => s.status === "failed").length);
  const skipped = $derived(wizard.runSteps.filter((s) => s.status === "skipped").length);
  const allOk = $derived(failed === 0);
  // 백업 단계가 실제로 완료된 경우에만 "백업 보관 중" 표시
  const backedUp = $derived(wizard.runSteps.some((s) => s.id === "backup" && s.status === "done"));
  let restoreAck = $state(false);
  // VoLTE 패치를 실제로 진행했는지 — 직접 확인 안내 표시
  const patched = $derived(wizard.runSteps.some((s) => s.id === "efs" && s.status === "done"));
  // 실패는 없지만 통신을 확인하지 못한 종료는 "성공"과 구분
  const unverified = $derived(allOk && wizard.imsUnverified);
</script>

<div class="flex-1 overflow-y-auto flex">
  <div class="m-auto w-full max-w-lg p-6 space-y-4">
    <Card class="elev-2">
      <CardContent class="pt-8 pb-8 text-center space-y-4">
        <div class="flex h-20 w-20 mx-auto items-center justify-center rounded-full {unverified ? 'bg-warning-container' : allOk ? 'bg-success-container' : 'bg-danger-container'}">
          {#if unverified}
            <TriangleAlert size={40} class="text-warning" />
          {:else if allOk}
            <CircleCheck size={40} class="text-success" />
          {:else}
            <CircleX size={40} class="text-destructive" />
          {/if}
        </div>
        <h1 class="text-2xl font-bold">{unverified ? "작업 종료 · 통신 미검증" : allOk ? "완료" : "일부 실패"}</h1>
        <p class="text-sm text-muted-foreground">
          {unverified
            ? "작업은 끝났지만 VoLTE 통화는 확인하지 못했습니다"
            : allOk
              ? "모든 작업이 성공적으로 완료되었습니다"
              : `${failed}개 단계에서 오류가 발생했습니다`}
        </p>
        <div class="flex items-center justify-center gap-3 text-sm">
          <span class="text-success font-semibold">✓ {succeeded} 성공</span>
          {#if failed > 0}<span class="text-destructive font-semibold">✗ {failed} 실패</span>{/if}
          {#if skipped > 0}<span class="text-muted-foreground">– {skipped} 건너뜀</span>{/if}
        </div>
      </CardContent>
    </Card>

    {#if wizard.imsUnverified}
      <Card class="elev-1">
        <CardContent class="py-4 text-[13px]">
          <div class="font-medium">VoLTE 등록은 확인하지 못했습니다</div>
          <div class="text-[12px] text-muted-foreground">
            SIM을 넣고 재부팅한 뒤 VoLTE가 켜지는지 확인해 주세요. SIM 없이 패치한 경우 처음 SIM을 넣을 때 프로파일이 바뀌어 패치가 풀릴 수 있어, 그때는 다시 패치해야 합니다.
          </div>
        </CardContent>
      </Card>
    {/if}

    {#if patched}
      <Card class="elev-1">
        <CardContent class="py-4 space-y-2 text-[13px]">
          <div class="font-medium">직접 확인해 주세요</div>
          <ul class="list-disc pl-5 space-y-0.5 text-[12px] text-muted-foreground">
            <li>실제 발신·수신 — VoLTE 등록 표시와 통화 성공은 다를 수 있습니다</li>
            <li>문자·MMS, 5G를 쓴다면 5G 데이터</li>
            <li>해외 로밍은 국내 통화와 별개입니다 — 따로 확인되기 전까지 보장되지 않습니다</li>
            <li>SIM 교체, 통신망 변경, 모뎀 포함 업데이트 뒤에는 패치가 풀릴 수 있습니다 (전원을 끄고 바꿔도 풀린 사례가 있습니다)</li>
          </ul>
        </CardContent>
      </Card>
    {/if}

    {#if backedUp && wizard.backupPath}
      <Card class="elev-1">
        <CardContent class="py-4 flex items-center gap-3">
          <HardDrive size={20} class="text-info shrink-0" />
          <div class="min-w-0 flex-1">
            <div class="text-sm font-medium">{restoreAck ? "복구 완료 승인됨" : "백업 보관 중"}</div>
            <div class="text-xs text-muted-foreground font-mono truncate">{wizard.backupPath}</div>
          </div>
          <!-- mock: 승인 상태만 표시 (백업 보존 해제 backup_ack는 M3) -->
          <Button size="sm" variant="outline" disabled={restoreAck} onclick={() => (restoreAck = true)}>복구 완료 승인</Button>
        </CardContent>
      </Card>
    {/if}

    <Card class="elev-1">
      <CardHeader class="pb-2">
        <CardTitle class="text-sm">작업 요약</CardTitle>
      </CardHeader>
      <CardContent class="space-y-1">
        {#each wizard.runSteps as step (step.id)}
          <div class="flex items-center gap-2 text-[13px] py-0.5">
            {#if step.status === "done"}
              <CircleCheck size={13} class="text-success shrink-0" />
            {:else if step.status === "failed"}
              <CircleX size={13} class="text-destructive shrink-0" />
            {:else}
              <span class="w-[13px] shrink-0 text-muted-foreground">–</span>
            {/if}
            <span class={step.status === "done" ? "" : "text-muted-foreground"}>{step.title}</span>
          </div>
        {/each}
      </CardContent>
    </Card>

    <div class="flex justify-center pt-2">
      <Button variant="outline" onclick={() => wizard.restart()}>
        <RotateCcw size={14} class="mr-2" />
        처음으로
      </Button>
    </div>
  </div>
</div>
