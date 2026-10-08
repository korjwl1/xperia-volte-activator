<script lang="ts">
  import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "$lib/components/ui/card";
  import { Badge } from "$lib/components/ui/badge";
  import { TriangleAlert, ExternalLink } from "@lucide/svelte/icons";
  import { api } from "$lib/api";
  import { wizard } from "$lib/stores/wizard.svelte";
  import { resolveCarrier, simStateLabel, type CarrierId, type SimTarget } from "$lib/types";
  import { simTypeLabel } from "$lib/domain/communication";

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
    if (id !== null) wizard.volteConfig.bootloaderAction = null; // 패치를 고르면 부트로더만 작업은 해제
  }

  // 인식된 기종·사용자 선택 통신사·리락 여부에 맞는 안내만 표시한다.
  const support = $derived(wizard.workflow.support);

  const isPicked = (slot: 1 | 2, id: CarrierId | null) => {
    const cur = targetOf(slot)?.carrier ?? null;
    return id === "LGU" ? cur === "LGU" || cur === "LGU_V" : cur === id;
  };
</script>

<div class="flex-1 overflow-y-auto flex">
  <div class="m-auto w-full max-w-2xl p-6 space-y-4">
  {#if support.level !== "일반"}
    <div class="rounded-xl border border-warning/40 bg-warning-container/40 px-4 py-3 space-y-1.5">
      <div class="flex items-center gap-2 text-sm font-semibold">
        <TriangleAlert size={15} class="shrink-0 text-warning" />{support.name} · {support.level}
      </div>
      {#each support.notes as n, i (i)}
        <p class="text-[12px] leading-relaxed text-muted-foreground">· {n}</p>
      {/each}
      {#if support.source}
        <button type="button" class="inline-flex items-center gap-1 text-[11px] text-primary hover:underline" onclick={() => api.openExternal(support.source!)}>
          <ExternalLink size={11} />기종별 주의 사항
        </button>
      {/if}
    </div>
  {/if}
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
                  <Badge variant="outline" class="text-[10px]">{simTypeLabel(sim.type)}</Badge>
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


  </div>
</div>
