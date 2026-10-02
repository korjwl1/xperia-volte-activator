<script lang="ts">
  import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "$lib/components/ui/card";
  import { Badge } from "$lib/components/ui/badge";
  import { LoaderCircle } from "@lucide/svelte/icons";
  import { wizard } from "$lib/stores/wizard.svelte";
  import { resolveCarrier, simStateLabel, type CarrierId, type SimTarget } from "$lib/types";

  // LGU_V는 UI에 표시하지 않음 — 기기 모델로 자동 판별 (1 V/5 V면 LGU 선택 시 LGU_V 사용)
  const carriers: { id: CarrierId | null; label: string }[] = [
    { id: "SKT", label: "SKT" },
    { id: "KT", label: "KT" },
    { id: "LGU", label: "LG U+" },
    { id: null, label: "패치 안 함" },
  ];

  const targetOf = (slot: 1 | 2): SimTarget | undefined => wizard.volteConfig.sims.find((t) => t.slot === slot);

  function pick(slot: 1 | 2, id: CarrierId | null) {
    const t = targetOf(slot);
    if (!t) return;
    t.carrier = id === null ? null : resolveCarrier(id, wizard.device?.model ?? "");
  }

  // 펌웨어 버전 — 서버 조회는 기기당 1회 (읽기 전용)
  wizard.ensureFirmwareVersions();
  const installed = $derived(wizard.device?.firmware ?? "");
  const versionRows = $derived.by(() => {
    const list = wizard.fwVersions?.versions ?? [];
    // 서버 목록에 설치된 버전이 없더라도 "현재 설치된 버전"은 항상 첫 줄
    return list.some((v) => v.version === installed)
      ? list
      : [{ version: installed, android: wizard.device?.android ?? "" }, ...list];
  });
  const selectedVersion = $derived(wizard.volteConfig.firmware ?? installed);
  const pickVersion = (v: string) => (wizard.volteConfig.firmware = v === installed ? null : v);

  // 표시 기준: LGU_V도 "LG U+" 버튼이 선택된 것으로
  const isPicked = (slot: 1 | 2, id: CarrierId | null) => {
    const cur = targetOf(slot)?.carrier ?? null;
    return id === "LGU" ? cur === "LGU" || cur === "LGU_V" : cur === id;
  };
</script>

<div class="flex-1 overflow-y-auto flex">
  <div class="m-auto w-full max-w-2xl p-6 space-y-4">
  <Card class="elev-1">
    <CardHeader>
      <CardTitle class="text-base">VoLTE 설정</CardTitle>
      <CardDescription>SIM 슬롯별로 적용할 통신사를 선택합니다</CardDescription>
    </CardHeader>
    <CardContent>
      <!-- SIM 슬롯별 통신사 -->
      <div class="space-y-3">
        <div class="text-xs font-semibold text-muted-foreground uppercase tracking-wide">SIM 슬롯</div>
        <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
          {#each wizard.device?.sims ?? [] as sim (sim.slot)}
            {@const patched = (targetOf(sim.slot)?.carrier ?? null) !== null}
            <div
              class="rounded-xl border-2 p-4 space-y-3 transition-all
                {patched ? 'border-primary ring-2 ring-primary/25 bg-primary/5' : 'border-border'}"
            >
              <div>
                <div class="flex items-center gap-2">
                  <span class="text-base font-bold">SIM{sim.slot}</span>
                  <Badge variant="outline" class="text-[10px]">{sim.type === "physical" ? "물리" : "eSIM"}</Badge>
                </div>
                <div class="text-sm text-muted-foreground mt-1">{sim.carrier || simStateLabel(sim.state)}</div>
              </div>
              <div class="grid grid-cols-2 gap-2">
                {#each carriers as c (c.label)}
                  <button
                    class="rounded-lg border-2 px-2 py-1.5 text-[13px] font-semibold transition-all
                      {isPicked(sim.slot, c.id)
                        ? 'border-primary bg-primary/10 text-primary'
                        : 'border-border text-muted-foreground hover:border-primary/40'}"
                    onclick={() => pick(sim.slot, c.id)}
                  >
                    {c.label}
                  </button>
                {/each}
              </div>
            </div>
          {/each}
        </div>
      </div>
    </CardContent>
  </Card>

  <Card class="elev-1">
    <CardHeader>
      <CardTitle class="text-base">펌웨어 설정</CardTitle>
      <CardDescription>설치할 펌웨어 버전을 선택합니다 — 새 버전을 고르면 사용자 데이터를 유지한 채 업데이트합니다</CardDescription>
    </CardHeader>
    <CardContent class="space-y-2">
      {#each versionRows as v (v.version)}
        {@const picked = selectedVersion === v.version}
        <button
          class="w-full flex items-center gap-3 rounded-xl border-2 px-4 py-3 text-left transition-all
            {picked ? 'border-primary ring-2 ring-primary/25 bg-primary/5' : 'border-border hover:border-primary/40'}"
          onclick={() => pickVersion(v.version)}
        >
          <span class="font-mono text-sm font-semibold">{v.version}</span>
          {#if v.android}<span class="text-xs text-muted-foreground">Android {v.android}</span>{/if}
          {#if v.version === installed}
            <Badge variant="outline" class="ml-auto text-[10px]">현재 설치된 버전</Badge>
          {:else}
            <Badge class="ml-auto text-[10px]">새 버전</Badge>
          {/if}
        </button>
      {/each}
      {#if wizard.fwVersionsState === "loading"}
        <div class="flex items-center gap-2 px-1 text-[11px] text-muted-foreground">
          <LoaderCircle size={12} class="animate-spin text-primary" />서버에서 새 버전 확인 중…
        </div>
      {:else if wizard.fwVersionsState === "failed"}
        <p class="px-1 text-[11px] text-muted-foreground">서버에서 새 버전을 확인할 수 없습니다</p>
      {:else if wizard.fwVersions && !wizard.fwVersions.supported}
        <p class="px-1 text-[11px] text-muted-foreground">이 기종은 아직 새 버전 확인을 지원하지 않습니다</p>
      {:else if versionRows.length === 1}
        <p class="px-1 text-[11px] text-muted-foreground">현재 설치된 버전이 서버의 최신 버전입니다</p>
      {/if}
    </CardContent>
  </Card>
  </div>
</div>
