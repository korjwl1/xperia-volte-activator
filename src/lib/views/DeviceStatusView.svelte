<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { CircleCheck, TriangleAlert, Usb, Smartphone, ArrowRight } from "@lucide/svelte/icons";
  import { api } from "$lib/api";
  import { wizard } from "$lib/stores/wizard.svelte";
  import type { DeviceStatus, EnvCheckItem } from "$lib/types";

  let devices = $state<DeviceStatus[]>([]);
  let env = $state<EnvCheckItem[]>([]);
  let loading = $state(true);
  let pollTimer: ReturnType<typeof setInterval> | undefined;

  const device = $derived(devices.length === 1 ? devices[0] : null);
  const multiDevice = $derived(devices.length > 1);

  async function refresh() {
    const list = await api.deviceList();
    devices = list;
    wizard.device = list.length === 1 ? list[0] : null;
    if (env.length === 0) env = await api.envCheck();
  }

  onMount(async () => {
    await refresh();
    loading = false;
    pollTimer = setInterval(refresh, 3000);
  });

  onDestroy(() => {
    if (pollTimer) clearInterval(pollTimer);
  });

  function start() {
    wizard.view = "volte-config";
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
          {#each devices as d (d.serial)}
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
    <!-- 풀스크린 그라디언트 — 콘텐츠 중앙 + 크레딧 하단 고정 -->
    <div class="flex-1 grad-hero flex flex-col overflow-hidden">
      <div class="flex-1 flex items-center justify-center overflow-y-auto">
        <div class="w-full max-w-3xl px-8 py-10 flex flex-col items-center gap-8 text-primary-foreground">
          <!-- 1행: 기기 정보 + 이미지 -->
          <div class="w-full flex items-center justify-between gap-10">
        <!-- 1행: 기기 정보 + 이미지 -->
        <div class="w-full flex items-center justify-between gap-10">
          <div class="flex-1 min-w-0 space-y-3 text-primary-foreground">
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
                {#if device.bootloader === "locked"}🔒 부트로더 잠김{:else if device.bootloader === "unlocked"}🔓 언락{:else}확인 중{/if}
              </div>
              <div class="rounded-lg bg-white/15 px-3 py-1.5 text-xs font-medium">
                {#if device.rooted === true}루팅됨{:else if device.rooted === false}루팅 없음{:else}확인 중{/if}
              </div>
            </div>
          </div>

          <!-- 폰 일러스트 — Xperia 1 V 비율 (71:165) -->
          <svg viewBox="0 0 71 165" class="w-[80px] lg:w-[100px] shrink-0 drop-shadow-xl" aria-hidden="true">
            <defs>
              <linearGradient id="scr" x1="0" y1="0" x2="1" y2="1">
                <stop offset="0" stop-color="rgba(255,255,255,0.25)" />
                <stop offset="0.5" stop-color="rgba(255,255,255,0.10)" />
                <stop offset="1" stop-color="rgba(255,255,255,0.04)" />
              </linearGradient>
            </defs>
            <!-- 바디 -->
            <rect x="0.5" y="0.5" width="70" height="164" rx="10" fill="rgba(0,0,0,0.32)" stroke="rgba(255,255,255,0.40)" stroke-width="1" />
            <!-- 스크린 -->
            <rect x="4" y="4" width="63" height="157" rx="8" fill="url(#scr)" />
            <!-- 전면 카메라 (상단 좌측 펀치홀) -->
            <circle cx="12" cy="11" r="2.5" fill="rgba(0,0,0,0.55)" stroke="rgba(255,255,255,0.25)" stroke-width="0.5" />
            <!-- 상태바: 신호바 + 5G (상단 우측) -->
            <g fill="rgba(255,255,255,0.80)">
              <rect x="44" y="10.5" width="1" height="3" rx="0.3" />
              <rect x="46" y="9.5" width="1" height="4" rx="0.3" />
              <rect x="48" y="8.5" width="1" height="5" rx="0.3" />
              <rect x="50" y="7.5" width="1" height="6" rx="0.3" />
            </g>
            <text x="63" y="13" text-anchor="end" fill="rgba(255,255,255,0.85)" font-size="6.5" font-weight="700" font-family="Segoe UI, sans-serif">5G</text>
            <!-- 홈 인디케이터 -->
            <rect x="22" y="157" width="27" height="1.5" rx="0.75" fill="rgba(255,255,255,0.35)" />
          </svg>
        </div>

        <!-- 2행: SIM 정보 -->
        <div class="w-full grid grid-cols-1 sm:grid-cols-2 gap-3">
          {#each device.sims as sim (sim.slot)}
            <div class="rounded-xl bg-black/25 border {sim.type === 'physical' ? 'border-white/10' : 'border-white/25 border-dashed'} px-5 py-4 space-y-1.5 text-primary-foreground">
              <div class="flex items-center gap-2.5">
                <span class="text-base font-bold">SIM{sim.slot}</span>
                <span class="rounded px-1.5 py-0.5 text-[10px] font-semibold {sim.type === 'physical' ? 'bg-white/15' : 'bg-white/25'}">
                  {sim.type === "physical" ? "물리" : "eSIM"}
                </span>
              </div>
              {#if sim.carrier}
                <div class="text-base font-semibold">{sim.carrier}</div>
                <div class="flex items-center gap-1.5 text-sm {sim.volteEnabled ? 'text-emerald-300' : 'opacity-70'}">
                  {#if sim.volteEnabled}
                    <CircleCheck size={14} /> VoLTE 사용 가능
                  {:else}
                    VoLTE 미적용
                  {/if}
                </div>
              {:else}
                <div class="text-base font-semibold opacity-50">미삽입</div>
                <div class="text-sm opacity-50">SIM을 꽂으면 자동 인식됩니다</div>
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
    </div>

      <!-- 크레딧 — 하단 고정 -->
      <div class="shrink-0 pb-3 flex flex-col items-center gap-0.5 text-[11px] text-primary-foreground/50">
        <span>
          made by <a href="https://github.com/korjwl1" target="_blank" rel="noopener" class="underline hover:text-primary-foreground/80 transition-colors">korjwl1</a>
        </span>
        <span>
          Special thanks to <a href="https://cafe.naver.com/x1smart" target="_blank" rel="noopener" class="underline hover:text-primary-foreground/80 transition-colors">Sony User Group</a>
        </span>
      </div>
    </div>

  {:else}
    <div class="flex-1 flex flex-col items-center justify-center gap-4">
      <Usb size={48} class="text-muted-foreground/30" />
      <p class="text-muted-foreground text-lg font-medium">연결된 기기가 없습니다</p>
      <p class="text-sm text-muted-foreground">USB 케이블로 Xperia를 연결하면 자동으로 인식됩니다</p>
    </div>
  {/if}
</div>
