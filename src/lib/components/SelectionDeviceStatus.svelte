<script lang="ts" module>
  // 화면(진행 방법·수동 진행 등)을 오가도 유지 — 첫 화면에서 이미 확인한 기기를 다시 전체 조회하지 않기 위한 표식.
  // USB 목록에서 폰이 사라지거나 USB 디버깅 연결이 아니게 되면 true가 되고, 다음 선택 때 전체 조회한다.
  let connectionChanged = false;
</script>

<script lang="ts">
  import { onMount } from "svelte";
  import { Button } from "$lib/components/ui/button";
  import { CircleHelp, Lock, LockOpen, LoaderCircle, RefreshCw, ShieldCheck, ShieldOff, Signal, SignalZero, Smartphone, TriangleAlert } from "@lucide/svelte/icons";
  import { volteSummary } from "$lib/domain/communication";
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

  // 작업 선택에 실제로 필요한 상태만 — 언락·루팅·VoLTE(셀룰러 IMS 음성 등록으로 판단)
  const shown = $derived(wizard.device ?? selected);
  const chips = $derived(shown ? [
    shown.bootloader === "unlocked" ? { icon: LockOpen, label: "언락", tone: "primary" }
      : shown.bootloader === "locked" ? { icon: Lock, label: "잠김", tone: "muted" }
      : { icon: CircleHelp, label: "부트로더 확인 불가", tone: "muted" },
    shown.rooted === true ? { icon: ShieldCheck, label: "루팅됨", tone: "success" }
      : shown.rooted === false ? { icon: ShieldOff, label: "루팅 안 됨", tone: "muted" }
      : { icon: CircleHelp, label: "루팅 확인 불가", tone: "muted" },
    volteSummary(shown.sims).state === "on" ? { icon: Signal, label: "VoLTE 활성", tone: "success" }
      : { icon: volteSummary(shown.sims).state === "unknown" ? CircleHelp : SignalZero, label: volteSummary(shown.sims).label, tone: "muted" },
  ] : []);
  const toneClass = { primary: "bg-primary/10 text-primary", success: "bg-success-container text-success", muted: "bg-background text-muted-foreground" } as const;

  function changed(a: DeviceStatus | null, b: DeviceStatus) {
    return !a || JSON.stringify(a) !== JSON.stringify(b);
  }

  /** [상태 다시 확인] — 항상 전체 조회. 끝날 때까지 로딩을 표시한다 */
  export async function refresh(): Promise<boolean> {
    checking = true;
    try {
      const ok = await probe();
      if (ok) connectionChanged = false;
      return ok;
    } finally { if (alive) checking = false; }
  }

  /** 카드 선택 — 연결 변화가 없으면 바로 진행한다(폰 명령 없음). 뽑혔다 꽂히는 등 변화가 감지됐을 때만 전체 조회.
   *  같은 폰인지는 각 작업이 실행 직전에 다시 대조한다 */
  export async function ensure(): Promise<boolean> {
    if (!connectionChanged && wizard.device && wizard.device.state === "device" && !error) return true;
    return refresh();
  }

  // 가벼운 연결 감시 — USB 장치 기술자만 읽는다(폰 셸 명령 없음, 화면을 막지 않음)
  async function watchUsb() {
    const modes = await api.usbModes();
    if (!alive || modes === null) return;
    const phones = modes.filter(m => m.mode !== "other" || m.vendorId === 0x0fce);
    if (phones.length !== 1 || phones[0].mode !== "android") connectionChanged = true;
  }

  onMount(() => {
    alive = true;
    void watchUsb();
    const timer = setInterval(() => void watchUsb(), 2000);
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
  <div class="flex shrink-0 flex-wrap justify-end gap-1.5" aria-label="기기 상태">
    {#each chips as chip (chip.label)}
      <span class="inline-flex items-center gap-1 rounded-full px-2.5 py-1 text-[11px] font-medium {toneClass[chip.tone as keyof typeof toneClass]}"><chip.icon size={12} />{chip.label}</span>
    {/each}
  </div>
  <Button variant="outline" size="sm" disabled={checking} onclick={refresh}>
    {#if checking}<LoaderCircle size={13} class="animate-spin" />{:else}<RefreshCw size={13} />{/if}
    상태 다시 확인
  </Button>
</div>
