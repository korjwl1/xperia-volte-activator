<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { Badge } from "$lib/components/ui/badge";
  import {
    CircleCheck, TriangleAlert, Usb, Smartphone, ArrowRight,
  } from "@lucide/svelte/icons";
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
    <!-- 풀스크린 그라디언트 히어로 -->
    <div class="flex-1 grad-hero flex flex-col overflow-y-auto">
      <div class="flex-1 flex flex-col max-w-4xl w-full mx-auto p-8 lg:p-12 gap-8">
        <!-- 1행: 기기 정보 + 이미지 -->
        <div class="flex items-center gap-8">
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
              <div class="flex items-center gap-1.5 rounded-lg bg-white/15 px-3 py-1.5 text-xs font-medium">
                {#if device.bootloader === "locked"}🔒 부트로더 잠김{:else if device.bootloader === "unlocked"}🔓 부트로더 언락{:else}확인 중{/if}
              </div>
              <div class="flex items-center gap-1.5 rounded-lg bg-white/15 px-3 py-1.5 text-xs font-medium">
                {#if device.rooted === true}루팅됨{:else if device.rooted === false}루팅 없음{:else}확인 중{/if}
              </div>
            </div>
          </div>

          <svg viewBox="0 0 96 176" class="w-24 lg:w-32 shrink-0 drop-shadow-xl self-center" aria-hidden="true">
            <defs>
              <linearGradient id="scr" x1="0" y1="0" x2="1" y2="1">
                <stop offset="0" stop-color="rgba(255,255,255,0.30)" />
                <stop offset="1" stop-color="rgba(255,255,255,0.06)" />
              </linearGradient>
            </defs>
            <rect x="2" y="2" width="92" height="172" rx="16" fill="rgba(0,0,0,0.30)" stroke="rgba(255,255,255,0.45)" stroke-width="1.5" />
            <rect x="8" y="8" width="80" height="160" rx="12" fill="url(#scr)" />
            <circle cx="48" cy="18" r="2.5" fill="rgba(255,255,255,0.65)" />
            <g fill="rgba(255,255,255,0.85)">
              <rect x="20" y="140" width="5" height="9" rx="1.5" />
              <rect x="29" y="134" width="5" height="15" rx="1.5" />
              <rect x="38" y="128" width="5" height="21" rx="1.5" />
              <rect x="47" y="122" width="5" height="27" rx="1.5" />
            </g>
            <text x="88" y="148" text-anchor="end" fill="rgba(255,255,255,0.90)" font-size="13" font-weight="700" font-family="Segoe UI, sans-serif">5G</text>
            <circle cx="84" cy="14" r="3.5" fill="rgba(0,0,0,0.30)" stroke="rgba(255,255,255,0.40)" />
          </svg>
        </div>

        <!-- 2행: SIM 정보 -->
        <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
          {#each device.sims as sim (sim.slot)}
            <div class="rounded-xl bg-black/25 border {sim.type === 'physical' ? 'border-white/10' : 'border-white/25 border-dashed'} px-5 py-4 space-y-2 text-primary-foreground">
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
                <div class="text-sm opacity-60">미삽입</div>
                <div class="text-xs opacity-50">SIM을 꽂으면 자동 인식됩니다</div>
              {/if}
            </div>
          {/each}
        </div>

        <!-- 하단: 큰 작업 시작 버튼 -->
        <div class="flex-1 flex items-end justify-center pb-4">
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
    </div>

  {:else}
    <!-- 미연결 — 풀스크린 -->
    <div class="flex-1 flex flex-col items-center justify-center gap-4">
      <Usb size={48} class="text-muted-foreground/30" />
      <p class="text-muted-foreground text-lg font-medium">연결된 기기가 없습니다</p>
      <p class="text-sm text-muted-foreground">USB 케이블로 Xperia를 연결하면 자동으로 인식됩니다</p>
    </div>
  {/if}
</div>
