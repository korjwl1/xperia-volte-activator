<script lang="ts">
  import { onMount } from "svelte";
  import { Button } from "$lib/components/ui/button";
  import { Badge } from "$lib/components/ui/badge";
  import OptionCard from "$lib/components/OptionCard.svelte";
  import OptionCategory from "$lib/components/OptionCategory.svelte";
  import { Checkbox } from "$lib/components/ui/checkbox";
  import { TriangleAlert, FolderOpen } from "@lucide/svelte/icons";
  import { Tooltip, TooltipContent, TooltipTrigger, TooltipProvider } from "$lib/components/ui/tooltip";
  import { wizard } from "$lib/stores/wizard.svelte";
  import { api } from "$lib/api";
  import { mockBackupGroups } from "$lib/mock/apps";
  import type { PlanStep } from "$lib/types";

  let activeTab = $state<"backup" | "rooting">("backup");
  let showPathAlert = $state(false);

  const hasWipe = wizard.device?.bootloader === "locked";
  const defaultOn = hasWipe;

  // 실측 용량 조회
  let realSizes = $state<Record<string, number>>({});
  let sizesLoading = $state(true);

  onMount(async () => {
    try {
      realSizes = await api.storageSizes();
    } finally {
      sizesLoading = false;
    }
  });

  let backupGroups = $state(
    mockBackupGroups.map((g) => ({
      id: g.id,
      label: g.label,
      desc: g.desc,
      bytes: g.items.reduce((a: number, i) => a + (i.bytes ?? 0), 0),
      items: g.items.map((i) => ({ ...i, checked: defaultOn })),
    })),
  );

  // 실측 용량이 도착하면 업데이트
  $effect(() => {
    if (Object.keys(realSizes).length === 0) return;

    // 앱 그룹
    const apps = backupGroups.find((g) => g.id === "apps");
    if (apps) {
      const apkItem = apps.items.find((i) => i.id === "apk");
      if (apkItem && realSizes["apk-total"]) apkItem.bytes = realSizes["apk-total"];
      const dataItem = apps.items.find((i) => i.id === "app-data");
      if (dataItem && realSizes["android-data"]) dataItem.bytes = realSizes["android-data"];
      apps.bytes = apps.items.reduce((a: number, i) => a + (i.bytes ?? 0), 0);
    }

    // 파일 그룹
    const files = backupGroups.find((g) => g.id === "files");
    if (files) {
      const sizeMap: Record<string, string> = {
        dcim: "dcim", download: "download", pictures: "pictures",
        movies: "movies", music: "music", documents: "documents",
        recordings: "recordings",
      };
      let namedTotal = 0;
      for (const item of files.items) {
        if (item.id === "fs-rest") continue;
        if (sizeMap[item.id] && realSizes[sizeMap[item.id]]) {
          item.bytes = realSizes[sizeMap[item.id]];
          namedTotal += realSizes[sizeMap[item.id]];
        } else if (item.bytes) {
          namedTotal += item.bytes;
        }
      }
      // 그 외 = sdcard 전체 - Android/data - 기명 항목 합계
      const fsRest = files.items.find((i) => i.id === "fs-rest");
      const sdcardTotal = realSizes["sdcard-total"] ?? 0;
      const androidData = realSizes["android-data"] ?? 0;
      const nonAppData = sdcardTotal - androidData;
      if (fsRest) {
        fsRest.bytes = Math.max(0, nonAppData - namedTotal);
      }
      files.bytes = files.items.reduce((a: number, i) => a + (i.bytes ?? 0), 0);
    }

    // 설정/메시지는 예상치 (실측 불가 — 텍스트 덤프)
    for (const g of backupGroups) {
      if (g.id === "settings" || g.id === "sms") {
        g.bytes = g.items.reduce((a: number, i) => a + (i.bytes ?? 0), 0);
      }
    }
  });
  let opts = $state({ restore: defaultOn, unroot: true, relock: true });

  const fmtBytes = (b: number) => {
    const gb = b / 1024 ** 3;
    if (gb >= 1) return `${gb.toFixed(1)} GB`;
    const mb = b / 1024 ** 2;
    if (mb >= 1) return `${mb.toFixed(0)} MB`;
    return `${Math.max(1, Math.round(b / 1024))} KB`;
  };

  const selectedBytes = $derived(backupGroups.flatMap((g) => g.items).filter((i) => i.checked).reduce((a, i) => a + (i.bytes ?? 0), 0));
  const anyBackupChecked = $derived(backupGroups.some((g) => g.items.some((i) => i.checked)));

  // 백업 저장 위치(PC 드라이브)의 여유 공간 — 경로가 바뀔 때마다 조회
  let freeBytes = $state<number | null>(null);
  let freeLoading = $state(false);
  $effect(() => {
    const path = wizard.backupPath.trim();
    freeBytes = null;
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
  const usageRatio = $derived(freeBytes ? selectedBytes / freeBytes : 0);
  const diskWarning = $derived(freeBytes !== null && usageRatio > 0.85);

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
    return cat ? backupGroups.filter((g) => cat.groupIds.includes(g.id)) : [];
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

  const planSteps = $derived.by(() => {
    const steps: { title: string; warn?: boolean }[] = [];
    const d = wizard.device;
    if (!d) return steps;
    const hasBackup = backupGroups.flatMap((g) => g.items).some((i) => i.checked);
    const needsUnlock = d.bootloader === "locked";
    const effUnroot = opts.unroot || opts.relock;
    if (hasBackup) steps.push({ title: "백업" });
    if (needsUnlock) {
      steps.push({ title: "개발자 옵션 준비" });
      steps.push({ title: "부트로더 언락", warn: true });
      steps.push({ title: "기본 설정" });
    }
    if (d.rooted !== true) steps.push({ title: "루팅" });
    steps.push({ title: "연결 안정성 검사" });
    steps.push({ title: "VoLTE 적용" });
    steps.push({ title: "적용 확인" });
    if (needsUnlock) {
      if (effUnroot) steps.push({ title: "언루팅" });
      if (opts.relock) steps.push({ title: "부트로더 리락", warn: true });
      steps.push({ title: "최종 확인" });
      if (opts.restore && hasBackup) steps.push({ title: "복구" });
    }
    return steps;
  });

  // 실행 전 확인 모달 — 초기화 단계가 포함된 계획에서만 (AGENTS 규칙 7)
  let confirmOpen = $state(false);
  let wipeAck = $state(false);
  let noBackupAck = $state(false);
  const wipeStepTitles = $derived(planSteps.filter((s) => s.warn).map((s) => s.title));
  const canLaunch = $derived(wipeAck && (anyBackupChecked || noBackupAck));

  function confirm() {
    // 방어: 백업 선택 + 경로 미지정 or 용량 부족
    if (anyBackupChecked && (!wizard.backupPath.trim() || diskWarning)) {
      showPathAlert = true;
      setTimeout(() => (showPathAlert = false), 4000);
      return;
    }
    showPathAlert = false;
    if (wipeStepTitles.length > 0) {
      wipeAck = false;
      noBackupAck = false;
      confirmOpen = true;
      return;
    }
    launch();
  }

  function launch() {
    confirmOpen = false;
    const anyBackup = anyBackupChecked;
    const effUnroot = opts.unroot || opts.relock;
    wizard.groups = mockBackupGroups.map((g) => {
      const bgGroup = backupGroups.find((bg) => bg.id === g.id);
      return {
        ...g,
        items: g.items.map((i) => {
          const bgItem = bgGroup?.items.find((bi) => bi.id === i.id);
          return { ...i, checked: bgItem?.checked ?? false };
        }),
      };
    });
    const steps: PlanStep[] = [];
    const d = wizard.device;
    if (!d) return;
    const push = (id: string, title: string, risk: PlanStep["risk"] = "safe", wipe = false, manual?: string, estSec = 120) =>
      steps.push({ id, kind: id as PlanStep["kind"], title, desc: "", optional: false, enabled: true, risk, wipe, estSec, manual: manual as PlanStep["manual"] });
    const needsUnlock = d.bootloader === "locked";
    if (anyBackup) push("backup-1", "백업", "warn", false, undefined, 1800);
    if (needsUnlock) {
      push("dev-options", "개발자 옵션 준비", "safe", false, "oem-toggle", 120);
      push("unlock", "부트로더 언락", "danger", true, "mode-wait", 120);
      push("setup-min", "기본 설정", "safe", false, "usb-debug", 300);
    }
    if (d.rooted !== true) push("root", "루팅", "warn", false, "magisk-patch", 600);
    push("efs-preflight", "연결 안정성 검사", "safe", false, undefined, 60);
    push("efs", "VoLTE 적용", "danger", false, undefined, 420);
    push("verify", "적용 확인", "safe", false, undefined, 120);
    if (needsUnlock) {
      if (effUnroot) push("unroot", "언루팅", "warn", false, undefined, 180);
      if (opts.relock) push("relock", "부트로더 리락", "danger", true, "mode-wait", 120);
      push("final-verify", "최종 확인", "safe", false, "ims-check", 300);
      if (opts.restore && anyBackup) push("restore", "복구", "safe", false, undefined, 1500);
    }
    wizard.steps = steps;
    wizard.view = "step3";
    wizard.prepareRun();
    wizard.begin();
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
                    <b class="inline-block h-3.5 w-14 align-middle bg-muted rounded animate-pulse"></b>
                  {:else}
                    <b>{fmtBytes(selectedBytes)}</b>
                  {/if}
                  {#if wizard.backupPath.trim() && (freeLoading || freeBytes !== null)}
                    / 여유 공간
                    {#if freeLoading}
                      <b class="inline-block h-3.5 w-10 align-middle bg-muted rounded animate-pulse"></b>
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
                    right={item.bytes ? fmtBytes(item.bytes) : undefined}
                    loading={sizesLoading && (cat.id === "apps" || cat.id === "files")}
                  />
                {/each}
              </div>
            </div>
          {/each}

          <div class="h-px bg-border mb-3"></div>

          <div class="space-y-1.5">
            <OptionCard
              checked={opts.restore}
              label="복구 자동 실행"
              desc="모든 작업 완료 후 백업한 데이터를 자동으로 복원합니다"
              onToggle={(v) => (opts.restore = v)}
            />
          </div>

        {:else}
          {#if wizard.device?.bootloader === "locked"}
            <div class="mb-3 px-1 text-[11px] text-muted-foreground">
              부트로더 언락 · 루팅 · VoLTE 적용은 자동으로 진행됩니다
            </div>
            <div class="h-px bg-border mb-3"></div>
            <div class="space-y-1.5">
              <OptionCard
                checked={opts.unroot}
                label="언루팅"
                desc="시스템을 원래대로 되돌립니다 — 리락하려면 필요합니다"
                onToggle={(v) => (opts.unroot = v)}
              />
              <OptionCard
                checked={opts.relock}
                label="부트로더 리락"
                desc="기기가 다시 초기화됩니다"
                onToggle={(v) => (opts.relock = v)}
                badge={opts.relock ? "초기화" : undefined}
                badgeVariant="destructive"
              />
            </div>
            {#if !opts.unroot && opts.relock}
              <div class="mt-2 rounded-lg bg-warning-container/60 px-4 py-2 text-xs text-warning">
                리락하려면 언루팅이 필요합니다 — 언루팅이 자동으로 포함됩니다
              </div>
            {/if}
          {:else}
            <div class="px-1 text-[11px] text-muted-foreground">
              {wizard.device?.rooted === true
                ? "이미 루팅되어 있어 언락/루팅 단계를 건너뜁니다. VoLTE 적용만 진행됩니다."
                : "루팅 후 VoLTE 적용이 진행됩니다."}
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
          {#if step.warn}
            <!-- 부트로더 언락/리락: ! 삼각형 + 목차/글자까지 행 전체가 초기화 안내 툴팁 트리거 -->
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
                부트로더 언락/리락 단계는 핸드폰 데이터가 초기화될 수 있습니다. 백업을 권장합니다.
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
    </div>
  </div>

  {#if showPathAlert}
    <div class="shrink-0 bg-danger-container px-6 py-2 text-xs text-destructive font-medium">
      {#if !wizard.backupPath.trim()}
        백업 위치를 지정해 주세요
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

<!-- 실행 전 확인 모달 — 초기화 단계 포함 시 (백업 미선택이면 추가 확인) -->
{#if confirmOpen}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/45 p-6" role="dialog">
    <div class="w-full max-w-md rounded-2xl border-2 border-destructive/40 bg-background elev-3 p-6 space-y-4">
      <div class="flex items-center gap-3">
        <span class="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-danger-container text-destructive">
          <TriangleAlert size={20} />
        </span>
        <div class="space-y-0.5">
          <h2 class="text-base font-semibold">핸드폰 데이터가 초기화됩니다</h2>
          <p class="text-xs text-muted-foreground">실행 순서에 아래 초기화 단계가 포함되어 있습니다</p>
        </div>
      </div>
      <ul class="space-y-1 text-[13px]">
        {#each wipeStepTitles as t (t)}
          <li class="flex items-center gap-2 text-destructive"><TriangleAlert size={12} class="shrink-0" />{t}</li>
        {/each}
      </ul>
      <label class="flex items-center gap-2.5 rounded-lg border px-4 py-2.5 cursor-pointer {wipeAck ? 'border-destructive/40 bg-danger-container/40' : 'border-border'}">
        <Checkbox checked={wipeAck} onCheckedChange={(v: boolean | "indeterminate") => (wipeAck = v === true)} />
        <span class="text-[13px] font-medium">데이터가 초기화되는 것을 확인했습니다</span>
      </label>
      {#if !anyBackupChecked}
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
