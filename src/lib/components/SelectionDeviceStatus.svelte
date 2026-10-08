<script lang="ts">
  import { onMount } from "svelte";
  import { Button } from "$lib/components/ui/button";
  import { LoaderCircle, RefreshCw, Smartphone, TriangleAlert } from "@lucide/svelte/icons";
  import { api } from "$lib/api";
  import { wizard } from "$lib/stores/wizard.svelte";
  let { checking = $bindable(true) }: { checking?: boolean } = $props();
  const selected = wizard.device;
  let error = $state("");
  let alive = false;
  export async function refresh(): Promise<boolean> {
    if (checking && alive) return false;
    checking = true;
    try {
      const list = await api.deviceList();
      if (!alive) return false;
      const device = list?.length === 1 ? list[0] : null;
      const matches = device && device.state === "device" && device.model === selected?.model
        && (device.serial ?? device.serialMasked) === (selected?.serial ?? selected?.serialMasked);
      wizard.device = matches ? device : null;
      error = matches ? "" : "선택한 기기의 연결·USB 디버깅 승인을 다시 확인하세요";
      return !!matches;
    } catch {
      if (!alive) return false;
      wizard.device = null;
      error = "기기 상태를 조회하지 못했습니다. 다시 확인하세요";
      return false;
    } finally { if (alive) checking = false; }
  }
  onMount(() => {
    checking = false; alive = true;
    void refresh();
    const timer = setInterval(() => void refresh(), 3000);
    return () => { alive = false; clearInterval(timer); };
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
