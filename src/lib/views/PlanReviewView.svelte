<script lang="ts">
  import { onMount } from "svelte";
  import { Button } from "$lib/components/ui/button";
  import { Badge } from "$lib/components/ui/badge";
  import OptionCard from "$lib/components/OptionCard.svelte";
  import OptionCategory from "$lib/components/OptionCategory.svelte";
  import { TriangleAlert, FolderOpen } from "@lucide/svelte/icons";
  import { wizard } from "$lib/stores/wizard.svelte";
  import { api } from "$lib/api";
  import { mockBackupGroups, mockDiskFree } from "$lib/mock/apps";
  import type { PlanStep } from "$lib/types";

  let activeTab = $state<"backup" | "rooting">("backup");
  let showPathAlert = $state(false);

  const hasWipe = wizard.device?.bootloader === "locked";
  const defaultOn = hasWipe;

  // 실측 용량 조회
  let realSizes = $state<Record<string, number>>({});

  onMount(async () => {
    realSizes = await api.storageSizes();
  });

  let backupGroups = $state(
    mockBackupGroups.map((g) => ({
      id: g.id,
      label: g.label,
      desc: g.desc,
      checked: defaultOn,
      bytes: g.items.reduce((a: number, i) => a + (i.bytes ?? 0), 0),
      items: g.items,
    })),
  );

  // 실측 용량이 도착하면 업데이트
  $effect(() => {
    if (Object.keys(realSizes).length === 0) return;
    const sizeMap: Record<string, number> = {
      "dcim": realSizes["dcim"] ?? 0,
      "download": realSizes["download"] ?? 0,
      "pictures": realSizes["pictures"] ?? 0,
      "perfectviewer": realSizes["perfectviewer"] ?? 0,
      "dxo": realSizes["dxo"] ?? 0,
      "kakao-media": realSizes["kakao-media"] ?? 0,
    };
    // storage 그룹 업데이트
    const storage = backupGroups.find((g) => g.id === "storage");
    if (storage) {
      let total = 0;
      for (const item of storage.items) {
        if (sizeMap[item.id] !== undefined) {
          item.bytes = sizeMap[item.id];
          total += sizeMap[item.id];
        } else if (item.bytes) {
          total += item.bytes;
        }
      }
      storage.bytes = total;
    }
    // hidden 그룹 업데이트
    const hidden = backupGroups.find((g) => g.id === "hidden");
    if (hidden) {
      const kakao = realSizes["kakao-media"] ?? 0;
      const androidData = realSizes["android-data"] ?? 0;
      const others = Math.max(0, androidData - kakao);
      hidden.items[0].bytes = kakao;
      if (hidden.items[1]) hidden.items[1].bytes = others;
      hidden.bytes = kakao + others;
    }
    // 전체 재계산
    for (const g of backupGroups) {
      if (g.id !== "storage" && g.id !== "hidden") {
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

  const selectedBytes = $derived(backupGroups.filter((g) => g.checked).reduce((a, g) => a + g.bytes, 0));
  const diskFreeGB = $derived((realSizes["sdcard-total"] ? realSizes["sdcard-total"] * 1.2 : mockDiskFree) / 1024 ** 3);
  const usageRatio = $derived(selectedBytes / mockDiskFree);
  const diskWarning = $derived(usageRatio > 0.85);

  const backupCategories = [
    { id: "settings", label: "설정", groupIds: ["settings"] },
    { id: "apps", label: "앱", groupIds: ["apps"] },
    { id: "files", label: "파일", groupIds: ["storage", "hidden", "sms"] },
  ];

  function groupsFor(catId: string) {
    const cat = backupCategories.find((c) => c.id === catId);
    return cat ? backupGroups.filter((g) => cat.groupIds.includes(g.id)) : [];
  }
  function setCategoryAll(catId: string, on: boolean) {
    for (const g of groupsFor(catId)) g.checked = on;
  }
  function isCategoryAll(catId: string) {
    return groupsFor(catId).every((g) => g.checked);
  }
  function countSelected(catId: string) {
    return groupsFor(catId).filter((g) => g.checked).length;
  }
  function countTotal(catId: string) {
    return groupsFor(catId).length;
  }

  const planSteps = $derived.by(() => {
    const steps: { title: string; warn?: boolean }[] = [];
    const d = wizard.device;
    if (!d) return steps;
    const anyBackup = backupGroups.some((g) => g.checked);
    const needsUnlock = d.bootloader === "locked";
    const effUnroot = opts.unroot || opts.relock;
    if (anyBackup) steps.push({ title: "백업" });
    if (needsUnlock) {
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
      if (opts.restore && anyBackup) steps.push({ title: "복구" });
    }
    return steps;
  });

  function confirm() {
    // 방어: 백업 선택 + 경로 미지정 or 용량 부족
    if (backupGroups.some((g) => g.checked)) {
      if (!wizard.backupPath.trim() || diskWarning) {
        showPathAlert = true;
        setTimeout(() => (showPathAlert = false), 4000);
        return;
      }
    }
    showPathAlert = false;
    const effUnroot = opts.unroot || opts.relock;
    wizard.groups = mockBackupGroups.map((g) => ({
      ...g,
      items: g.items.map((i) => ({ ...i, checked: backupGroups.find((bg) => bg.id === g.id)?.checked ?? false })),
    }));
    const steps: PlanStep[] = [];
    const d = wizard.device;
    if (!d) return;
    const push = (id: string, title: string, risk: PlanStep["risk"] = "safe", wipe = false, manual?: string, estSec = 120) =>
      steps.push({ id, kind: id as PlanStep["kind"], title, desc: "", optional: false, enabled: true, risk, wipe, estSec, manual: manual as PlanStep["manual"] });
    const anyBackup = backupGroups.some((g) => g.checked);
    const needsUnlock = d.bootloader === "locked";
    if (anyBackup) push("backup-1", "백업", "warn", false, undefined, 1800);
    if (needsUnlock) {
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
          {#if backupGroups.some((g) => g.checked)}
            <div class="sticky top-0 z-10 mb-3 bg-background/95 backdrop-blur border-b pb-3 space-y-2">
              <div class="flex items-center gap-2.5">
                <FolderOpen size={16} class="text-primary shrink-0" />
                <div class="flex-1 min-w-0 rounded-lg border bg-background px-3 py-1.5 font-mono text-[12px] truncate">
                  {wizard.backupPath || "백업 위치를 지정해 주세요"}
                </div>
                <Button size="sm" variant="outline" class="h-7 text-xs shrink-0" onclick={async () => {
                  const w = window as unknown as { showDirectoryPicker?: (opts: object) => Promise<FileSystemDirectoryHandle> };
                  if (!w.showDirectoryPicker) return;
                  const handle = await w.showDirectoryPicker({ mode: "readwrite" }).catch(() => null);
                  if (handle) wizard.backupPath = handle.name;
                }}>
                  폴더 지정
                </Button>
              </div>
              <div class="flex items-center justify-between text-[11px]">
                <span>
                  예상 <b>{fmtBytes(selectedBytes)}</b>
                  {#if wizard.backupPath.trim()}
                    / 여유 공간 <b class={diskWarning ? "text-destructive" : ""}>{diskFreeGB.toFixed(0)} GB</b>
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
                {#each groupsFor(cat.id) as group (group.id)}
                  <OptionCard
                    checked={group.checked}
                    label={group.label}
                    desc={group.desc}
                    onToggle={(v) => (group.checked = v)}
                    right={group.bytes ? fmtBytes(group.bytes) : undefined}
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
        {#each planSteps as step, i (i)}
          <div class="flex items-center gap-2.5 rounded-md px-2 py-1.5 text-[13px] {step.warn ? 'text-destructive' : ''}">
            <span class="w-5 h-5 shrink-0 rounded-full border flex items-center justify-center text-[10px]
              {step.warn ? 'border-destructive/40 text-destructive' : 'border-border text-muted-foreground'}">
              {i + 1}
            </span>
            <span class="truncate">{step.title}</span>
            {#if step.warn}
              <TriangleAlert size={12} class="shrink-0 text-destructive/70 cursor-help" title="이 단계에서 기기가 초기화됩니다" />
            {/if}
          </div>
        {/each}
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
