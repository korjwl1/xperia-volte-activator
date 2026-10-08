<script lang="ts">
  import { onMount } from "svelte";
  import { Button } from "$lib/components/ui/button";
  import { LoaderCircle, RefreshCw, Smartphone, TriangleAlert } from "@lucide/svelte/icons";
  import { api } from "$lib/api";
  import { wizard } from "$lib/stores/wizard.svelte";
  import type { DeviceStatus } from "$lib/types";
  /** 사용자가 직접 확인 중일 때만 true — 백그라운드 감시는 화면을 막지 않는다 */
  let { checking = $bindable(false) }: { checking?: boolean } = $props();
  const selected = wizard.device;
  let error = $state("");
  let alive = false;
  let inFlight: Promise<boolean> | null = null;

  // 첫 화면에서 기기 상태는 이미 확인했다. 여기서는 "같은 폰이 아직 연결·승인돼 있는가"만 필요하므로
  // 반복 감시 없이, 진행을 누를 때와 [상태 다시 확인]을 누를 때만 조회한다. 3초마다 전체 조회를 돌리며
  // 조회 중 화면을 막던 방식은 버튼이 계속 돌고 카드가 눌리지 않게 만들었다(2026-10-08 사용자 지적).
  function probe(): Promise<boolean> {
    inFlight ??= (async () => {
      try {
        const list = await api.deviceList();
        if (!alive) return false;
        const device = list?.length === 1 ? list[0] : null;
        const matches = !!device && device.state === "device" && device.model === selected?.model
          && (device.serial ?? device.serialMasked) === (selected?.serial ?? selected?.serialMasked);
        // 같은 기기·같은 상태면 객체를 바꾸지 않는다(불필요한 다시 그리기 방지)
        if (!matches) wizard.device = null;
        else if (changed(wizard.device, device)) wizard.device = device;
        error = matches ? "" : "선택한 기기의 연결·USB 디버깅 승인을 다시 확인하세요";
        return matches;
      } catch {
        if (!alive) return false;
        wizard.device = null;
        error = "기기 상태를 조회하지 못했습니다. 다시 확인하세요";
        return false;
      } finally {
        inFlight = null;
      }
    })();
    return inFlight;
  }

  function changed(a: DeviceStatus | null, b: DeviceStatus) {
    return !a || JSON.stringify(a) !== JSON.stringify(b);
  }

  /** 사용자가 누른 확인(버튼·카드) — 끝날 때까지 로딩을 표시한다 */
  export async function refresh(): Promise<boolean> {
    checking = true;
    try { return await probe(); } finally { if (alive) checking = false; }
  }

  onMount(() => {
    alive = true;
    return () => { alive = false; };
  });
</script>

<div class="shrink-0 flex items-center gap-3 rounded-xl bg-muted p-3">
  <Smartphone size={20} class="text-primary shrink-0" />
  <div class="min-w-0 flex-1 text-xs">
    <p class="font-semibold">{selected?.productName ?? "선택한 기기"} · {selected?.model}</p>
    <p class="mt-1 text-muted-foreground">{selected?.serialMasked} · {wizard.device?.firmware ?? selected?.firmware}</p>
    {#if error}<p role="alert" class="mt-1 text-warning flex items-center gap-1"><TriangleAlert size={12} />{error}</p>{/if}
  </div>
  <Button variant="outline" size="sm" disabled={checking} onclick={refresh}>
    {#if checking}<LoaderCircle size={13} class="animate-spin" />{:else}<RefreshCw size={13} />{/if}
    상태 다시 확인
  </Button>
</div>
