<script lang="ts">
  import { onMount } from "svelte";
  import { Button } from "$lib/components/ui/button";
  import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "$lib/components/ui/card";
  import { Badge } from "$lib/components/ui/badge";
  import { Separator } from "$lib/components/ui/separator";
  import { api } from "$lib/api";
  import { wizard } from "$lib/stores/wizard.svelte";
  import type { DeviceStatus, EnvCheckItem } from "$lib/types";

  let device = $state<DeviceStatus | null>(null);
  let env = $state<EnvCheckItem[]>([]);
  let loading = $state(false);

  onMount(async () => {
    loading = true;
    try {
      // contract: device_list → device_status (mock 250ms 지연)
      const [d, e] = await Promise.all([api.deviceStatus("AB1234****"), api.envCheck()]);
      device = d;
      env = e;
      wizard.device = d;
      wizard.env = e;
    } finally {
      loading = false;
    }
  });

  const envVariant = (s: EnvCheckItem["state"]) =>
    s === "pass" ? "default" : s === "warn" ? "secondary" : s === "fail" ? "destructive" : "outline";
  const envText = (s: EnvCheckItem["state"]) =>
    s === "pass" ? "통과" : s === "warn" ? "경고" : s === "fail" ? "차단" : "정보";

  async function fix(id: string) {
    // contract: env_fix({id}) — 자동 수리 (§12.6)
    await api.envFix(id);
    const item = env.find((e) => e.id === id);
    if (item) item.state = "pass";
  }
</script>

<div class="space-y-4">
  {#if loading}
    <Card><CardContent class="py-10 text-center text-muted-foreground">디바이스 감지 중… (USB 폴링)</CardContent></Card>
  {:else if device}
    <Card>
      <CardHeader>
        <div class="flex items-center justify-between">
          <div>
            <CardTitle class="text-lg">{device.productName} <span class="text-muted-foreground font-mono text-sm">{device.model}</span></CardTitle>
            <CardDescription>
              펌웨어 {device.firmware} · Android {device.android} · 모드 {device.mode}
            </CardDescription>
          </div>
          <Badge variant="outline" class="font-mono">{device.serialMasked}</Badge>
        </div>
      </CardHeader>
      <CardContent class="space-y-4">
        <div class="grid grid-cols-2 gap-3 md:grid-cols-4">
          <div class="rounded-lg border p-3 space-y-1">
            <div class="text-xs text-muted-foreground">부트로더</div>
            <div>{device.bootloader === "locked" ? "🔒 잠김" : device.bootloader === "unlocked" ? "🔓 언락" : "unknown"}</div>
          </div>
          <div class="rounded-lg border p-3 space-y-1">
            <div class="text-xs text-muted-foreground">루팅</div>
            <div>{device.rooted === true ? "있음" : device.rooted === false ? "없음" : "unknown"}</div>
          </div>
          <div class="rounded-lg border p-3 space-y-1">
            <div class="text-xs text-muted-foreground">VoLTE</div>
            <div>{device.volte.enabled ? "활성" : "비활성"} · IMS {device.volte.ims}</div>
            {#if !device.volte.enabled && device.volte.reason}
              <div class="text-[11px] text-muted-foreground leading-tight">{device.volte.reason}</div>
            {/if}
          </div>
          <div class="rounded-lg border p-3 space-y-1">
            <div class="text-xs text-muted-foreground">SIM</div>
            {#each device.sims as sim}
              <div class="text-sm">슬롯{sim.slot}: {sim.carrier} <span class="text-muted-foreground">{sim.plmn}</span></div>
            {/each}
          </div>
        </div>
        <Separator />
        <div class="text-xs text-muted-foreground">
          USB — {device.usb.topology} · {device.usb.controller} · {device.usb.linkSpeed}
          <span class="ml-2">§10-5 프리플라이트는 EFS 단계 직전에 자동 실행됩니다</span>
        </div>
      </CardContent>
    </Card>

    <Card>
      <CardHeader>
        <CardTitle class="text-base">환경 검사 (Readiness)</CardTitle>
        <CardDescription>§12.6 — Blocker 해제 전 해당 작업 버튼이 비활성됩니다 · QPST 미설치는 정상 상태입니다</CardDescription>
      </CardHeader>
      <CardContent class="space-y-2">
        {#each env as item (item.id)}
          <div class="flex items-center justify-between gap-3 rounded-lg border px-3 py-2">
            <div class="min-w-0">
              <div class="flex items-center gap-2">
                <Badge variant={envVariant(item.state)}>{envText(item.state)}</Badge>
                <span class="text-sm font-medium">{item.label}</span>
              </div>
              <div class="text-xs text-muted-foreground truncate">{item.detail}</div>
            </div>
            {#if item.state === "warn" || item.state === "fail"}
              <Button size="sm" variant="outline" onclick={() => fix(item.id)}>자동 수리</Button>
            {/if}
          </div>
        {/each}
      </CardContent>
    </Card>
  {:else}
    <Card>
      <CardContent class="py-10 text-center space-y-2">
        <p class="text-muted-foreground">연결된 Xperia가 없습니다.</p>
        <p class="text-xs text-muted-foreground">USB 케이블로 연결해 주세요 — 모드 전환 시 자동 재감지됩니다 (§9-3)</p>
      </CardContent>
    </Card>
  {/if}
</div>
