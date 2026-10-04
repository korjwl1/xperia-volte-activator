<script lang="ts">
  import { onMount } from "svelte";
  import { CircleCheck, TriangleAlert, Usb, Smartphone, ArrowRight, Lock, LockOpen } from "@lucide/svelte/icons";
  import { api } from "$lib/api";
  import { wizard } from "$lib/stores/wizard.svelte";
  import { simStateLabel, type DeviceStatus } from "$lib/types";

  const CAFE_URL = "https://cafe.naver.com/x1smart";
  const GITHUB_URL = "https://github.com/korjwl1";

  let devices = $state<DeviceStatus[]>([]);
  let loading = $state(true);
  let linkError = $state<string | null>(null); // 연결 수단 점검 실패 — 재시도 팝업 표시
  let linkDismissed = $state(false);
  let pollTimer: ReturnType<typeof setInterval> | undefined;

  // 준비 완료(device) 1대일 때만 작업 가능 — 미승인/오프라인은 안내 화면
  const single = $derived(devices.length === 1 ? devices[0] : null);
  const device = $derived(single?.state === "device" ? single : null);
  const pending = $derived(single && single.state !== "device" ? single : null);
  const multiDevice = $derived(devices.length > 1);
  const showLinkPopup = $derived(linkError !== null && !linkDismissed);

  let inFlight = false; // 이전 조회가 끝나기 전 다음 폴링이 겹치지 않도록
  let alive = true; // 화면을 떠난 뒤 도착한 조회 결과는 버린다
  let failStreak = 0; // 일시적 조회 실패 1회로 기기 카드가 사라지지 않도록

  async function refresh() {
    if (inFlight) return;
    inFlight = true;
    try {
      const list = await api.deviceList();
      if (!alive || wizard.view !== "device") return;
      if (list === null) {
        failStreak++;
        if (failStreak < 2) return;
      } else {
        failStreak = 0;
      }
      devices = list ?? [];
      wizard.device = devices.length === 1 && devices[0].state === "device" ? devices[0] : null;
      const st = await api.adbStatus();
      if (!alive) return;
      linkError = st && !st.available ? (st.detail ?? "기기 연결 기능을 사용할 수 없습니다") : null;
    } finally {
      inFlight = false;
    }
  }

  function retryLink() {
    linkDismissed = false;
    refresh();
  }

  onMount(() => {
    alive = true;
    void refresh().finally(() => {
      if (alive) loading = false;
    });
    // 폴링은 첫 조회 결과와 무관하게 바로 건다 — 첫 조회가 오래 걸려도 이후 조회가 막히지 않는다(겹침은 inFlight가 막음)
    pollTimer = setInterval(refresh, 3000);
    return () => {
      alive = false;
      if (pollTimer) clearInterval(pollTimer);
    };
  });

  function start() {
    wizard.startSession();
  }
</script>

<div class="flex-1 flex flex-col overflow-hidden">
  {#if loading}
    <div class="flex-1 flex items-center justify-center text-muted-foreground">확인 중…</div>

  {:else if multiDevice}
    <div class="flex-1 flex flex-col items-center justify-center p-8">
      <div class="w-full max-w-xl rounded-xl border-2 border-destructive/40 bg-danger-container/60 elev-2 p-8 space-y-4">
        <div class="flex items-center gap-3">
          <span class="flex h-12 w-12 items-center justify-center rounded-xl bg-destructive text-destructive-foreground">
            <TriangleAlert size={24} />
          </span>
          <div>
            <h2 class="text-lg font-semibold">여러 대의 기기가 연결되어 있습니다</h2>
            <p class="text-sm text-muted-foreground">하나의 기기만 연결해 주세요</p>
          </div>
        </div>
        <div class="space-y-2">
          {#each devices as d (d.serial ?? d.serialMasked)}
            <div class="flex items-center gap-3 rounded-lg border bg-card px-4 py-3">
              <Smartphone size={18} class="text-muted-foreground shrink-0" />
              <span class="text-sm font-medium">{d.productName}</span>
              <span class="font-mono text-xs text-muted-foreground">{d.serialMasked}</span>
            </div>
          {/each}
        </div>
      </div>
    </div>

  {:else if device}
    <div class="flex-1 grad-hero relative flex items-center justify-center overflow-hidden">
      <!-- 중앙 콘텐츠 — 전체 화면 기준 중앙 -->
      <div class="w-full max-w-3xl px-8 py-8 flex flex-col items-center gap-8 text-primary-foreground">
          <!-- 1행: 기기 정보 + 이미지 -->
          <div class="w-full flex items-center justify-between gap-10">
            <div class="flex-1 min-w-0 space-y-3">
              <div>
                <div class="text-[11px] uppercase tracking-widest opacity-70">연결된 기기</div>
                <h1 class="text-3xl lg:text-4xl font-bold tracking-tight mt-1">{device.productName}</h1>
              </div>
              <div class="flex flex-wrap items-center gap-2">
                <span class="rounded-md bg-white/15 px-2.5 py-1 font-mono text-sm">{device.model}</span>
                <span class="rounded-md bg-white/15 px-2.5 py-1 font-mono text-sm">{device.serialMasked}</span>
              </div>
              <p class="text-sm opacity-80">{device.firmware} · Android {device.android}</p>
              <div class="flex flex-wrap gap-2 pt-1">
                <div class="rounded-lg bg-white/15 px-3 py-1.5 text-xs font-medium">
                  {#if device.bootloader === "locked"}<Lock size={12} class="inline mr-1" />부트로더 잠김{:else if device.bootloader === "unlocked"}<LockOpen size={12} class="inline mr-1" />언락{:else}부트로더 확인 불가{/if}
                </div>
                <div class="rounded-lg bg-white/15 px-3 py-1.5 text-xs font-medium">
                  {#if device.rooted === true}루팅됨{:else if device.rooted === false}루팅 미감지{:else}루팅 확인 불가{/if}
                </div>
              </div>
            </div>

            <svg viewBox="0 0 71 165" class="w-[80px] lg:w-[100px] shrink-0 drop-shadow-xl" aria-hidden="true">
              <defs>
                <linearGradient id="scr" x1="0" y1="0" x2="1" y2="1">
                  <stop offset="0" stop-color="rgba(255,255,255,0.25)" />
                  <stop offset="0.5" stop-color="rgba(255,255,255,0.10)" />
                  <stop offset="1" stop-color="rgba(255,255,255,0.04)" />
                </linearGradient>
              </defs>
              <rect x="0.5" y="0.5" width="70" height="164" rx="10" fill="rgba(0,0,0,0.32)" stroke="rgba(255,255,255,0.40)" stroke-width="1" />
              <rect x="4" y="4" width="63" height="157" rx="8" fill="url(#scr)" />
              <circle cx="12" cy="11" r="2.5" fill="rgba(0,0,0,0.55)" stroke="rgba(255,255,255,0.25)" stroke-width="0.5" />
              <g fill="rgba(255,255,255,0.80)">
                <rect x="44" y="10.5" width="1" height="3" rx="0.3" />
                <rect x="46" y="9.5" width="1" height="4" rx="0.3" />
                <rect x="48" y="8.5" width="1" height="5" rx="0.3" />
                <rect x="50" y="7.5" width="1" height="6" rx="0.3" />
              </g>
              <text x="63" y="13" text-anchor="end" fill="rgba(255,255,255,0.85)" font-size="6.5" font-weight="700" font-family="Segoe UI, sans-serif">5G</text>
              <rect x="22" y="157" width="27" height="1.5" rx="0.75" fill="rgba(255,255,255,0.35)" />
            </svg>
          </div>

          <!-- 2행: SIM 정보 -->
          <div class="w-full grid grid-cols-1 sm:grid-cols-2 gap-3">
            {#each device.sims as sim (sim.slot)}
              <div class="rounded-xl bg-black/25 border {sim.type === 'physical' ? 'border-white/10' : 'border-white/25 border-dashed'} px-5 py-4 space-y-1.5">
                <div class="flex items-center gap-2.5">
                  <span class="text-base font-bold">SIM{sim.slot}</span>
                  <span class="rounded px-1.5 py-0.5 text-[10px] font-semibold {sim.type === 'physical' ? 'bg-white/15' : 'bg-white/25'}">
                    {sim.type === "physical" ? "물리" : "eSIM"}
                  </span>
                </div>
                {#if sim.carrier}
                  <div class="text-base font-semibold">{sim.carrier}</div>
                  <div class="flex items-center gap-1.5 text-sm {sim.volte === 'on' ? 'text-emerald-300' : 'opacity-70'}">
                    {#if sim.volte === "on"}
                      <CircleCheck size={14} /> VoLTE 활성화
                    {:else if sim.volte === "wifi"}
                      Wi-Fi 통화만 등록 (VoLTE 아님)
                    {:else if sim.volte === "off"}
                      VoLTE 비활성화
                    {:else}
                      VoLTE 상태 확인 불가
                    {/if}
                  </div>
                {:else}
                  <div class="text-base font-semibold opacity-50">{simStateLabel(sim.state)}</div>
                  <div class="text-sm opacity-50">
                    {sim.state === "ABSENT" ? "SIM을 꽂으면 자동 인식됩니다" : "폰에서 SIM 상태를 확인해 주세요"}
                  </div>
                {/if}
              </div>
            {/each}
          </div>

          <!-- 작업 시작 버튼 -->
          <button
            class="rounded-xl bg-white/95 px-12 py-4 text-lg font-bold text-primary elev-3
              hover:bg-white active:scale-[0.98] transition-all
              flex items-center gap-3"
            onclick={start}
          >
            VoLTE 작업 시작
            <ArrowRight size={20} />
          </button>
      </div>

      <!-- 크레딧 — 하단 absolute (공간 차지 안 함) -->
      <div class="absolute bottom-3 left-0 right-0 flex flex-col items-center gap-0.5 text-[11px] text-primary-foreground/50 pointer-events-auto">
        <span>
          made by <button type="button" onclick={() => api.openExternal(GITHUB_URL)} class="underline hover:text-primary-foreground/80 transition-colors">korjwl1</button>
        </span>
        <span>
          Special thanks to <button type="button" onclick={() => api.openExternal(CAFE_URL)} class="underline hover:text-primary-foreground/80 transition-colors">Sony User Group</button>
        </span>
      </div>
    </div>

  {:else if pending}
    <div class="flex-1 flex flex-col items-center justify-center gap-4 p-8 text-center">
      <Smartphone size={48} class="text-muted-foreground/30" />
      {#if pending.state === "unauthorized"}
        <p class="text-muted-foreground text-lg font-medium">USB 디버깅 허용을 기다리는 중입니다</p>
        <p class="text-sm text-muted-foreground">폰 화면에 뜬 'USB 디버깅을 허용하시겠습니까?'에서 허용을 눌러 주세요</p>
      {:else}
        <p class="text-muted-foreground text-lg font-medium">기기와 통신할 수 없습니다</p>
        <p class="text-sm text-muted-foreground">USB 케이블을 뽑았다가 다시 연결해 주세요</p>
      {/if}
    </div>

  {:else}
    <div class="flex-1 flex flex-col items-center justify-center gap-4">
      <Usb size={48} class="text-muted-foreground/30" />
      <p class="text-muted-foreground text-lg font-medium">연결된 기기가 없습니다</p>
      <p class="text-sm text-muted-foreground">USB 케이블로 Xperia를 연결하면 자동으로 인식됩니다</p>
    </div>
  {/if}

  <!-- 연결 수단 점검 실패 팝업 -->
  {#if showLinkPopup}
    <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/45 p-6">
      <div class="w-full max-w-md rounded-2xl border-2 border-warning/40 bg-background elev-3 p-6 space-y-4">
        <div class="flex items-center gap-3">
          <span class="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-warning-container text-warning">
            <TriangleAlert size={20} />
          </span>
          <div class="space-y-0.5">
            <h2 class="text-base font-semibold">기기와 연결할 수 없습니다</h2>
            <p class="text-xs text-muted-foreground">{linkError}</p>
          </div>
        </div>
        <p class="text-[11px] leading-relaxed text-muted-foreground">
          PC의 USB 설정이나 보안 프로그램이 연결을 막고 있을 수 있습니다.
          케이블과 포트를 확인한 뒤 다시 시도해 주세요.
        </p>
        <div class="flex justify-end gap-2">
          <button
            class="rounded-md border px-3 py-1.5 text-sm hover:bg-muted transition-colors"
            onclick={() => (linkDismissed = true)}
          >
            닫기
          </button>
          <button
            class="rounded-md bg-primary px-3 py-1.5 text-sm font-medium text-primary-foreground hover:opacity-90 transition-opacity"
            onclick={retryLink}
          >
            다시 시도
          </button>
        </div>
      </div>
    </div>
  {/if}
</div>
