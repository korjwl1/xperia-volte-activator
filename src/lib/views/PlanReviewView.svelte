<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import OptionCard from "$lib/components/OptionCard.svelte";
  import OptionCategory from "$lib/components/OptionCategory.svelte";
  import { Checkbox } from "$lib/components/ui/checkbox";
  import { TriangleAlert, FolderOpen, LoaderCircle } from "@lucide/svelte/icons";
  import { Tooltip, TooltipContent, TooltipTrigger, TooltipProvider } from "$lib/components/ui/tooltip";
  import { wizard } from "$lib/stores/wizard.svelte";
  import { api, inDesktop as desktopRuntime } from "$lib/api";
  import { simIssue, type BackupItem } from "$lib/types";
  import { stepHazard } from "$lib/domain/plan";

  // 선택 상태·실측 결과는 스토어에 보관 — 이전/다음으로 오가도 유지 (기기가 바뀔 때만 초기화)
  wizard.ensureOptions();

  let activeTab = $state<"backup" | "rooting">("backup");
  let showPathAlert = $state(false);
  let pathAlertTimer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => () => clearTimeout(pathAlertTimer));

  const bootloaderKnown = $derived(wizard.device?.bootloader === "locked" || wizard.device?.bootloader === "unlocked");
  const sizesLoading = $derived(wizard.sizesState === "loading");

  // ── 항목별 용량: 실측(storage_sizes) 매핑, 실측 불가 항목은 고정 추정치 ──
  const FOLDER_IDS = ["dcim", "download", "pictures", "movies", "music", "documents", "recordings"];
  const SIZE_KEY: Record<string, string> = { apk: "apk-total", "app-data": "android-data" };

  /** 바이트 — 실측 실패/미도착이면 null */
  function bytesOf(item: BackupItem): number | null {
    if (item.estBytes !== undefined) return item.estBytes;
    const s = wizard.sizes;
    if (!s) return null;
    if (item.id === "fs-rest") {
      // 그 외 = 내부 저장소 전체 − Android/data − 기명 폴더 합계
      if (!("sdcard-total" in s) || FOLDER_IDS.some((id) => !(id in s))) return null; // 하나라도 측정 실패면 계산 불가
      const named = FOLDER_IDS.reduce((a, id) => a + (s[id] ?? 0), 0);
      return Math.max(0, s["sdcard-total"] - (s["android-data"] ?? 0) - named);
    }
    const key = SIZE_KEY[item.id] ?? item.id;
    return key in s ? s[key] : null;
  }

  const fmtBytes = (b: number) => {
    const gb = b / 1024 ** 3;
    if (gb >= 1) return `${gb.toFixed(1)} GB`;
    const mb = b / 1024 ** 2;
    if (mb >= 1) return `${mb.toFixed(0)} MB`;
    return `${Math.max(1, Math.round(b / 1024))} KB`;
  };

  function rightLabel(item: BackupItem): string | undefined {
    const b = bytesOf(item);
    if (item.estBytes !== undefined) return `약 ${fmtBytes(item.estBytes)} (추정)`;
    if (b !== null) return fmtBytes(b);
    return wizard.sizesState === "failed" ? "측정 불가" : undefined;
  }
  const isLoading = (item: BackupItem) => sizesLoading && item.estBytes === undefined;

  const anyBackupChecked = $derived(wizard.anyBackupChecked);
  const selectedBytes = $derived(
    wizard.groups.flatMap((g) => g.items).filter((i) => i.checked).reduce((a, i) => a + (bytesOf(i) ?? 0), 0),
  );

  // 백업 저장 위치(PC 드라이브)의 여유 공간 — 경로가 바뀔 때마다 조회
  let freeBytes = $state<number | null>(null);
  let freeLoading = $state(false);
  $effect(() => {
    const path = wizard.backupPath.trim();
    freeBytes = null;
    freeLoading = false;
    if (!path) return;
    freeLoading = true;
    let stale = false;
    api.diskFree(path).then((v) => {
      if (stale) return;
      freeBytes = v;
      freeLoading = false;
    });
    return () => {
      stale = true;
    };
  });
  const diskFreeGB = $derived((freeBytes ?? 0) / 1024 ** 3);
  const usageRatio = $derived(freeBytes === null ? 0 : freeBytes === 0 ? Infinity : selectedBytes / freeBytes);
  const diskWarning = $derived(freeBytes !== null && usageRatio > 0.85);
  // 데스크톱 앱에서 여유 공간을 확인하지 못했으면(조회 중·실패) 실행하지 않는다 — 브라우저 개발 환경은 조회 수단이 없어 제외
  const inDesktop = desktopRuntime();
  const diskUnknown = $derived(inDesktop && wizard.backupPath.trim() !== "" && (freeLoading || freeBytes === null));

  async function pickBackupFolder() {
    const path = await api.pickFolder();
    if (path) wizard.backupPath = path;
  }

  const backupCategories = [
    { id: "settings", label: "설정", groupIds: ["settings"] },
    { id: "apps", label: "앱", groupIds: ["apps"] },
    { id: "files", label: "파일", groupIds: ["files"] },
    { id: "sms", label: "통화 및 문자", groupIds: ["sms"] },
  ];

  function groupsFor(catId: string) {
    const cat = backupCategories.find((c) => c.id === catId);
    return cat ? wizard.groups.filter((g) => cat.groupIds.includes(g.id)) : [];
  }
  function itemsFor(catId: string) {
    return groupsFor(catId).flatMap((g) => g.items);
  }
  function setCategoryAll(catId: string, on: boolean) {
    for (const item of itemsFor(catId)) item.checked = on;
  }
  function isCategoryAll(catId: string) {
    const items = itemsFor(catId);
    return items.length > 0 && items.every((i) => i.checked);
  }
  function countSelected(catId: string) {
    return itemsFor(catId).filter((i) => i.checked).length;
  }
  function countTotal(catId: string) {
    return itemsFor(catId).length;
  }

  // 실행 순서 — 실제 실행과 같은 계획(wizard.plan)에서 파생
  // hazard: 초기화·펌웨어/부트 이미지 기록·모뎀 설정 수정 — 툴팁·확인 모달 대상(domain/plan stepHazard)
  const planSteps = $derived(wizard.plan.map((s) => ({ title: s.title, wipe: s.wipe, hazard: stepHazard(s) })));
  const patching = $derived(wizard.hasPatchTarget);
  // 패치 대상 슬롯의 SIM 문제(없음·PIN 잠김·통신사 미확인) — 기기·선택에 따라 고정
  const simProblems = $derived(
    wizard.volteConfig.sims
      .filter((t) => t.carrier !== null)
      .map((t) => ({ slot: t.slot, issue: simIssue(wizard.device?.sims.find((s) => s.slot === t.slot)) }))
      .filter((x) => x.issue !== null),
  );

  // 실행 전 확인 모달 — 위험 단계(초기화·기록·모뎀 설정 수정)가 포함된 계획에서만 (AGENTS 규칙 7)
  let confirmOpen = $state(false);
  let wipeAck = $state(false);
  let noBackupAck = $state(false);
  const riskySteps = $derived(planSteps.filter((s) => s.hazard !== null));
  const hasWipe = $derived(planSteps.some((s) => s.wipe));
  // 백업 미선택 이중 확인은 초기화가 있을 때만
  const canLaunch = $derived(wipeAck && (anyBackupChecked || !hasWipe || noBackupAck));

  function confirm() {
    // 방어: 백업 선택 + 경로 미지정 or 용량 부족
    // 용량 계산 중에는 여유 공간 판단이 불완전하므로 실행 보류
    if (anyBackupChecked && (!wizard.backupPath.trim() || diskWarning || diskUnknown || sizesLoading)) {
      showPathAlert = true;
      // 연속 클릭 시 이전 타이머가 새 알림을 일찍 닫지 않게 다시 건다
      clearTimeout(pathAlertTimer);
      pathAlertTimer = setTimeout(() => (showPathAlert = false), 4000);
      return;
    }
    showPathAlert = false;
    if (riskySteps.length > 0) {
      wipeAck = false;
      noBackupAck = false;
      confirmOpen = true;
      return;
    }
    launch();
  }

  function launch() {
    confirmOpen = false;
    wizard.launch();
  }
</script>

<div class="flex-1 min-h-0 flex flex-col">
  <div class="flex-1 min-h-0 flex gap-4 p-4 lg:p-6">
    <!-- 좌: 옵션 (더 넓게) -->
    <div class="flex-[7] min-w-0 flex flex-col gap-4">
      <div class="shrink-0 flex gap-1 rounded-lg bg-muted p-1">
        <button
          class="flex-1 rounded-md px-4 py-2 text-sm font-medium transition-colors
            {activeTab === 'backup' ? 'bg-background elev-1 text-foreground' : 'text-muted-foreground hover:text-foreground'}"
          onclick={() => (activeTab = "backup")}
        >
          백업 및 복구
        </button>
        <button
          class="flex-1 rounded-md px-4 py-2 text-sm font-medium transition-colors
            {activeTab === 'rooting' ? 'bg-background elev-1 text-foreground' : 'text-muted-foreground hover:text-foreground'}"
          onclick={() => (activeTab = "rooting")}
        >
          루팅
        </button>
      </div>

      <div class="flex-1 min-h-0 overflow-y-auto">
        {#if activeTab === "backup"}
          {#if anyBackupChecked}
            <div class="sticky top-0 z-10 mb-3 bg-background/95 backdrop-blur border-b pb-3 space-y-2">
              <div class="flex items-center gap-2.5">
                <FolderOpen size={16} class="text-primary shrink-0" />
                <div class="flex-1 min-w-0 rounded-lg border bg-background px-3 py-1.5 font-mono text-[12px] truncate">
                  {wizard.backupPath || "백업 위치를 지정해 주세요"}
                </div>
                <Button size="sm" variant="outline" class="h-7 text-xs shrink-0" onclick={pickBackupFolder}>
                  폴더 지정
                </Button>
              </div>
              <div class="flex items-center justify-between text-[11px]">
                <span>
                  예상
                  {#if sizesLoading}
                    <b class="inline-flex items-center gap-1 align-middle text-muted-foreground font-medium">
                      <LoaderCircle size={12} class="animate-spin text-primary" />휴대폰 용량 계산 중…
                    </b>
                  {:else}
                    <b>{fmtBytes(selectedBytes)}</b>
                    {#if wizard.sizesState === "failed"}
                      <span class="text-muted-foreground">(측정 불가 항목 제외)</span>
                    {/if}
                  {/if}
                  {#if wizard.backupPath.trim() && (freeLoading || freeBytes !== null)}
                    / 여유 공간
                    {#if freeLoading}
                      <b class="inline-flex items-center align-middle"><LoaderCircle size={12} class="animate-spin text-primary" /></b>
                    {:else}
                      <b class={diskWarning ? "text-destructive" : ""}>{diskFreeGB.toFixed(0)} GB</b>
                    {/if}
                  {/if}
                </span>
                {#if wizard.backupPath.trim() && diskWarning}
                  <span class="text-destructive font-medium">여유 공간이 부족합니다</span>
                {/if}
              </div>
            </div>
          {/if}

          {#each backupCategories as cat (cat.id)}
            <div class="mb-4">
              <OptionCategory
                label={cat.label}
                selected={countSelected(cat.id)}
                total={countTotal(cat.id)}
                onToggleAll={() => setCategoryAll(cat.id, !isCategoryAll(cat.id))}
              />
              <div class="space-y-1.5">
                {#each groupsFor(cat.id).flatMap((g) => g.items) as item (item.id)}
                  <OptionCard
                    checked={item.checked}
                    label={item.label}
                    onToggle={(v) => (item.checked = v)}
                    right={rightLabel(item)}
                    loading={isLoading(item)}
                  />
                {/each}
              </div>
            </div>
          {/each}

          <div class="h-px bg-border mb-3"></div>

          <div class="space-y-1.5">
            <OptionCard
              checked={wizard.opts.restore}
              label="복구 자동 실행"
              desc="모든 작업 완료 후 백업한 데이터를 자동으로 복원합니다"
              onToggle={(v) => (wizard.opts.restore = v)}
            />
          </div>

        {:else}
          {#if wizard.bootloaderOnly === "unlock"}
            <div class="px-1 text-[11px] text-muted-foreground">
              부트로더 언락만 진행합니다 — 루팅 · VoLTE 적용은 하지 않습니다
            </div>
          {:else if wizard.bootloaderOnly === "relock"}
            <div class="px-1 text-[11px] text-muted-foreground">
              {wizard.device?.rooted === false
                ? "부트로더 리락만 진행합니다 — VoLTE 패치는 유지됩니다"
                : "리락 전에 언루팅(순정 이미지 복원)을 자동으로 진행합니다 — VoLTE 패치는 유지됩니다"}
            </div>
          {:else if !patching}
            <div class="px-1 text-[11px] text-muted-foreground">
              {wizard.device?.rooted === true
                ? "VoLTE 패치를 선택하지 않았습니다 — 펌웨어 업데이트 후 풀리는 루팅만 새 버전으로 다시 적용합니다"
                : "VoLTE 패치를 선택하지 않아 언락 · 루팅 관련 옵션이 없습니다"}
            </div>
          {:else if bootloaderKnown}
            <div class="mb-3 px-1 text-[11px] text-muted-foreground">
              {#if wizard.device?.bootloader === "locked"}
                부트로더 언락 · 루팅 · VoLTE 적용은 자동으로 진행됩니다
              {:else if wizard.device?.rooted === true}
                이미 언락·루팅되어 있어 VoLTE 적용만 자동으로 진행됩니다
              {:else}
                이미 언락되어 있어 루팅 · VoLTE 적용만 자동으로 진행됩니다
              {/if}
            </div>
            <div class="h-px bg-border mb-3"></div>
            <div class="space-y-1.5">
              <OptionCard
                checked={wizard.opts.unroot}
                label="언루팅"
                desc="시스템을 원래대로 되돌립니다 — 리락하려면 필요합니다"
                onToggle={(v) => (wizard.opts.unroot = v)}
              />
              <OptionCard
                checked={wizard.opts.relock}
                label="부트로더 리락"
                desc="기기가 다시 초기화됩니다"
                onToggle={(v) => (wizard.opts.relock = v)}
                badge={wizard.opts.relock ? "초기화" : undefined}
                badgeVariant="destructive"
              />
            </div>
            {#if !wizard.opts.unroot && wizard.opts.relock}
              <div class="mt-2 rounded-lg bg-warning-container/60 px-4 py-2 text-xs text-warning">
                리락하려면 언루팅이 필요합니다 — 언루팅이 자동으로 포함됩니다
              </div>
            {/if}
          {:else}
            <div class="px-1 text-[11px] text-muted-foreground">
              부트로더 상태를 확인할 수 없어 루팅 · VoLTE 적용만 진행됩니다
            </div>
          {/if}
        {/if}
      </div>
    </div>

    <!-- 우: 실행 순서 (좁게) -->
    <div class="flex-[3] min-w-0 max-w-[280px] rounded-xl border bg-muted/30 flex flex-col overflow-hidden">
      <div class="shrink-0 px-4 py-3 border-b">
        <div class="text-sm font-semibold">실행 순서</div>
        <div class="text-[11px] text-muted-foreground">{planSteps.length}단계</div>
      </div>
      <div class="flex-1 overflow-y-auto p-2">
        <TooltipProvider delayDuration={150}>
        {#each planSteps as step, i (i)}
          {#if step.hazard}
            <!-- 위험 단계: ! 삼각형 + 목차/글자까지 행 전체가 위험 안내 툴팁 트리거 -->
            <Tooltip>
              <TooltipTrigger
                class="flex w-full items-center gap-2.5 rounded-md px-2 py-1.5 text-[13px] text-destructive text-left cursor-help"
              >
                <span class="w-5 h-5 shrink-0 rounded-full border border-destructive/40 flex items-center justify-center text-[10px] text-destructive">
                  {i + 1}
                </span>
                <span class="truncate">{step.title}</span>
                <TriangleAlert size={12} class="shrink-0 text-destructive/70" />
              </TooltipTrigger>
              <TooltipContent>
                {step.hazard.detail}
              </TooltipContent>
            </Tooltip>
          {:else}
            <div class="flex items-center gap-2.5 rounded-md px-2 py-1.5 text-[13px]">
              <span class="w-5 h-5 shrink-0 rounded-full border flex items-center justify-center text-[10px] border-border text-muted-foreground">
                {i + 1}
              </span>
              <span class="truncate">{step.title}</span>
            </div>
          {/if}
        {/each}
        </TooltipProvider>
      </div>
      {#if simProblems.length > 0}
        <div class="shrink-0 border-t bg-warning-container/40 px-4 py-3 space-y-1">
          <div class="flex items-center gap-1.5 text-[12px] font-semibold text-warning">
            <TriangleAlert size={13} class="shrink-0" />{simProblems.map((p) => `SIM${p.slot} ${p.issue}`).join(" · ")}
          </div>
          <p class="text-[11px] leading-relaxed text-muted-foreground">
            SIM 없이 패치하면 처음 SIM을 넣을 때 프로파일이 바뀌어 패치가 풀릴 수 있습니다. 사용할 SIM을 넣고 진행하는 것을 권장합니다.
            {wizard.opts.relock ? "리락 전 통신 확인을 할 수 없어 리락 단계로 넘어갈 수 없습니다." : ""}
          </p>
        </div>
      {/if}
    </div>
  </div>

  {#if showPathAlert}
    <div class="shrink-0 bg-danger-container px-6 py-2 text-xs text-destructive font-medium">
      {#if !wizard.backupPath.trim()}
        백업 위치를 지정해 주세요
      {:else if sizesLoading}
        휴대폰 용량 계산이 끝난 뒤 실행해 주세요
      {:else if diskUnknown}
        {freeLoading ? "백업 위치의 여유 공간을 확인하는 중입니다 — 잠시 후 실행해 주세요" : "백업 위치의 여유 공간을 확인할 수 없습니다 — 다른 위치를 지정해 주세요"}
      {:else}
        디스크 여유 공간이 부족합니다 — 백업 항목을 줄이거나 다른 위치를 지정해 주세요
      {/if}
    </div>
  {/if}

  <footer class="h-14 shrink-0 border-t bg-muted/40 flex items-center justify-between px-6">
    <Button variant="ghost" size="sm" onclick={() => (wizard.view = "step1")}>← 이전</Button>
    <Button size="sm" onclick={confirm}>실행</Button>
  </footer>
</div>

<!-- 실행 전 확인 모달 — 위험 단계 포함 시 (초기화가 있고 백업 미선택이면 추가 확인) -->
{#if confirmOpen}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/45 p-6" role="dialog">
    <div class="w-full max-w-md rounded-2xl border-2 border-destructive/40 bg-background elev-3 p-6 space-y-4">
      <div class="flex items-center gap-3">
        <span class="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-danger-container text-destructive">
          <TriangleAlert size={20} />
        </span>
        <div class="space-y-0.5">
          <h2 class="text-base font-semibold">{hasWipe ? "핸드폰 데이터가 초기화됩니다" : "기기에 직접 기록하는 단계가 있습니다"}</h2>
          <p class="text-xs text-muted-foreground">실행 순서에 아래 되돌리기 어려운 단계가 포함되어 있습니다</p>
        </div>
      </div>
      <ul class="space-y-1 text-[13px]">
        {#each riskySteps as st (st.title)}
          <li class="flex items-center gap-2 text-destructive">
            <TriangleAlert size={12} class="shrink-0" />{st.title}
            <span class="text-[11px] text-muted-foreground">— {st.hazard?.short}</span>
          </li>
          {#if !st.wipe}<li class="pl-5 text-[11px] text-muted-foreground">{st.hazard?.detail}</li>{/if}
        {/each}
      </ul>
      <label class="flex items-center gap-2.5 rounded-lg border px-4 py-2.5 cursor-pointer {wipeAck ? 'border-destructive/40 bg-danger-container/40' : 'border-border'}">
        <Checkbox checked={wipeAck} onCheckedChange={(v: boolean | "indeterminate") => (wipeAck = v === true)} />
        <span class="text-[13px] font-medium">{hasWipe ? "데이터가 초기화되는 것을 확인했습니다" : "위 작업이 진행되는 것을 확인했습니다"}</span>
      </label>
      {#if hasWipe && !anyBackupChecked}
        <div class="rounded-lg bg-danger-container/60 px-4 py-2 text-xs text-destructive">
          백업 항목이 선택되지 않았습니다. 초기화된 데이터는 복구할 수 없습니다.
        </div>
        <label class="flex items-center gap-2.5 rounded-lg border px-4 py-2.5 cursor-pointer {noBackupAck ? 'border-destructive/40 bg-danger-container/40' : 'border-border'}">
          <Checkbox checked={noBackupAck} onCheckedChange={(v: boolean | "indeterminate") => (noBackupAck = v === true)} />
          <span class="text-[13px] font-medium">백업 없이 진행합니다</span>
        </label>
      {/if}
      <div class="flex justify-end gap-2">
        <Button variant="outline" size="sm" onclick={() => (confirmOpen = false)}>취소</Button>
        <Button variant="destructive" size="sm" disabled={!canLaunch} onclick={launch}>실행</Button>
      </div>
    </div>
  </div>
{/if}
