<script lang="ts">
  import Modal from "$lib/components/Modal.svelte";
  import CommunicationPanel from "$lib/components/CommunicationPanel.svelte";
  import { Button } from "$lib/components/ui/button";
  import { Card, CardContent, CardHeader, CardTitle } from "$lib/components/ui/card";
  import { Switch } from "$lib/components/ui/switch";
  import { Label } from "$lib/components/ui/label";
  import { Alert, AlertDescription, AlertTitle } from "$lib/components/ui/alert";
  import { Play, Pause, Square, Usb, ChevronsRight, ExternalLink, CircleCheck, OctagonX, CircleHelp, LoaderCircle } from "@lucide/svelte/icons";
  import { wizard } from "$lib/stores/wizard.svelte";
  import { api } from "$lib/api";
  import { LINKS, maskImei } from "$lib/data/links";

  let imeiCopied = $state(false);
  let copiedTimer: ReturnType<typeof setTimeout> | undefined;
  async function copyImei() {
    if (!wizard.imei1) return;
    imeiCopied = await api.copyText(wizard.imei1);
    // 연속 복사 시 이전 타이머가 새 표시를 일찍 지우지 않게 다시 건다
    clearTimeout(copiedTimer);
    if (imeiCopied) copiedTimer = setTimeout(() => (imeiCopied = false), 2500);
  }
  $effect(() => () => clearTimeout(copiedTimer));
  const mb = (b: number) => `${(b / 1024 ** 2).toFixed(1)} MB`;
  import BackupNotice from "$lib/components/BackupNotice.svelte";
  import GuideSlides from "$lib/components/GuideSlides.svelte";
  import { GUIDES } from "$lib/data/guides";
  import { SIMULATED_RUN } from "$lib/data/runMode";
  import { Checkbox } from "$lib/components/ui/checkbox";

  async function pickFirmware() {
    const dir = await api.pickFolder();
    if (dir) void wizard.setFirmwareDir(dir);
  }
  async function pickFirmwareDest() {
    const dir = await api.pickFolder();
    if (dir) wizard.firmwareDest = dir;
  }

  let consoleEl: HTMLDivElement | undefined = $state();

  // 단계별 로그를 합치지 않고 그대로 그린다 — 줄마다 전체 로그를 다시 복사하지 않도록 단계별 시작 번호만 계산
  const logOffsets = $derived.by(() => {
    let n = 0;
    return wizard.runSteps.map((s) => {
      const start = n;
      n += s.logs.length;
      return start;
    });
  });
  const logCount = $derived(wizard.runSteps.reduce((n, s) => n + s.logs.length, 0));

  $effect(() => {
    logCount;
    if (consoleEl) consoleEl.scrollTop = consoleEl.scrollHeight;
  });

  const lineColor = (t: string) =>
    t.startsWith("[오류]") ? "text-red-400"
    : t.startsWith("[완료]") || t.startsWith("[재개]") ? "text-emerald-400"
    : t.startsWith("[대기]") ? "text-amber-400"
    : t.startsWith("[시작]") ? "text-sky-400"
    : "text-zinc-400";

  const overallPct = $derived(Math.round(wizard.overall * 100));
  const currentStep = $derived(wizard.runSteps.find((s) => s.status === "running" || s.status === "manual-wait"));
  const currentFailed = $derived(wizard.runSteps.find((s) => s.status === "failed"));
</script>

<div class="flex-1 min-h-0 flex flex-col gap-3 p-4 lg:p-6">
  {#if wizard.stepError}
    <Alert variant="destructive" class="shrink-0">
      <OctagonX size={16} />
      <AlertTitle>{currentFailed?.title ?? "단계"} 단계가 실패했습니다</AlertTitle>
      <AlertDescription class="flex flex-col gap-2">
        <span>{wizard.stepError} — 다음 단계(리락 포함)로 넘어가지 않습니다.</span>
        <div class="flex gap-2">
          <Button size="sm" onclick={() => wizard.retryStep()}>이 단계 다시 시도</Button>
          <Button size="sm" variant="outline" onclick={() => wizard.abort()}>중단</Button>
          {#if wizard.corruptRelockHistory}
            <Button size="sm" variant="outline" disabled={wizard.busy > 0} onclick={() => wizard.archiveFlashHistory()}>손상 이력 보관 후 순정 복원부터 재검사</Button>
          {/if}
        </div>
      </AlertDescription>
    </Alert>
  {/if}
  {#if wizard.usbError}
    <Alert variant="destructive" class="shrink-0">
      <Usb size={16} />
      <AlertTitle>USB 연결이 불안정합니다</AlertTitle>
      <AlertDescription class="flex flex-col gap-2">
        <span>
          {wizard.usbErrorCount === 1
            ? "같은 포트에 다시 연결하거나, 다른 포트(본체 뒷면 권장)로 바꿔 꽂아주세요."
            : "반복 실패 — 케이블 교체나 허브 제거를 권장합니다."}
          작업 상태는 보존되어 재연결 시 이어서 진행됩니다.
        </span>
        <div>
          <Button size="sm" onclick={() => wizard.dismissUsbError()}>재연결 완료 — 이어서 진행</Button>
        </div>
      </AlertDescription>
    </Alert>
  {/if}

  <!-- 상단: 현재 단계 + 전체 진행률 + 컨트롤 -->
  <Card class="elev-1 shrink-0">
    <CardContent class="py-3 space-y-2">
      <div class="flex items-center justify-between gap-4">
        <div class="min-w-0">
          <div class="text-sm font-semibold truncate">
            {currentStep?.title ?? (wizard.finished ? "완료" : "대기 중")}
          </div>
          <!-- 단계 사이(진행 중 단계가 잠깐 없는 순간)에도 줄을 남겨 카드 높이가 흔들리지 않게 한다 -->
          <div class="text-[11px] text-muted-foreground {currentStep ? '' : 'invisible'}">{Math.round((currentStep?.progress ?? 0) * 100)}%</div>
        </div>
        <div class="flex items-center gap-2 shrink-0">
          {#if wizard.simulationControlsVisible}
          <div class="flex items-center gap-1.5 mr-1">
            <Switch id="sim-err" checked={wizard.simulateUsbError} onCheckedChange={(v: boolean) => (wizard.simulateUsbError = v)} />
            <Label for="sim-err" class="text-[11px] text-muted-foreground cursor-pointer">USB 오류 시뮬</Label>
          </div>
          <div class="flex items-center gap-1.5 mr-1">
            <Switch id="sim-efs" checked={wizard.simulateEfsFail} onCheckedChange={(v: boolean) => (wizard.simulateEfsFail = v)} />
            <Label for="sim-efs" class="text-[11px] text-muted-foreground cursor-pointer">EFS 실패 시뮬</Label>
          </div>
          {/if}
          {#if !wizard.finished}
            {#if wizard.running}
              <Button size="sm" variant="outline" onclick={() => wizard.pause()}><Pause size={13} class="mr-1" />일시정지</Button>
            {:else if wizard.busy === 0}
              <!-- 기기 작업(엔진·완결 게이트·펌웨어 받기)이 진행 중이면 [이어서]를 두지 않는다 -->

              <Button size="sm" disabled={wizard.usbError || !!wizard.stepError} onclick={() => wizard.resumeRun()}><Play size={13} class="mr-1" />{wizard.runSteps.some((s) => s.status !== "pending") ? "이어서" : "실행"}</Button>
            {/if}
            <Button size="sm" variant="destructive" onclick={() => wizard.abort()}><Square size={12} class="mr-1" />중단</Button>
          {:else}
            <Button size="sm" onclick={() => wizard.goFinish()}>다음 단계 →</Button>
          {/if}
        </div>
      </div>
      <div class="h-2.5 rounded-full bg-muted overflow-hidden">
        <div class="h-full grad-hero transition-all" style="width: {overallPct}%"></div>
      </div>
    </CardContent>
  </Card>

  <!-- 콘솔 -->
  <Card class="elev-1 flex-1 min-h-0 flex flex-col overflow-hidden">
    <CardHeader class="py-3 pb-2 shrink-0 border-b flex-row items-center justify-between">
      <CardTitle class="text-xs font-semibold text-muted-foreground tracking-wide">로그</CardTitle>
    </CardHeader>
    <CardContent class="flex-1 p-0 min-h-0">
      <div bind:this={consoleEl} class="console-bg h-full overflow-y-auto px-4 py-3 font-mono text-[11.5px] leading-relaxed">
        {#each wizard.runSteps as s, si (s.id)}
          {#each s.logs as text, j (j)}
            <div class="flex gap-2">
              <span class="text-zinc-600 shrink-0 select-none">{String(logOffsets[si] + j + 1).padStart(3, "0")}</span>
              <span class="text-zinc-500 shrink-0 hidden md:inline">[{s.title}]</span>
              <span class={lineColor(text)}>{text}</span>
            </div>
          {/each}
        {/each}
        <!-- 진행 표시는 자리를 유지한 채 보이기만 바꾼다(단계 전환마다 로그 높이가 바뀌어 스크롤이 튀지 않게) -->
        <div class="flex gap-2 text-primary {wizard.running && !wizard.manualCurrent ? '' : 'invisible'}">
          <ChevronsRight size={13} class="animate-pulse" />
        </div>
      </div>
    </CardContent>
  </Card>
</div>

<!-- 수동 개입 모달 (백업 직전 안내는 전용 화면) -->
{#if wizard.manualCurrent?.id === "backup-notice"}
  <BackupNotice />
{:else if wizard.manualCurrent}
  {@const guide = GUIDES[wizard.manualCurrent.id]}
  <Modal title={wizard.manualCurrent.title} onClose={() => { if (wizard.busy === 0 && !wizard.runInDanger) wizard.abort(); }} class="fixed inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-sm p-4">
    <Card class="w-full max-w-lg elev-3 max-h-[calc(100vh-2rem)] flex flex-col">
      <CardHeader class="shrink-0">
        <CardTitle class="text-base">{wizard.manualCurrent.title}</CardTitle>
      </CardHeader>
      <CardContent class="space-y-4 overflow-y-auto min-h-0">
        {#if guide}
          <!-- 순서대로 따라 하는 조작: 그림 카드 넘김 -->
          {#key wizard.manualCurrent.id}<GuideSlides slides={guide} />{/key}
        {:else if wizard.manualCurrent.steps.length > 0}
          <ol class="space-y-2.5">
            {#each wizard.manualCurrent.steps as s, i (i)}
              <li class="flex items-start gap-3 rounded-lg bg-muted/60 px-3 py-2.5 text-sm">
                <span class="flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-primary text-primary-foreground text-[11px] font-bold">{i + 1}</span>
                {s}
              </li>
            {/each}
          </ol>
        {/if}
        {#if wizard.manualCurrent.id === "smsie-export"}
          <label class="flex items-start gap-2 rounded-lg border p-3 text-xs">
            <Checkbox checked={wizard.smsieExportAck} onCheckedChange={(v) => { wizard.smsieExportAck = v === true; }} />
            폰 앱에서 선택한 문자·통화 기록 모두의 내보내기 성공 안내를 확인했습니다
          </label>
          <p class="text-xs text-muted-foreground">파일 검사 후 [확인하고 진행]을 누르면 PC 백업을 완료 처리하고 폰의 임시 사본을 정리합니다.</p>
        {/if}
        {#if wizard.manualCurrent.id === "oem-toggle" && wizard.device}
          {@const p = wizard.device.prep}
          <div class="rounded-lg border divide-y">
            {#each [["개발자 옵션", p.developerOptions], ["USB 디버깅", p.usbDebugging], ["OEM 잠금 해제", p.oemUnlockAllowed]] as [label, on] (label)}
              <div class="flex items-center gap-2.5 px-3 py-2 text-sm">
                {#if on === true}
                  <CircleCheck size={15} class="text-success shrink-0" />
                {:else if on === false}
                  <OctagonX size={15} class="text-destructive shrink-0" />
                {:else}
                  <CircleHelp size={15} class="text-muted-foreground shrink-0" />
                {/if}
                <span class="flex-1">{label}</span>
                <span class="text-[11px] {on === true ? 'text-success' : on === false ? 'text-destructive' : 'text-muted-foreground'}">
                  {on === true ? "켜짐" : on === false ? "꺼짐" : "확인 불가 — 폰에서 직접 확인"}
                </span>
              </div>
            {/each}
          </div>
          {#if wizard.prepUnknown.length > 0}
            <label class="flex items-start gap-2.5 rounded-lg border px-3 py-2.5 cursor-pointer {wizard.oemUnknownAck ? 'border-primary/40 bg-primary/5' : ''}">
              <Checkbox class="mt-0.5" checked={wizard.oemUnknownAck} onCheckedChange={(v: boolean | "indeterminate") => (wizard.oemUnknownAck = v === true)} />
              <span class="text-[12.5px]">
                {wizard.prepUnknown.join(", ")}을(를) 폰에서 직접 켜 두었습니다
                <span class="block text-[11px] text-muted-foreground">켜져 있지 않으면 언락 단계에서 거부되어 멈춥니다 — 폰에는 영향이 없습니다</span>
              </span>
            </label>
          {/if}
          <div class="flex gap-2">
            <Button variant="outline" size="sm" onclick={() => wizard.openPhoneSettings()}>폰에서 설정 화면 열기</Button>
            <Button variant="outline" size="sm" disabled={wizard.prepChecking} onclick={() => wizard.recheckPrep()}>
              {#if wizard.prepChecking}<LoaderCircle size={13} class="mr-1 animate-spin" />{/if}다시 확인
            </Button>
          </div>
        {/if}
        {#if wizard.manualCurrent.input === "unlock-code"}
          <div class="space-y-2">
            <!-- IMEI 1: 기기에서 읽어 마스킹 표시, 복사 버튼으로만 전체 값 사용 -->
            <div class="flex items-center gap-2 rounded-lg border px-3 py-2">
              <span class="text-[12px] text-muted-foreground shrink-0">IMEI 1</span>
              <span class="flex-1 font-mono text-[13px]">
                {#if wizard.imeiState === "loading"}
                  <LoaderCircle size={13} class="inline animate-spin text-primary" />
                {:else if wizard.imei1}
                  {maskImei(wizard.imei1)}
                {:else}
                  <span class="text-[11px] text-muted-foreground">읽을 수 없습니다 — 설정 &gt; 휴대전화 정보 &gt; IMEI(SIM 슬롯 1)에서 확인해 주세요</span>
                {/if}
              </span>
              <Button variant="outline" size="sm" class="h-7 shrink-0" disabled={!wizard.imei1} onclick={copyImei}>
                {imeiCopied ? "복사됨" : "IMEI 복사"}
              </Button>
            </div>
            <Button variant="ghost" size="sm" class="h-7 px-2 text-[12px]" onclick={() => api.openExternal(LINKS.unlock)}>
              <ExternalLink size={13} class="mr-1" />발급 페이지 다시 열기
            </Button>
            <input
              type="password"
              autocomplete="off"
              spellcheck="false"
              class="w-full rounded-lg border bg-background px-3 py-2 font-mono text-[13px] outline-none focus:ring-1 focus:ring-ring"
              placeholder="언락 코드 붙여넣기"
              bind:value={wizard.unlockCode}
            />
            {#if wizard.unlockCode.trim() && !wizard.unlockCodeValid}
              <p class="text-[11px] text-destructive">언락 코드는 16자리 영문·숫자(0-9, A-F)입니다</p>
            {/if}
          </div>
        {:else if wizard.manualCurrent.input === "firmware"}
          <!-- 자동 다운로드가 실패했을 때만 열림 — 원인별 안내 -->
          {@const fwVersion = wizard.updateVersion ?? wizard.device?.firmware ?? ""}
          {#if wizard.firmwareFail === "space"}
            <div class="flex items-start gap-2 rounded-lg border border-destructive/40 bg-destructive/5 px-3 py-2.5 text-[12.5px]">
              <OctagonX size={14} class="text-destructive shrink-0 mt-0.5" />
              <div class="min-w-0 space-y-0.5">
                <div class="font-medium">저장 공간이 부족해 순정 펌웨어를 받지 못했습니다</div>
                <div class="text-[11px] text-muted-foreground break-all">{wizard.firmwareError}</div>
              </div>
            </div>
            <p class="text-[12px] text-muted-foreground">여유 공간이 있는 다른 저장 위치를 고르면 그곳에 다시 받습니다.</p>
            <div class="flex items-center gap-2">
              <div class="flex-1 min-w-0 rounded-lg border bg-background px-3 py-1.5 font-mono text-[12px] truncate">
                {wizard.firmwareDest || "저장 위치를 선택해 주세요"}
              </div>
              <Button variant="outline" size="sm" class="shrink-0" onclick={pickFirmwareDest}>위치 선택</Button>
            </div>
            <Button size="sm" class="w-full" disabled={!wizard.firmwareDest || wizard.firmwareState === "loading"} onclick={() => wizard.retryFirmware()}>
              {#if wizard.firmwareState === "loading"}<LoaderCircle size={13} class="mr-1 animate-spin" />받는 중…{:else}이 위치로 다시 받기{/if}
            </Button>
          {:else}
            <div class="flex items-start gap-2 rounded-lg border border-destructive/40 bg-destructive/5 px-3 py-2.5 text-[12.5px]">
              <OctagonX size={14} class="text-destructive shrink-0 mt-0.5" />
              <div class="min-w-0 flex-1 space-y-0.5">
                <div class="font-medium">Sony 서버에서 순정 펌웨어를 자동으로 받지 못했습니다</div>
                <div class="text-[11px] text-muted-foreground break-all">{wizard.firmwareError}</div>
              </div>
              <Button variant="outline" size="sm" class="h-7 shrink-0" disabled={wizard.firmwareState === "loading"} onclick={() => wizard.retryFirmware()}>
                {#if wizard.firmwareState === "loading"}<LoaderCircle size={13} class="mr-1 animate-spin" />{/if}다시 시도
              </Button>
            </div>
            <ol class="space-y-2.5">
              {#each [
                `XperiFirm에서 ${wizard.device?.model ?? "기종"} → 지역/통신사 → ${fwVersion ? `버전 ${fwVersion}` : "설치된 버전과 같은 버전"}을 받습니다`,
                `받은 펌웨어 폴더를 아래에서 지정합니다${wizard.partition ? ` — 폴더 안에 ${wizard.partition}_*.sin 파일이 있어야 합니다` : ""}`,
              ] as t, n (n)}
                <li class="flex items-start gap-3 rounded-lg bg-muted/60 px-3 py-2.5 text-sm">
                  <span class="flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-primary text-primary-foreground text-[11px] font-bold">{n + 1}</span>
                  {t}
                </li>
              {/each}
            </ol>
            <div class="flex items-center gap-2">
              <div class="flex-1 min-w-0 rounded-lg border bg-background px-3 py-1.5 font-mono text-[12px] truncate">
                {wizard.firmwareDir || "펌웨어 폴더를 선택해 주세요"}
              </div>
              <Button variant="outline" size="sm" class="shrink-0" onclick={pickFirmware}>폴더 선택</Button>
            </div>
            {#if wizard.firmwareDirState === "loading"}
              <div class="flex items-center gap-2 text-[12px] text-muted-foreground">
                <LoaderCircle size={13} class="animate-spin text-primary shrink-0" />폴더 검사 중…
              </div>
            {:else if wizard.firmwareDirInfo}
              <div class="flex items-center gap-2 text-[12px] text-success">
                <CircleCheck size={13} class="shrink-0" />{wizard.firmwareDirInfo.file} · 부트 이미지 {mb(wizard.firmwareDirInfo.imageBytes)} 확인
              </div>
            {:else if wizard.firmwareDirState === "failed"}
              <div class="flex items-start gap-2 text-[12px] text-destructive">
                <OctagonX size={13} class="shrink-0 mt-0.5" />{wizard.firmwareDirError}
              </div>
            {/if}
          {/if}
        {/if}
        {#if wizard.manualCurrent.id === "ims-precheck" || wizard.manualCurrent.id === "ims-check"}
          <CommunicationPanel snapshot={wizard.communicationLatest} loading={wizard.communicationLoading} error={wizard.communicationError} slots={wizard.communicationSlots} calls={wizard.callChecks} showCalls onCallChange={(slot, item, checked) => wizard.setCallCheck(slot, item, checked)} />
        {/if}
        {#if wizard.manualWatching}
          <div class="flex items-center gap-2 rounded-lg bg-primary/5 px-3 py-2 text-[12px] text-primary">
            <LoaderCircle size={13} class="animate-spin shrink-0" />
            {wizard.manualCurrent.id === "ims-check" || wizard.manualCurrent.id === "ims-precheck"
              ? wizard.imsRegistered ? "IMS 등록 확인됨 — 통화 확인 여부를 선택하고 마무리하세요" : "IMS 등록 확인 중 — SIM 없이도 통신 확인을 생략하고 마무리할 수 있습니다"
              : `${wizard.manualWatching} 자동 감지 중 — 감지되면 바로 다음 단계로 진행합니다`}
          </div>
        {/if}
        {#if wizard.manualCheckError}
          <div class="flex items-start gap-2 rounded-lg bg-danger-container/60 px-3 py-2 text-[12px] text-destructive">
            <OctagonX size={13} class="shrink-0 mt-0.5" />{wizard.manualCheckError}
          </div>
        {/if}
        <div class="flex items-center justify-between gap-3">
          <span class="text-[11px] text-muted-foreground">
            {wizard.manualCurrent.input
              ? "입력을 마치면 다음 단계로 진행됩니다"
              : wizard.manualVerifiable
                ? wizard.manualWatching
                  ? wizard.manualCurrent.id === "ims-check" || wizard.manualCurrent.id === "ims-precheck" ? "파일 기록과 실제 통신은 별도로 확인합니다" : "감지되면 자동으로 진행합니다 — [확인하고 진행]으로 바로 확인할 수도 있습니다"
                  : "폰에서 마친 뒤 [확인하고 진행]을 누르면 확인 후 진행합니다"
                : "완료하면 다음 단계로 진행됩니다"}
          </span>
          <div class="flex shrink-0 gap-2">
            {#if wizard.manualSetupState === "failed" && wizard.manualCurrent.id === "smsie-export"}
              <Button variant="outline" onclick={() => wizard.smsiePrepare()}>준비 다시 시도</Button>
            {/if}
            {#if wizard.manualSetupState === "failed" && wizard.manualCurrent.id === "smsie-import"}
              <Button variant="outline" onclick={() => wizard.smsieRestoreStage()}>준비 다시 시도</Button>
            {/if}
            {#if wizard.manualSkippable}
              <Button variant="ghost" class="text-muted-foreground" onclick={() => wizard.skipManual()}>(목업) 건너뛰기</Button>
            {/if}
            {#if wizard.manualCurrent.id === "ims-precheck" && wizard.manualCheckError}
              <Button variant="outline" onclick={() => wizard.repatch()}>다시 패치</Button>
            {/if}
            {#if wizard.manualCurrent.id === "ims-check" || wizard.manualCurrent.id === "ims-precheck"}
              <Button variant="outline" disabled={wizard.manualChecking} onclick={() => wizard.finishWithoutIms()}>{wizard.manualCurrent.id === "ims-check" ? "통신 확인 없이 마무리" : "통신 확인 생략하고 계속"}</Button>
            {/if}
            <Button disabled={!wizard.manualInputReady || wizard.manualChecking} onclick={() => wizard.confirmManual()}>
              {#if wizard.manualChecking}<LoaderCircle size={14} class="mr-1 animate-spin" />확인 중…{:else}{wizard.manualCurrent.input ? "입력 완료" : wizard.manualVerifiable ? "확인하고 진행" : "다음"}{/if}
            </Button>
          </div>
        </div>
      </CardContent>
    </Card>
  </Modal>
{/if}
