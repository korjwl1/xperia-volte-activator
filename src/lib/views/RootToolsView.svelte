<script lang="ts">
  import ModuleSetSelector from "$lib/components/ModuleSetSelector.svelte";
  import ModuleInstruction from "$lib/components/ModuleInstruction.svelte";
  import { emptyModuleSelection, installModuleSets, selectedModuleSets, waitForModuleReboot } from "$lib/domain/moduleSets";
  import { onMount } from "svelte";
  import { Button } from "$lib/components/ui/button";
  import { api } from "$lib/api";
  import { REAL_STEPS } from "$lib/data/runMode";
  import { bootPartition } from "$lib/data/devices";
  import { rootToolsState } from "$lib/stores/rootTools.svelte";
  import { requireResult, prepareMagiskImage, flashRootImage, waitForFastboot } from "$lib/domain/rootTools";
  import type { DeviceStatus, RootState, RootRelease, RootPackage, RootSwitch, RootModuleInventory, RootImportedImage, RootUpdatePlan, RootModuleSelection } from "$lib/types";
  import { ArrowLeft, TriangleAlert, ShieldCheck, Package, RefreshCw, Loader2 } from "@lucide/svelte/icons";

  let { device, onClose, section = "manager" }: { device: DeviceStatus; onClose: () => void; section?: "manager" | "modules" } = $props();
  // Capture this session's phone once; DeviceStatus polling pauses while this view is open.
  const { serial, partition } = (() => ({ serial: device.serial ?? "", partition: bootPartition(device.model) }))();
  let root = $state<RootState | null>(null);
  let inventory = $state<RootModuleInventory | null>(null);
  let caps = $state({ writeEnabled: false, switchEnabled: false });
  let releases = $state<RootRelease[]>([]);
  let tag = $state("");
  let target = $state<"magisk" | "resukisu">("resukisu");
  let manager = $state<RootPackage | null>(null);
  let stock = $state<RootImportedImage | null>(null);
  let patched = $state<RootImportedImage | null>(null);
  let switched = $state<RootSwitch | null>(null);
  let riskAck = $state(false);
  let samePhone = $state(false);
  let selection = $state<RootModuleSelection>(emptyModuleSelection());
  let moduleInstruction = $state("");
  let moduleInstructionResolve: ((completed: boolean) => void) | null = null;
  let cancelled = false;
  let patchedPath = $state("");
  let log = $state<string[]>([]);
  let error = $state("");
  let updatePlan = $state<RootUpdatePlan | null>(null);
  const busy = $derived(rootToolsState.busy);
  const canWrite = $derived(caps.writeEnabled && REAL_STEPS.rootTools && riskAck && !!serial);
  const canFlash = $derived(canWrite && REAL_STEPS.fastboot && caps.switchEnabled);
  const openSwitch = $derived(switched && switched.stage !== "complete");
  const newEngineAllowed = $derived(switched?.stage === "stock-verified" || (!openSwitch && root?.access === "unavailable" && root.engine === "unknown"));
  const supportedTarget = $derived(target === "magisk" || partition === "init_boot");

  onMount(() => {
    void api.rootToolsCapabilities().then(result => { if (result.ok) caps = result.value; else error = result.error; });
    // 들어오자마자 권한·엔진·모듈을 조회한다(버튼을 누르지 않아도 되게)
    if (serial) void work("루트 상태 조회", inspect);
  });
  async function work(label: string, operation: () => Promise<void>, writes = false) {
    if (busy) return;
    rootToolsState.busy = true; rootToolsState.label = label; error = "";
    let guard = false;
    try {
      if (writes) {
        if (!canWrite) throw new Error("실전 기능 활성화와 위험 확인이 필요합니다");
        guard = await api.runGuard(true, label);
        if (!guard) throw new Error("작업 중 PC 보호를 시작하지 못했습니다");
      }
      await operation();
      log = [...log.slice(-199), `${label}: 완료`];
    } catch (e) {
      if (writes) { root = null; inventory = null; updatePlan = null; patched = null; manager = null; }
      error = e instanceof Error ? e.message : String(e);
      log = [...log.slice(-199), `${label}: 중단 · ${error}`];
    } finally {
      try { if (guard && !(await api.runGuard(false))) error = error || "PC 보호 해제 실패"; }
      catch (e) { error = error || `PC 보호 해제 실패: ${e instanceof Error ? e.message : String(e)}`; }
      finally { rootToolsState.busy = false; rootToolsState.label = ""; }
    }
  }
  async function inspect() {
    root = null; inventory = null; updatePlan = null;
    const observed = requireResult(await api.rootInspect(serial));
    const modules = observed.access === "granted" && ["magisk", "kernelsu-family"].includes(observed.engine)
      ? requireResult(await api.rootModulesInspect(serial)) : null;
    root = observed; inventory = modules;
  }
  async function loadSwitch() { switched = null; resetPatch(); switched = requireResult(await api.rootSwitchStatus(serial)); target = switched.target; }
  async function prepareStock() {
    stock = null; resetPatch();
    if (!partition) throw new Error("부트 파티션을 확인할 수 없는 기종입니다");
    const fw = requireResult(await api.firmwareFetch(serial, partition));
    const sha256 = requireResult(await api.bootImageCheck(serial, fw.path, fw.fingerprint));
    stock = { path: fw.path, sha256, partition, fingerprint: fw.fingerprint }; resetPatch();
  }
  function resetPatch() { manager = null; patched = null; samePhone = false; patchedPath = ""; }
  async function cleanup() {
    if (!canFlash || !stock || !supportedTarget) throw new Error("순정 이미지·실전 게이트·지원 기종 확인이 필요합니다");
    switched = null; resetPatch(); root = null; inventory = null; updatePlan = null; selection = emptyModuleSelection();
    switched = requireResult(await api.rootSwitchPrepare(serial, stock.path, target, true));
  }
  async function enterFastboot() {
    if (!canFlash) throw new Error("fastboot 실행 게이트가 꺼져 있습니다");
    requireResult(await api.rootReboot(serial, "fastboot")); await waitForFastboot(api);
  }
  async function restoreStock() {
    if (!stock || !switched || switched.stage !== "cleaned-awaiting-stock") throw new Error("모듈 정리 단계를 먼저 완료하세요");
    await enterFastboot(); await flashRootImage(api, serial, stock);
  }
  async function prepareNewEngine() {
    if (!newEngineAllowed || !stock || !supportedTarget || device.bootloader !== "unlocked") throw new Error("같은 버전 순정 복원과 언락 상태 확인이 필요합니다");
    patched = null; manager = null;
    if (target === "resukisu") {
      if (!tag) throw new Error("ReSukiSU 버전을 선택하세요");
      const prepared = requireResult(await api.rootPackagePrepare("resukisu", tag));
      requireResult(await api.resukisuInstall(serial, prepared.sha256, true));
      manager = prepared;
      log = [...log, "폰의 ReSukiSU 매니저에서 이 순정 IMG를 직접 패치한 뒤 결과를 PC로 복사하세요. 순정 IMG 경로: " + stock.path];
    } else {
      patched = await prepareMagiskImage(api, serial, stock);
    }
  }
  async function importPatched() {
    if (!stock || !samePhone || !newEngineAllowed) throw new Error("같은 폰의 순정 IMG 패치와 복원 단계를 확인하세요");
    patched = null;
    patched = requireResult(await api.rootExternalPatchImport(serial, stock.path, patchedPath, true));
  }
  async function applyPatched() {
    if (!canFlash || !patched || !newEngineAllowed) throw new Error("새 패치 이미지와 실전 게이트 확인이 필요합니다");
    const image = patched;
    // A failed/partial flash must not leave the same ready button armed for an immediate retry.
    patched = null; root = null; inventory = null; updatePlan = null;
    await enterFastboot(); await flashRootImage(api, serial, image);
  }
  function finishInstruction(completed: boolean) {
    const resolve = moduleInstructionResolve; moduleInstructionResolve = null; moduleInstruction = ""; resolve?.(completed);
  }
  async function installSets() {
    cancelled = false;
    const selected = JSON.parse(JSON.stringify(selection)) as RootModuleSelection;
    const check = () => { if (cancelled) throw new Error("모듈 세트 설치를 중단했습니다"); };
    await installModuleSets(api, serial, selected, {
      check,
      progress: (message, done, total) => { rootToolsState.label = `${done}/${total} · ${message}`; log = [...log.slice(-199), message]; },
      rebooted: (engine, previousBootId) => waitForModuleReboot(api, serial, engine, check, previousBootId),
      instruction: async message => {
        check(); moduleInstruction = message;
        const completed = await new Promise<boolean>(resolve => moduleInstructionResolve = resolve);
        check(); if (!completed) throw new Error("모듈 설정을 확인하지 않았습니다");
      },
    });
    await inspect();
  }
  async function inspectUpdate() {
    if (!root || !partition) throw new Error("루트 상태와 기종을 먼저 확인하세요");
    updatePlan = null;
    updatePlan = requireResult(await api.firmwareUpdateRootPlan({ root, unlocked: device.bootloader === "unknown" ? null : device.bootloader === "unlocked", intent: "preserve", partition, backupSelected: true }));
  }
</script>

<div class="flex-1 min-h-0 flex flex-col bg-background text-foreground overflow-hidden">
  <header class="h-14 shrink-0 border-b px-6 flex items-center gap-3">
    <Button variant="ghost" size="sm" disabled={busy} onclick={onClose}><ArrowLeft size={15} /> 돌아가기</Button>
    <ShieldCheck size={20} class="text-primary" /><h1 class="text-sm font-semibold">{section === "manager" ? "루팅 매니저 변경" : "루팅 모듈 세트 설치"}</h1>
    <span class="ml-auto text-xs text-muted-foreground">{device.productName} · {device.serialMasked}</span>
  </header>
  <div class="flex-1 min-h-0 flex gap-4 p-5 overflow-hidden">
    <main class="flex-1 min-w-0 overflow-y-auto space-y-4 pr-2">
      <div class="rounded-xl bg-warning-container text-warning px-4 py-3 flex gap-3">
        <TriangleAlert size={18} class="shrink-0" />
        <div class="space-y-1 text-xs"><p>{section === "manager" ? "엔진 전환은 모듈과 엔진 설정을 지웁니다. 먼저 백업하세요." : "선택한 세트와 의존 세트를 순서대로 설치합니다. 재부팅·폰 설정 확인이 필요합니다."}</p>
          <p>새 엔진·모듈 기능은 실기기 검증 전이며 기본 빌드에서는 설치와 기록이 비활성화됩니다.</p>
          <label class="flex gap-2 items-center"><input type="checkbox" bind:checked={riskAck} disabled={busy} />백업과 데이터·부팅 위험을 확인했습니다</label>
        </div>
      </div>
      <section class="rounded-xl bg-card elev-1 p-4 space-y-3">
        <div class="flex items-center gap-2"><ShieldCheck size={16} class="text-info" /><h2 class="text-sm font-semibold">현재 엔진 확인</h2></div>
        <p class="text-xs text-muted-foreground">폰 화면을 켜고 잠금을 푼 뒤 Shell 루트 요청을 허용하세요. 권한 거부는 순정 상태로 판정하지 않습니다.</p>
        <!-- 화면을 열면 자동으로 조회한다. 전환 기록·업데이트 점검 버튼은 매니저 변경이 일반 계획으로 바뀌어 없앴다(2026-10-09) -->
        <div class="flex gap-2 flex-wrap"><Button size="sm" variant="outline" disabled={busy || !serial} onclick={() => work("루트 상태 조회", inspect)}><RefreshCw size={14} />다시 조회</Button></div>
        {#if root}<p class="text-xs rounded-lg bg-muted p-2">권한: {root.access === "granted" ? "허용" : root.access === "denied" ? "거부됨" : root.access === "unavailable" ? "su 미감지" : "확인 불가"} · 엔진: {root.engine === "magisk" ? "Magisk" : root.engine === "kernelsu-family" ? "KernelSU 계열 · 세부 포크 미확정" : root.engine === "conflicting" ? "마커 충돌 · 진행 중단" : "확인 불가"}</p>{/if}
        {#if updatePlan}<p class="text-xs text-info">{updatePlan.action === "blocked" ? "현재 상태에서는 루팅 유지 업데이트를 제공할 수 없습니다." : "목표 버전의 새 이미지를 같은 폰에서 패치하고, 순정 업데이트 후 별도로 적용해야 합니다."} 업데이트 실행 연결과 기종별 검증이 남아 있습니다.</p>{/if}
      </section>
      {#if section === "manager"}<section class="rounded-xl bg-card elev-1 p-4 space-y-3">
        <h2 class="text-sm font-semibold">엔진 전환 · 순정 복원 후 새 엔진 설치</h2>
        <div class="flex items-center gap-3 text-xs">
          <label>새 엔진 <select class="rounded border bg-background p-2 ml-2" bind:value={target} disabled={busy || !!openSwitch} onchange={resetPatch}><option value="magisk">Magisk · 최신 stable</option><option value="resukisu">ReSukiSU · 버전 선택</option></select></label>
          <Button size="sm" variant="outline" disabled={busy} onclick={() => work("현재 버전 순정 이미지 준비", prepareStock)}>현재 버전 순정 IMG 준비</Button>
        </div>
        {#if !supportedTarget}<p class="text-xs text-warning">ReSukiSU 경로는 init_boot 기종만 준비됐습니다. 이 기종의 boot 패치는 별도 검증이 필요합니다.</p>{/if}
        {#if stock}<p class="text-xs break-all text-muted-foreground">순정 IMG: {stock.path}</p>{/if}
        {#if switched}<p class="text-xs bg-info-container text-info rounded-lg p-2">전환 단계: {switched.stage === "cleanup-intent" ? "정리 결과 불확정 · 수동 복원 필요" : switched.stage === "cleaned-awaiting-stock" ? "모듈 정리 완료 · 순정 복원 대기" : switched.stage === "stock-verified" ? "순정 부팅 검증 완료 · 새 엔진 설치 대기" : "완료"}</p>{/if}
        <p class="text-xs text-muted-foreground">기존 매니저 앱은 직접 제거하세요. 숨긴 Magisk 앱도 제거 대상입니다. 새 엔진에서 앱별 su 권한과 모듈 설정을 다시 구성해야 합니다.</p>
        <div class="grid grid-cols-3 gap-2">
          <Button size="sm" variant="destructive" disabled={busy || !canFlash || !stock || !supportedTarget || device.bootloader !== "unlocked" || !!openSwitch || root?.access !== "granted"} onclick={() => work("1. 모듈·엔진 정리", cleanup, true)}>1. 모듈·엔진 정리</Button>
          <Button size="sm" disabled={busy || !canFlash || !stock || switched?.stage !== "cleaned-awaiting-stock"} onclick={() => work("2. 양 슬롯 순정 복원", restoreStock, true)}>2. 순정 복원·재부팅</Button>
          <Button size="sm" variant="outline" disabled={busy || switched?.stage !== "cleaned-awaiting-stock"} onclick={() => work("3. 순정 부팅 검증", async () => { switched = requireResult(await api.rootSwitchStatus(serial, true)); await inspect(); })}>3. OS 복귀 후 검증</Button>
        </div>
        {#if target === "resukisu"}
          <div class="flex gap-2 items-center text-xs"><Button size="sm" variant="outline" disabled={busy} onclick={() => work("ReSukiSU 버전 목록", async () => { releases = requireResult(await api.resukisuReleases()); tag = ""; resetPatch(); })}>버전 목록 조회</Button>
            <select aria-label="ReSukiSU 버전" class="border rounded bg-background p-2" bind:value={tag} disabled={busy} onchange={resetPatch}><option value="">버전을 선택하세요</option>{#each releases as r}<option value={r.tag}>{r.tag}{r.prerelease ? " · RC/사전 버전" : ""} · {r.publishedAt.slice(0, 10)}</option>{/each}</select></div>
          <p class="text-xs text-warning">사전 버전은 기종별 검증이 필요합니다. 패치는 폰의 매니저에서 직접 수행합니다.</p>
        {/if}
        <Button size="sm" disabled={busy || !canWrite || !stock || !newEngineAllowed || !supportedTarget || device.bootloader !== "unlocked" || (target === "resukisu" && !tag)} onclick={() => work("4. 새 엔진 이미지 준비", prepareNewEngine, true)}>{target === "resukisu" ? "4. 선택 버전 매니저 설치" : "4. Magisk 자동 패치·매니저 설치"}</Button>
        {#if target === "resukisu" && manager}
          <div class="space-y-2 text-xs rounded-lg bg-muted p-3"><p>위 순정 IMG를 같은 폰으로 복사 → ReSukiSU 매니저에서 패치 → 결과 IMG를 PC로 복사하세요.</p>
            <label class="block">패치 결과 경로 <input class="w-full rounded border bg-background p-2 mt-1" bind:value={patchedPath} disabled={busy} /></label>
            <label class="flex gap-2"><input type="checkbox" bind:checked={samePhone} disabled={busy} />같은 폰·같은 순정 IMG를 직접 패치했습니다</label>
            <Button size="sm" variant="outline" disabled={busy || !canWrite || !samePhone || !patchedPath} onclick={() => work("외부 패치 이미지 검증", importPatched, true)}>이미지 검사·가져오기</Button>
          </div>
        {/if}
        <div class="flex gap-2"><Button size="sm" variant="destructive" disabled={busy || !canFlash || !patched || !newEngineAllowed} onclick={() => work("5. 새 패치 IMG 양 슬롯 기록", applyPatched, true)}>5. 새 패치 적용·재부팅</Button>
          <Button size="sm" variant="outline" disabled={busy || switched?.stage !== "stock-verified"} onclick={() => work("6. 엔진 전환 최종 확인", async () => { switched = requireResult(await api.rootSwitchFinish(serial)); await inspect(); })}>6. OS 복귀 후 루트 재승인·검증</Button></div>
      </section>{:else}<section class="rounded-xl bg-card elev-1 p-4 space-y-3">
        <div class="flex gap-2 items-center"><Package size={16} class="text-primary" /><h2 class="text-sm font-semibold">모듈 · 한 번에 하나씩 설치 후 재부팅</h2></div>
        <ModuleSetSelector {selection} onChange={value => selection = value} disabled={busy || !!openSwitch} engine={root?.engine ?? "unknown"} />
        <Button size="sm" disabled={busy || !canWrite || !!openSwitch || !inventory || inventory.rebootRequired || inventory.uncertain || selectedModuleSets(selection).length === 0} onclick={() => work("모듈 세트 설치", installSets, true)}>선택한 세트 설치</Button>
        {#if moduleInstruction}<ModuleInstruction message={moduleInstruction} onComplete={() => finishInstruction(true)} onCancel={() => { cancelled = true; finishInstruction(false); }} />{:else if busy}<Button size="sm" variant="outline" onclick={() => cancelled = true}>현재 작업 종료 후 세트 설치 중단</Button>{/if}
        {#if inventory?.rebootRequired}<p class="text-xs text-warning">이전 모듈 설치·변경 이후 재부팅이 필요합니다. 재부팅 후 권한·엔진·모듈 조회로 확인하세요.</p>{/if}
        {#if inventory?.uncertain}<div class="space-y-2"><p class="text-xs text-destructive">설치 결과를 확인할 수 없습니다. 모듈을 점검하고 필요한 경우 비활성화·제거하세요. 자동 재시도하지 않습니다.</p><Button size="sm" variant="outline" disabled={busy || !riskAck} onclick={() => work("재부팅 후 모듈 기록 재확인", async () => { inventory = requireResult(await api.rootModuleReconcile(serial, true)); })}>재부팅 후 오류·모듈 목록을 검토했습니다</Button></div>{/if}
        {#if inventory}<div class="max-h-48 overflow-y-auto rounded-lg bg-muted text-xs">{#each inventory.modules as m}<div class="flex items-center gap-2 p-2 border-b"><span class="flex-1">{m.id} · {m.state}</span><Button size="sm" variant="outline" disabled={busy || !canWrite} onclick={() => work("모듈 비활성화", async () => { requireResult(await api.rootModuleAction(serial, m.id, "disable", true)); await inspect(); }, true)}>끄기</Button><Button size="sm" variant="destructive" disabled={busy || !canWrite} onclick={() => work("모듈 제거 예약", async () => { requireResult(await api.rootModuleAction(serial, m.id, "remove", true)); await inspect(); }, true)}>제거</Button></div>{/each}</div>{/if}
        <!-- 설치는 모듈마다 자동으로 재부팅한다 — 끄기·제거 예약처럼 재부팅이 남았을 때만 버튼을 보인다.
             HMA 프리셋은 설치 중 폰 Download에 자동으로 넣고, WebUI는 TrickyAddon이 스스로 설치한다(2026-10-09 정리) -->
        {#if inventory?.rebootRequired}<div><Button size="sm" variant="outline" disabled={busy || !canWrite} onclick={() => work("모듈 변경 후 OS 재부팅", async () => { requireResult(await api.rootReboot(serial, "os")); inventory = null; }, true)}><RefreshCw size={14} /> 재부팅해서 적용</Button></div>{/if}
      </section>{/if}
    </main>
    <aside class="w-72 shrink-0 flex flex-col rounded-xl bg-muted elev-1 overflow-hidden">
      <h2 class="text-xs font-semibold p-3 border-b">진행 기록</h2>
      <div aria-live="polite" class="flex-1 overflow-y-auto p-3 space-y-2 text-xs break-words">{#each log as line}<p>{line}</p>{/each}</div>
      {#if error}<p role="alert" class="shrink-0 bg-danger-container text-destructive p-3 text-xs break-words">{error}</p>{/if}
      {#if busy}<div class="shrink-0 p-3 text-xs flex gap-2 items-center"><Loader2 size={14} class="animate-spin" />{rootToolsState.label}</div>{/if}
    </aside>
  </div>
</div>
