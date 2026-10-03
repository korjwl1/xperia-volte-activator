<script lang="ts">
  import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "$lib/components/ui/card";
  import { Button } from "$lib/components/ui/button";
  import { Badge } from "$lib/components/ui/badge";
  import { CircleCheck, CircleX, RotateCcw, HardDrive } from "@lucide/svelte/icons";
  import { wizard } from "$lib/stores/wizard.svelte";

  const succeeded = $derived(wizard.runSteps.filter((s) => s.status === "done").length);
  const failed = $derived(wizard.runSteps.filter((s) => s.status === "failed").length);
  const skipped = $derived(wizard.runSteps.filter((s) => s.status === "skipped").length);
  const allOk = $derived(failed === 0);
  // 백업 단계가 실제로 완료된 경우에만 "백업 보관 중" 표시
  const backedUp = $derived(wizard.runSteps.some((s) => s.id === "backup" && s.status === "done"));
  let restoreAck = $state(false);
</script>

<div class="flex-1 overflow-y-auto flex">
  <div class="m-auto w-full max-w-lg p-6 space-y-4">
    <Card class="elev-2">
      <CardContent class="pt-8 pb-8 text-center space-y-4">
        <div class="flex h-20 w-20 mx-auto items-center justify-center rounded-full {allOk ? 'bg-success-container' : 'bg-danger-container'}">
          {#if allOk}
            <CircleCheck size={40} class="text-success" />
          {:else}
            <CircleX size={40} class="text-destructive" />
          {/if}
        </div>
        <h1 class="text-2xl font-bold">{allOk ? "완료" : "일부 실패"}</h1>
        <p class="text-sm text-muted-foreground">
          {allOk
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
            작업은 모두 끝났습니다. SIM을 넣고 재부팅한 뒤 통화 설정에서 VoLTE가 켜지는지 확인해 주세요.
          </div>
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
