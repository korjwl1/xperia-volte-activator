<script lang="ts">
  import { Lock, LockOpen, HelpCircle } from "@lucide/svelte/icons";
  import type { DeviceStatus } from "$lib/types";

  let { device }: { device: DeviceStatus } = $props();
</script>

<div class="grad-hero rounded-xl elev-2 text-primary-foreground overflow-hidden">
  <div class="flex flex-col sm:flex-row items-stretch gap-6 p-6">
    <div class="flex-1 min-w-0 space-y-4">
      <div>
        <div class="text-[11px] uppercase tracking-widest opacity-80">Connected Device</div>
        <h1 class="text-2xl font-semibold mt-1">{device.productName}</h1>
        <div class="flex flex-wrap items-center gap-2 mt-2">
          <span class="rounded-md bg-white/15 px-2 py-0.5 font-mono text-xs">{device.model}</span>
          <span class="rounded-md bg-white/15 px-2 py-0.5 font-mono text-xs">{device.serialMasked}</span>
        </div>
        <p class="text-xs opacity-85 mt-2">
          펌웨어 {device.firmware} · Android {device.android} · 모드 {device.mode}
        </p>
      </div>

      <div class="flex flex-wrap gap-2">
        <div class="flex items-center gap-1.5 rounded-lg bg-white/15 px-2.5 py-1.5 text-xs font-medium">
          {#if device.bootloader === "locked"}
            <Lock size={14} /> 부트로더 잠김
          {:else if device.bootloader === "unlocked"}
            <LockOpen size={14} /> 부트로더 언락
          {:else}
            <HelpCircle size={14} /> 언락 상태 unknown
          {/if}
        </div>
        <div class="flex items-center gap-1.5 rounded-lg bg-white/15 px-2.5 py-1.5 text-xs font-medium">
          {#if device.rooted === true}
            루팅 있음
          {:else if device.rooted === false}
            루팅 없음
          {:else}
            <HelpCircle size={14} /> 루팅 unknown
          {/if}
        </div>
        <div class="flex items-center gap-1.5 rounded-lg px-2.5 py-1.5 text-xs font-medium {device.volte.enabled ? 'bg-white/15' : 'bg-black/25'}">
          {device.volte.enabled ? "VoLTE 활성" : "VoLTE 비활성"}
          <span class="opacity-75">· IMS {device.volte.ims}</span>
        </div>
        {#each device.sims as sim (sim.slot)}
          {#if sim.plmn !== "-"}
            <div class="rounded-lg bg-white/15 px-2.5 py-1.5 text-xs font-medium">
              SIM{sim.slot} {sim.carrier} <span class="opacity-75 font-mono">{sim.plmn}</span>
            </div>
          {/if}
        {/each}
      </div>
    </div>

    <!-- 폰 일러스트 -->
    <svg viewBox="0 0 96 176" class="w-[88px] shrink-0 drop-shadow-lg hidden sm:block self-center" aria-hidden="true">
      <defs>
        <linearGradient id="scr" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stop-color="rgba(255,255,255,0.35)" />
          <stop offset="1" stop-color="rgba(255,255,255,0.08)" />
        </linearGradient>
      </defs>
      <rect x="4" y="4" width="88" height="168" rx="14" fill="rgba(0,0,0,0.38)" stroke="rgba(255,255,255,0.5)" stroke-width="1.5" />
      <rect x="9" y="9" width="78" height="158" rx="10" fill="url(#scr)" />
      <circle cx="48" cy="20" r="2.2" fill="rgba(255,255,255,0.7)" />
      <!-- 신호 표시 -->
      <g fill="rgba(255,255,255,0.9)">
        <rect x="20" y="140" width="5" height="8" rx="1" />
        <rect x="28" y="135" width="5" height="13" rx="1" />
        <rect x="36" y="129" width="5" height="19" rx="1" />
        <rect x="44" y="123" width="5" height="25" rx="1" />
      </g>
      <text x="74" y="146" text-anchor="end" fill="rgba(255,255,255,0.95)" font-size="11" font-weight="700" font-family="Segoe UI, sans-serif">5G</text>
      <!-- 카메라 도트 -->
      <circle cx="80" cy="16" r="3" fill="rgba(0,0,0,0.35)" stroke="rgba(255,255,255,0.45)" />
    </svg>
  </div>

  {#if !device.volte.enabled && device.volte.reason}
    <div class="bg-black/20 px-6 py-2 text-[11px] opacity-90">{device.volte.reason}</div>
  {/if}
</div>
