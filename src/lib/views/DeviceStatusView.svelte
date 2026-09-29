<script lang="ts">
  import { onMount } from "svelte";
  import { Button } from "$lib/components/ui/button";
  import { Card, CardContent, CardHeader, CardTitle } from "$lib/components/ui/card";
  import {
    CircleCheck, TriangleAlert, OctagonX, Info, Wrench, Usb, Smartphone,
  } from "@lucide/svelte/icons";
  import { api } from "$lib/api";
  import { wizard } from "$lib/stores/wizard.svelte";
  import type { DeviceStatus, EnvCheckItem } from "$lib/types";

  let device = $state<DeviceStatus | null>(null);
  let env = $state<EnvCheckItem[]>([]);
  let loading = $state(false);

  onMount(async () => {
    loading = true;
    try {
      const [d, e] = await Promise.all([api.deviceStatus("AB1234****"), api.envCheck()]);
      device = d;
      env = e;
      wizard.device = d;
      wizard.env = e;
    } finally {
      loading = false;
    }
  });

  const stateStyle: Record<EnvCheckItem["state"], string> = {
    pass: "bg-success-container text-success",
    warn: "bg-warning-container text-warning",
    fail: "bg-danger-container text-destructive",
    info: "bg-info-container text-info",
  };

  async function fix(id: string) {
    await api.envFix(id);
    const item = env.find((e) => e.id === id);
    if (item) item.state = "pass";
  }
</script>

<div class="space-y-4">
  {#if loading}
    <Card class="elev-1"><CardContent class="py-16 text-center text-muted-foreground">기기 연결 대기 중…</CardContent></Card>
  {:else if device}
    {@render DeviceConnected(device)}
  {:else}
    <!-- 미연결 상태 -->
    <div class="rounded-xl grad-hero-soft elev-2 border p-8 text-center space-y-3">
      <Smartphone size={40} class="mx-auto text-muted-foreground/40" />
      <p class="text-muted-foreground text-sm font-medium">연결된 기기가 없습니다</p>
      <p class="text-xs text-muted-foreground">USB 케이블로 Xperia를 연결하면 자동으로 인식됩니다</p>
    </div>
  {/if}

  <!-- 준비 상태: 문제 있는 항목만 -->
  {#if device && env.some((e) => e.state !== "pass")}
    <Card class="elev-1">
      <CardHeader class="pb-3">
        <CardTitle class="text-sm">주의 필요</CardTitle>
      </CardHeader>
      <CardContent class="space-y-1.5">
        {#each env.filter((e) => e.state !== "pass") as item (item.id)}
          <div class="flex items-center gap-3 rounded-lg border bg-background/60 px-3 py-2">
            <span class="flex h-6 w-6 shrink-0 items-center justify-center rounded-md {stateStyle[item.state]}">
              {#if item.state === "pass"}<CircleCheck size={15} />
              {:else if item.state === "warn"}<TriangleAlert size={15} />
              {:else if item.state === "fail"}<OctagonX size={15} />
              {:else}<Info size={15} />{/if}
            </span>
            <div class="min-w-0 flex-1">
              <div class="text-[13px] font-medium">{item.label}</div>
              <div class="text-[11px] text-muted-foreground truncate">{item.detail}</div>
            </div>
            {#if item.fixable}
              <Button size="sm" variant="outline" class="h-7 text-xs" onclick={() => fix(item.id)}>
                <Wrench size={13} class="mr-1" />수정
              </Button>
            {/if}
          </div>
        {/each}
      </CardContent>
    </Card>
  {/if}
</div>

{#snippet DeviceConnected(d: DeviceStatus)}
  <div class="grad-hero rounded-xl elev-2 text-primary-foreground overflow-hidden">
    <div class="flex flex-col sm:flex-row items-stretch gap-6 p-6">
      <div class="flex-1 min-w-0 space-y-4">
        <div>
          <div class="text-[11px] uppercase tracking-widest opacity-80">연결된 기기</div>
          <h1 class="text-2xl font-semibold mt-1">{d.productName}</h1>
          <div class="flex flex-wrap items-center gap-2 mt-2">
            <span class="rounded-md bg-white/15 px-2 py-0.5 font-mono text-xs">{d.model}</span>
            <span class="rounded-md bg-white/15 px-2 py-0.5 font-mono text-xs">{d.serialMasked}</span>
          </div>
          <p class="text-xs opacity-85 mt-2">펌웨어 {d.firmware} · Android {d.android}</p>
        </div>
        <div class="flex flex-wrap gap-2">
          <div class="flex items-center gap-1.5 rounded-lg bg-white/15 px-2.5 py-1.5 text-xs font-medium">
            {#if d.bootloader === "locked"}🔒 부트로더 잠김{:else if d.bootloader === "unlocked"}🔓 부트로더 언락{:else}확인 중{/if}
          </div>
          <div class="flex items-center gap-1.5 rounded-lg bg-white/15 px-2.5 py-1.5 text-xs font-medium">
            {#if d.rooted === true}루팅됨{:else if d.rooted === false}루팅 없음{:else}확인 중{/if}
          </div>
        </div>
        <div class="grid grid-cols-1 sm:grid-cols-2 gap-2">
          {#each d.sims as sim (sim.slot)}
            <div class="rounded-lg bg-black/20 px-3 py-2.5 space-y-1 border {sim.type === 'physical' ? 'border-white/10' : 'border-white/25 border-dashed'}">
              <div class="flex items-center gap-2 text-[11px] opacity-90">
                SIM{sim.slot}
                <span class="rounded px-1 py-0.5 text-[10px] font-semibold {sim.type === 'physical' ? 'bg-white/15' : 'bg-white/25'}">
                  {sim.type === "physical" ? "물리" : "eSIM"}
                </span>
              </div>
              {#if sim.carrier}
                <div class="text-sm font-semibold">{sim.carrier}</div>
                <div class="text-[11px] {sim.volteEnabled ? 'text-emerald-300' : 'opacity-75'}">
                  {#if sim.volteEnabled}
                    ✓ VoLTE 사용 가능{sim.patchedWith ? ` (${sim.patchedWith})` : ""}
                  {:else}
                    VoLTE 미적용
                  {/if}
                </div>
              {:else}
                <div class="text-sm opacity-60">미삽입</div>
              {/if}
            </div>
          {/each}
        </div>
      </div>
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
        <g fill="rgba(255,255,255,0.9)">
          <rect x="20" y="140" width="5" height="8" rx="1" />
          <rect x="28" y="135" width="5" height="13" rx="1" />
          <rect x="36" y="129" width="5" height="19" rx="1" />
          <rect x="44" y="123" width="5" height="25" rx="1" />
        </g>
        <text x="74" y="146" text-anchor="end" fill="rgba(255,255,255,0.95)" font-size="11" font-weight="700" font-family="Segoe UI, sans-serif">5G</text>
        <circle cx="80" cy="16" r="3" fill="rgba(0,0,0,0.35)" stroke="rgba(255,255,255,0.45)" />
      </svg>
    </div>
  </div>
{/snippet}
