<script lang="ts">
  import { onMount } from "svelte";
  import { Button } from "$lib/components/ui/button";
  import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "$lib/components/ui/card";
  import { Badge } from "$lib/components/ui/badge";
  import DeviceHero from "$lib/components/DeviceHero.svelte";
  import {
    CircleCheck, TriangleAlert, OctagonX, Info, Wrench, Usb, Cable,
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
    // contract: env_fix({id}) — 자동 수리 (§12.6)
    await api.envFix(id);
    const item = env.find((e) => e.id === id);
    if (item) item.state = "pass";
  }
</script>

<div class="space-y-4">
  {#if loading}
    <Card class="elev-1"><CardContent class="py-16 text-center text-muted-foreground">디바이스 감지 중… (USB 폴링)</CardContent></Card>
  {:else if device}
    <DeviceHero {device} />

    <!-- USB 정보 -->
    <div class="flex items-center gap-3 rounded-lg border bg-card px-4 py-2.5 text-xs text-muted-foreground elev-1">
      <Usb size={15} class="text-info shrink-0" />
      <span>{device.usb.topology} · {device.usb.controller} · {device.usb.linkSpeed}</span>
      <Cable size={15} class="ml-auto shrink-0 opacity-60" />
      <span class="opacity-80">§10-5 프리플라이트는 EFS 단계 직전 자동 실행</span>
    </div>

    <!-- 환경 검사 -->
    <Card class="elev-1">
      <CardHeader class="pb-3">
        <CardTitle class="text-sm">환경 검사 <span class="text-muted-foreground font-normal">Readiness</span></CardTitle>
        <CardDescription class="text-xs">
          Blocker 해제 전 해당 작업이 비활성됩니다 · QPST 미설치는 정상 상태입니다 (§12.6)
        </CardDescription>
      </CardHeader>
      <CardContent class="space-y-1.5">
        {#each env as item (item.id)}
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
            {#if item.state === "warn" || item.state === "fail"}
              <Button size="sm" variant="outline" class="h-7 text-xs" onclick={() => fix(item.id)}>
                <Wrench size={13} class="mr-1" />자동 수리
              </Button>
            {:else}
              <Badge variant="outline" class="text-[10px] px-1.5">
                {item.state === "pass" ? "준비됨" : item.state === "info" ? "정보" : item.state}
              </Badge>
            {/if}
          </div>
        {/each}
      </CardContent>
    </Card>
  {:else}
    <Card class="elev-1">
      <CardContent class="py-16 text-center space-y-2">
        <Usb size={28} class="mx-auto text-muted-foreground" />
        <p class="text-muted-foreground text-sm">연결된 Xperia가 없습니다</p>
        <p class="text-xs text-muted-foreground">USB 케이블로 연결해 주세요 — 모드 전환 시 자동 재감지됩니다 (§9-3)</p>
      </CardContent>
    </Card>
  {/if}
</div>
