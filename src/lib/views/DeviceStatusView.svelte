<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { Button } from "$lib/components/ui/button";
  import { Badge } from "$lib/components/ui/badge";
  import {
    CircleCheck, TriangleAlert, OctagonX, Info, Wrench, Usb, Smartphone, ArrowRight,
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
    <div class="flex-1 flex flex-col overflow-y-auto p-8 lg:p-12">
      <div class="flex-1 grid grid-rows-[auto_auto_1fr] gap-6 max-w-4xl w-full mx-auto">
        <!-- 1행: 기기 정보 + 이미지 -->
        <div class="flex items-center gap-8">
          <div class="flex-1 min-w-0 space-y-3">
            <h1 class="text-3xl lg:text-4xl font-bold tracking-tight">{device.productName}</h1>
            <div class="flex flex-wrap items-center gap-2">
              <span class="rounded-md bg-muted px-2.5 py-1 font-mono text-sm">{device.model}</span>
              <span class="rounded-md bg-muted px-2.5 py-1 font-mono text-sm">{device.serialMasked}</span>
            </div>
            <p class="text-sm text-muted-foreground">{device.firmware} · Android {device.android}</p>
            <div class="flex flex-wrap gap-2 pt-1">
              <Badge variant={device.bootloader === "locked" ? "secondary" : "destructive"} class="text-xs px-3 py-1">
                {device.bootloader === "locked" ? "🔒 부트로더 잠김" : "🔓 부트로더 언락"}
              </Badge>
              <Badge variant="outline" class="text-xs px-3 py-1">
                {device.rooted === true ? "루팅됨" : "루팅 없음"}
              </Badge>
            </div>
          </div>
          <svg viewBox="0 0 96 176" class="w-24 lg:w-32 shrink-0 drop-shadow-xl" aria-hidden="true">
            <defs>
              <linearGradient id="scr" x1="0" y1="0" x2="1" y2="1">
                <stop offset="0" stop-color="oklch(0.545 0.19 282)" stop-opacity="0.3" />
                <stop offset="1" stop-color="oklch(0.545 0.19 282)" stop-opacity="0.08" />
              </linearGradient>
            </defs>
            <rect x="2" y="2" width="92" height="172" rx="16" fill="url(#scr)" stroke="currentColor" stroke-width="1.5" class="text-primary" />
            <rect x="8" y="8" width="80" height="160" rx="12" fill="transparent" stroke="currentColor" stroke-width="0.75" class="text-primary/40" />
            <circle cx="48" cy="18" r="2.5" fill="currentColor" class="text-primary/50" />
            <g fill="currentColor" class="text-primary/60">
              <rect x="20" y="140" width="5" height="9" rx="1.5" />
              <rect x="29" y="134" width="5" height="15" rx="1.5" />
              <rect x="38" y="128" width="5" height="21" rx="1.5" />
              <rect x="47" y="122" width="5" height="27" rx="1.5" />
            </g>
            <text x="88" y="148" text-anchor="end" fill="currentColor" class="text-primary/80" font-size="13" font-weight="700" font-family="Segoe UI, sans-serif">5G</text>
            <circle cx="84" cy="14" r="3.5" fill="currentColor" class="text-primary/30" />
          </svg>
        </div>

        <!-- 2행: SIM 정보 -->
        <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
          {#each device.sims as sim (sim.slot)}
            <div class="rounded-xl border bg-card p-5 space-y-2 {sim.type === 'physical' ? '' : 'border-dashed'}">
              <div class="flex items-center gap-2.5">
                <span class="text-lg font-bold">SIM{sim.slot}</span>
                <Badge variant="outline" class="text-[11px]">{sim.type === "physical" ? "물리" : "eSIM"}</Badge>
              </div>
              {#if sim.carrier}
                <div class="text-base font-semibold">{sim.carrier}</div>
                <div class="flex items-center gap-1.5 text-sm {sim.volteEnabled ? 'text-success' : 'text-muted-foreground'}">
                  {#if sim.volteEnabled}
                    <CircleCheck size={15} /> VoLTE 사용 가능
                  {:else}
                    VoLTE 미적용
                  {/if}
                </div>
              {:else}
                <div class="text-sm text-muted-foreground">미삽입</div>
              {/if}
            </div>
          {/each}
        </div>

        <!-- 하단: 작업 시작 버튼 -->
        <div class="flex items-end justify-center pt-4 pb-2">
          <button
            class="grad-hero rounded-xl px-12 py-5 text-lg font-bold text-primary-foreground elev-3
              hover:brightness-110 active:brightness-95 transition-all
              flex items-center gap-3"
            onclick={start}
          >
            VoLTE 작업 시작
            <ArrowRight size={22} />
          </button>
        </div>
      </div>
    </div>

  {:else}
    <!-- 미연결 — 풀스크린 -->
    <div class="flex-1 flex flex-col items-center justify-center gap-4 p-8">
      <Usb size={48} class="text-muted-foreground/30" />
      <p class="text-muted-foreground text-lg font-medium">연결된 기기가 없습니다</p>
      <p class="text-sm text-muted-foreground">USB 케이블로 Xperia를 연결하면 자동으로 인식됩니다</p>
    </div>
  {/if}
</div>
