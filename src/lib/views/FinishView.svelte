<script lang="ts">
  import Modal from "$lib/components/Modal.svelte";
  import CommunicationPanel from "$lib/components/CommunicationPanel.svelte";
  import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "$lib/components/ui/card";
  import { Button } from "$lib/components/ui/button";
  import { Badge } from "$lib/components/ui/badge";
  import { CircleCheck, CircleX, RotateCcw, HardDrive, TriangleAlert, Trash2, CircleMinus } from "@lucide/svelte/icons";
  import { wizard } from "$lib/stores/wizard.svelte";
  import { Checkbox } from "$lib/components/ui/checkbox";

  const succeeded = $derived(wizard.runSteps.filter((s) => s.status === "done").length);
  const failed = $derived(wizard.runSteps.filter((s) => s.status === "failed").length);
  const skipped = $derived(wizard.runSteps.filter((s) => s.status === "skipped").length);
  const allOk = $derived(failed === 0);
  // 백업 단계가 실제로 완료된 경우에만 "백업 보관 중" 표시
  const backedUp = $derived(wizard.runSteps.some((s) => s.id === "backup" && s.status === "done"));
  // 목 모드(실전 백업 꺼짐)는 실제 백업 폴더가 없다 — 삭제도 표시만 바뀐다
  const mockBackup = $derived(!wizard.backupLive);
  let deleteConfirmOpen = $state(false);
  // 초기화 후 복원이 끝나지 않았으면 이중 확인(백업이 유일한 사본)
  let keepAck = $state(false);
  function openDeleteConfirm() {
    keepAck = false;
    deleteConfirmOpen = true;
  }
  function confirmDelete() {
    deleteConfirmOpen = false;
    void wizard.deleteBackup();
  }
  // VoLTE 패치를 실제로 진행했는지 — 직접 확인 안내 표시
  const patched = $derived(wizard.runSteps.some((s) => s.id === "efs" && s.status === "done"));
  // 실패는 없지만 통신을 확인하지 못한 종료는 "성공"과 구분
  const communicationTask = $derived(wizard.runSteps.some(s => s.id === "final-verify") || patched);
  const fileVerified = $derived(wizard.runSteps.some(s => s.id === "verify" && s.status === "done"));
  const unverified = $derived(allOk && communicationTask && (!wizard.imsVerified || !wizard.callVerified));
  const resultTitle = $derived(unverified
    ? `${patched && fileVerified ? "패치 완료" : "작업 종료"} · ${wizard.imsVerified ? "통화 미확인" : "통신 미확인"}`
    : allOk ? "완료" : "일부 실패");
  // 오른쪽(통신 확인) 열이 필요한지 — VoLTE를 적용했거나 통신을 확인하는 작업일 때만
  const hasRight = $derived(communicationTask || patched);
</script>

<div class="flex-1 min-h-0 overflow-y-auto">
  <!-- 결과·요약(좌) | 통신 확인(우) 2단 — 확인할 통신이 없으면 1단 -->
  <div class="mx-auto w-full p-6 {hasRight ? 'grid max-w-6xl grid-cols-2 items-start gap-5' : 'max-w-2xl space-y-4'}">
    <div class="space-y-4">
      <Card class="elev-2">
        <CardContent class="flex items-center gap-5 py-6">
          <div class="flex size-16 shrink-0 items-center justify-center rounded-full {unverified ? 'bg-warning-container' : allOk ? 'bg-success-container' : 'bg-danger-container'}">
            {#if unverified}<TriangleAlert size={32} class="text-warning" />{:else if allOk}<CircleCheck size={32} class="text-success" />{:else}<CircleX size={32} class="text-destructive" />{/if}
          </div>
          <div class="min-w-0 space-y-1">
            <h1 class="text-xl font-bold">{resultTitle}</h1>
            <p class="text-sm text-muted-foreground">
              {unverified ? "기록 작업과 실제 통신의 확인 결과를 구분해 표시합니다" : allOk ? "모든 작업이 성공적으로 완료되었습니다" : `${failed}개 단계에서 오류가 발생했습니다`}
            </p>
            <div class="flex items-center gap-3 text-sm">
              <span class="inline-flex items-center gap-1 font-semibold text-success"><CircleCheck size={14} />{succeeded} 성공</span>
              {#if failed > 0}<span class="inline-flex items-center gap-1 font-semibold text-destructive"><CircleX size={14} />{failed} 실패</span>{/if}
              {#if skipped > 0}<span class="inline-flex items-center gap-1 text-muted-foreground"><CircleMinus size={14} />{skipped} 건너뜀</span>{/if}
            </div>
          </div>
        </CardContent>
      </Card>

      <Card class="elev-1">
        <CardHeader class="pb-2"><CardTitle class="text-sm">작업 요약</CardTitle></CardHeader>
        <CardContent class="space-y-1">
          {#each wizard.runSteps as step (step.id)}
            <div class="flex items-center gap-2 py-0.5 text-[13px]">
              {#if step.status === "done"}<CircleCheck size={14} class="shrink-0 text-success" />
              {:else if step.status === "failed"}<CircleX size={14} class="shrink-0 text-destructive" />
              {:else}<CircleMinus size={14} class="shrink-0 text-muted-foreground" />{/if}
              <span class={step.status === "done" ? "" : "text-muted-foreground"}>{step.title}</span>
            </div>
          {/each}
        </CardContent>
      </Card>

      {#if backedUp && wizard.backupPath}
        <Card class="elev-1">
          <CardContent class="flex items-center gap-3 py-4">
            <HardDrive size={20} class="shrink-0 text-info" />
            <div class="min-w-0 flex-1">
              <div class="text-sm font-medium">{wizard.backupDeleteState === "deleted" ? (mockBackup ? "백업 삭제됨(목업)" : "백업 삭제됨") : "백업 보관 중"}</div>
              <div class="truncate font-mono text-xs text-muted-foreground" title={wizard.backupDir || wizard.backupPath}>{wizard.backupDir || wizard.backupPath}</div>
              {#if wizard.backupDeleteState === "failed"}<div class="text-[11px] text-destructive">{wizard.backupDeleteError}</div>{/if}
            </div>
            {#if wizard.backupDeleteState !== "deleted"}
              <Button size="sm" variant="outline" class="text-destructive" disabled={wizard.backupDeleteState === "deleting"} onclick={openDeleteConfirm}>
                <Trash2 size={13} class="mr-1" />백업 파일 삭제
              </Button>
            {/if}
          </CardContent>
        </Card>
      {/if}

      <div class="flex gap-2 pt-1">
        {#if wizard.mode === "manual" && wizard.finished}<Button variant="outline" disabled={wizard.busy > 0} onclick={() => wizard.returnToTasks()}>수동 작업 목록으로</Button>{/if}
        <Button variant="outline" onclick={() => wizard.restart()}><RotateCcw size={14} class="mr-2" />처음으로</Button>
      </div>
    </div>

    {#if hasRight}
      <div class="space-y-4">
        {#if communicationTask}
          <CommunicationPanel snapshot={wizard.communicationLatest} loading={wizard.communicationLoading} error={wizard.communicationError} slots={wizard.communicationSlots} calls={wizard.callChecks} showCalls onRefresh={() => void wizard.refreshCommunication()} onCallChange={(slot, item, checked) => wizard.setCallCheck(slot, item, checked)} />
        {/if}
        <!-- 확인 결과와 직접 확인할 것을 한 카드로 -->
        <Card class="elev-1">
          <CardContent class="space-y-2 py-4 text-[13px]">
            <div class="font-medium">확인 결과</div>
            {#if patched}<div class="flex justify-between gap-3"><span>VoLTE 설정 기록 검증</span><span class={fileVerified ? "text-success" : "text-warning"}>{fileVerified ? "일치" : "미확인"}</span></div>{/if}
            {#if communicationTask}
              <div class="flex justify-between gap-3"><span>VoLTE 등록</span><span class={wizard.imsVerified ? "text-success" : "text-warning"}>{wizard.imsVerified ? "확인" : "미확인"}</span></div>
              <div class="flex justify-between gap-3"><span>발신·수신·양방향 음성</span><span class={wizard.callVerified ? "text-success" : "text-warning"}>{wizard.callVerified ? "확인" : "미확인"}</span></div>
            {/if}
            <div class="pt-2 font-medium">직접 확인해 주세요</div>
            <ul class="list-disc space-y-0.5 pl-5 text-[12px] text-muted-foreground">
              <li>실제 발신·수신 — VoLTE 등록 표시와 통화 성공은 다를 수 있습니다</li>
              <li>문자·MMS, 5G를 쓴다면 5G 데이터, 해외 로밍</li>
              <li>SIM 교체·통신망 변경·모뎀 포함 업데이트 뒤에는 패치가 풀릴 수 있습니다</li>
            </ul>
          </CardContent>
        </Card>
      </div>
    {/if}
  </div>
</div>

<!-- 백업 삭제 확인 — 되돌릴 수 없음 -->
{#if deleteConfirmOpen}
  <Modal title="백업 삭제 확인" onClose={() => { deleteConfirmOpen = false; }} class="fixed inset-0 z-50 flex items-center justify-center bg-black/45 p-6">
    <div class="w-full max-w-md rounded-2xl border-2 border-destructive/40 bg-background elev-3 p-6 space-y-4">
      <div class="flex items-center gap-3">
        <span class="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-danger-container text-destructive">
          <Trash2 size={20} />
        </span>
        <div class="space-y-0.5">
          <h2 class="text-base font-semibold">백업 파일을 삭제합니다</h2>
          <p class="text-xs text-muted-foreground">삭제한 백업은 되돌릴 수 없습니다</p>
        </div>
      </div>
      <div class="rounded-lg bg-muted/50 px-3 py-2 text-xs font-mono break-all">{wizard.backupDir || wizard.backupPath}</div>
      {#if mockBackup}
        <p class="text-[11px] text-muted-foreground">(목업) 실전 백업이 꺼져 있어 실제 파일은 삭제하지 않습니다</p>
      {/if}
      {#if wizard.backupStillNeeded}
        <div class="rounded-lg bg-danger-container/60 px-4 py-2 text-xs text-destructive">
          폰이 초기화됐지만 복원이 끝나지 않았습니다. 이 백업이 유일한 사본일 수 있으며, 지우면 데이터를 되돌릴 수 없습니다.
        </div>
        <label class="flex items-center gap-2.5 rounded-lg border px-4 py-2.5 cursor-pointer {keepAck ? 'border-destructive/40 bg-danger-container/40' : 'border-border'}">
          <Checkbox checked={keepAck} onCheckedChange={(v: boolean | "indeterminate") => (keepAck = v === true)} />
          <span class="text-[13px] font-medium">복원되지 않은 데이터가 있어도 삭제합니다</span>
        </label>
      {/if}
      <div class="flex justify-end gap-2">
        <Button variant="outline" size="sm" onclick={() => (deleteConfirmOpen = false)}>취소</Button>
        <Button variant="destructive" size="sm" disabled={wizard.backupStillNeeded && !keepAck} onclick={confirmDelete}>삭제</Button>
      </div>
    </div>
  </Modal>
{/if}
