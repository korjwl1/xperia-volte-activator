<script lang="ts">
  // 같은 폰의 끝나지 않은 작업 — 경고 페이지 [다음] 뒤에 표시, 이어서 진행 / 새로 시작 선택
  import { Button } from "$lib/components/ui/button";
  import { History, CircleCheck, TriangleAlert, Circle } from "@lucide/svelte/icons";
  import { wizard } from "$lib/stores/wizard.svelte";
  import { mockBackupGroups } from "$lib/mock/apps";
  import { CARRIER_LABEL, type RunJournal } from "$lib/types";

  let { journal }: { journal: RunJournal } = $props();

  const fmt = (iso: string) => {
    const d = new Date(iso);
    return isNaN(d.getTime()) ? "-" : d.toLocaleString("ko-KR", { month: "long", day: "numeric", hour: "2-digit", minute: "2-digit" });
  };

  const itemLabel = new Map(mockBackupGroups.flatMap((g) => g.items.map((i) => [i.id, i.label] as const)));

  const done = $derived(journal.runSteps.filter((s) => s.status === "done" || s.status === "skipped").length);
  const stopIdx = $derived(journal.runSteps.findIndex((s) => s.status !== "done" && s.status !== "skipped"));
  const stopStep = $derived(stopIdx >= 0 ? journal.runSteps[stopIdx] : null);

  // 멈춘 사유 — 명시적으로 멈춘 기록이 없으면 마지막 상태로 추정
  const reason = $derived.by(() => {
    if (journal.stop) return journal.stop.reason;
    if (stopStep?.status === "manual-wait") return "폰에서 확인을 기다리는 중에 앱이 종료되었거나 폰 연결이 끊겼습니다";
    if (stopStep?.status === "running") return "진행 중에 앱이 종료되었거나 폰 연결이 끊겼습니다";
    return "다음 단계를 시작하기 전에 멈췄습니다";
  });
  const lastLogs = $derived((stopStep?.logs ?? []).slice(-4));

  const options = $derived.by(() => {
    const c = journal.config;
    const rows: [string, string][] = c.sims.map((s) => [`SIM${s.slot}`, s.carrier ? CARRIER_LABEL[s.carrier] : "패치 안 함"]);
    rows.push(["펌웨어", c.firmware ? `${c.firmware}로 업데이트` : "현재 버전 유지"]);
    if (c.bootloaderAction) rows.push(["부트로더", c.bootloaderAction === "unlock" ? "언락만 진행" : "리락만 진행"]);
    rows.push([
      "백업",
      journal.backupItems.length > 0
        ? `${journal.backupItems.map((id) => itemLabel.get(id) ?? id).join(", ")}${journal.backupPath ? ` → ${journal.backupPath}` : ""}`
        : "백업 안 함",
    ]);
    const post = [journal.opts.unroot && "언루팅", journal.opts.relock && "리락", journal.opts.restore && journal.backupItems.length > 0 && "복구"].filter(Boolean);
    if (post.length > 0) rows.push(["마무리", post.join(" · ")]);
    return rows;
  });
</script>

<div class="fixed inset-0 z-50 flex items-center justify-center bg-black/45 p-6" role="dialog">
  <div class="w-full max-w-xl max-h-full flex flex-col rounded-2xl border bg-background elev-3">
    <div class="flex items-center gap-3 p-6 pb-4 shrink-0">
      <span class="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-primary/10 text-primary">
        <History size={20} />
      </span>
      <div class="space-y-0.5">
        <h2 class="text-base font-semibold">이전에 하던 작업이 있습니다! 불러올까요?</h2>
        <p class="text-xs text-muted-foreground">
          {journal.productName || journal.model} · {journal.serialMasked} · 마지막 기록 {fmt(journal.updatedAt)}
        </p>
      </div>
    </div>

    <div class="min-h-0 overflow-y-auto px-6 space-y-4">
      <!-- 멈춘 지점 -->
      {#if stopStep}
        <div class="rounded-xl border border-warning/40 bg-warning-container/40 px-4 py-3 space-y-2">
          <div class="flex items-start gap-2">
            <TriangleAlert size={15} class="mt-0.5 shrink-0 text-warning" />
            <div class="min-w-0 space-y-0.5">
              <div class="text-sm font-semibold">{stopStep.title} 단계에서 멈췄습니다</div>
              <div class="text-[12px] text-muted-foreground">{reason}{journal.stop ? ` · ${fmt(journal.stop.at)}` : ""}</div>
              {#if stopStep.sub && stopStep.sub.done > 0}
                <div class="text-[12px]">
                  세부 작업 {stopStep.sub.done}/{stopStep.sub.list.length} 완료 — '{stopStep.sub.list[stopStep.sub.done - 1]}'까지 끝났습니다
                </div>
              {/if}
            </div>
          </div>
          {#if lastLogs.length > 0}
            <div class="console-bg rounded-lg px-3 py-2 font-mono text-[11px] leading-relaxed space-y-0.5 text-zinc-300">
              {#each lastLogs as l, i (i)}<div class="truncate">{l}</div>{/each}
            </div>
          {/if}
        </div>
      {/if}

      <!-- 진행 상황 -->
      <div class="space-y-2">
        <div class="flex items-baseline justify-between">
          <span class="text-xs font-semibold text-muted-foreground uppercase tracking-wide">진행 상황</span>
          <span class="text-[11px] text-muted-foreground tabular-nums">{done} / {journal.runSteps.length} 단계 완료</span>
        </div>
        <ol class="rounded-xl border divide-y">
          {#each journal.runSteps as s, i (s.id)}
            {@const isDone = s.status === "done" || s.status === "skipped"}
            <li class="flex items-center gap-2.5 px-3 py-1.5 text-[13px] {i === stopIdx ? 'bg-warning-container/30' : ''}">
              {#if isDone}
                <CircleCheck size={14} class="shrink-0 text-success" />
              {:else if i === stopIdx}
                <TriangleAlert size={14} class="shrink-0 text-warning" />
              {:else}
                <Circle size={14} class="shrink-0 text-muted-foreground/50" />
              {/if}
              <span class="flex-1 {isDone || i === stopIdx ? '' : 'text-muted-foreground'}">{s.title}</span>
              {#if i === stopIdx}<span class="text-[11px] text-warning">여기서 다시 시작</span>{/if}
            </li>
          {/each}
        </ol>
      </div>

      <!-- 선택했던 옵션 -->
      <div class="space-y-2">
        <span class="text-xs font-semibold text-muted-foreground uppercase tracking-wide">선택했던 옵션</span>
        <dl class="rounded-xl border divide-y text-[13px]">
          {#each options as [k, v] (k)}
            <div class="flex gap-3 px-3 py-1.5">
              <dt class="w-16 shrink-0 text-muted-foreground">{k}</dt>
              <dd class="min-w-0 break-all">{v}</dd>
            </div>
          {/each}
        </dl>
      </div>

      <p class="text-[11px] text-muted-foreground">
        이어서 진행하면 멈춘 단계의 끝낸 세부 작업 다음부터 진행합니다. 언락 코드는 저장하지 않으므로 언락 전이라면 다시 입력해야 합니다.
      </p>
    </div>

    <div class="flex justify-end gap-2 p-6 pt-4 shrink-0">
      <Button variant="outline" size="sm" onclick={() => wizard.discardJournal()}>새로 시작</Button>
      <Button size="sm" onclick={() => wizard.resumeJournal()}>이어서 진행</Button>
    </div>
  </div>
</div>
