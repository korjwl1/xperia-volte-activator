<script lang="ts">
  import { onMount } from "svelte";
  import { Button } from "$lib/components/ui/button";
  import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "$lib/components/ui/card";
  import { Badge } from "$lib/components/ui/badge";
  import DeviceHero from "$lib/components/DeviceHero.svelte";
  import {
    CircleCheck, TriangleAlert, OctagonX, Info, Wrench, Usb,
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

    <!-- 환경 검사 -->
    <Card class="elev-1">
      <CardHeader class="pb-3">
        <CardTitle class="text-sm">준비 상태</CardTitle>
        <CardDescription class="text-xs">
          문제가 있는 항목만 표시됩니다 · 모두 통과하면 표시되지 않습니다
        </CardDescription>
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
        <p class="text-xs text-muted-foreground">USB 케이블로 연결해 주세요 — 자동으로 감지됩니다</p>
      </CardContent>
    </Card>
  {/if}
</div>
