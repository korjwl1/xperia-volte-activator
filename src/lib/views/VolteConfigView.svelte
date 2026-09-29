<script lang="ts">
  import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "$lib/components/ui/card";
  import { Badge } from "$lib/components/ui/badge";
  import { Signal, Scale, Zap } from "@lucide/svelte/icons";
  import { wizard } from "$lib/stores/wizard.svelte";
  import type { CarrierId, PresetMode } from "$lib/types";

  // LGU_V는 UI에 표시하지 않음 — 기기 모델로 자동 판별 (1 V/5 V면 LGU 선택 시 LGU_V 사용)
  const carriers: { id: CarrierId; label: string }[] = [
    { id: "SKT", label: "SKT" },
    { id: "KT", label: "KT" },
    { id: "LGU", label: "LG U+" },
  ];
  const modes: { id: PresetMode; icon: typeof Scale; label: string; desc: string }[] = [
    { id: "balance", icon: Scale, label: "균형", desc: "배터리와 성능의 균형" },
    { id: "performance", icon: Zap, label: "실내 우선", desc: "실내 수신 개선 우선" },
  ];

  // LGU 선택 시 기기가 1 V/5 V면 자동으로 LGU_V 사용
  $effect(() => {
    if (wizard.volteConfig.carrier === "LGU") {
      const model = wizard.device?.model ?? "";
      if (model.includes("XQ-DQ") || model.includes("XQ-DE")) {
        wizard.volteConfig.carrier = "LGU_V";
      }
    }
  });

  // 표시용 라벨 (LGU_V도 "LG U+"로 표시)
  const carrierLabel = (id: CarrierId) => (id === "LGU_V" ? "LG U+" : id);
</script>

<div class="flex-1 flex items-center justify-center overflow-y-auto">
  <div class="w-full max-w-2xl p-6 space-y-4">
  <Card class="elev-1">
    <CardHeader>
      <CardTitle class="text-base">VoLTE 설정</CardTitle>
      <CardDescription>적용할 SIM 슬롯과 통신사를 선택합니다</CardDescription>
    </CardHeader>
    <CardContent class="space-y-8">
      <!-- SIM 슬롯 -->
      <div class="space-y-3">
        <div class="text-xs font-semibold text-muted-foreground uppercase tracking-wide">SIM 슬롯</div>
        <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
          {#each wizard.device?.sims ?? [] as sim (sim.slot)}
            <button
              class="rounded-xl border-2 p-4 text-left transition-all
                {wizard.volteConfig.simSlot === sim.slot ? 'border-primary ring-2 ring-primary/25 bg-primary/5' : 'border-border hover:border-primary/40'}"
              onclick={() => (wizard.volteConfig.simSlot = sim.slot)}
            >
              <div class="flex items-center gap-2">
                <span class="text-base font-bold">SIM{sim.slot}</span>
                <Badge variant="outline" class="text-[10px]">{sim.type === "physical" ? "물리" : "eSIM"}</Badge>
              </div>
              <div class="text-sm text-muted-foreground mt-1">{sim.carrier ?? "미삽입"}</div>
            </button>
          {/each}
        </div>
      </div>

      <!-- 통신사 -->
      <div class="space-y-3">
        <div class="text-xs font-semibold text-muted-foreground uppercase tracking-wide">통신사</div>
        <div class="grid grid-cols-3 gap-3">
          {#each carriers as c (c.id)}
            <button
              class="rounded-xl border-2 p-4 text-center transition-all
                {(wizard.volteConfig.carrier === c.id || (c.id === 'LGU' && wizard.volteConfig.carrier === 'LGU_V'))
                  ? 'border-primary ring-2 ring-primary/25 bg-primary/5'
                  : 'border-border hover:border-primary/40'}"
              onclick={() => {
                if (c.id === 'LGU') {
                  const model = wizard.device?.model ?? "";
                  wizard.volteConfig.carrier = (model.includes("XQ-DQ") || model.includes("XQ-DE")) ? "LGU_V" : "LGU";
                } else {
                  wizard.volteConfig.carrier = c.id;
                }
              }}
            >
              <Signal size={20} class="mx-auto {(wizard.volteConfig.carrier === c.id || (c.id === 'LGU' && wizard.volteConfig.carrier === 'LGU_V')) ? 'text-primary' : 'text-muted-foreground'}" />
              <div class="text-sm font-bold mt-2">{c.label}</div>
            </button>
          {/each}
        </div>
      </div>

      <!-- 프로파일 모드 — 간단 토글 -->
      <div class="space-y-3">
        <div class="text-xs font-semibold text-muted-foreground uppercase tracking-wide">프로파일 모드</div>
        <div class="grid grid-cols-2 gap-3">
          {#each modes as m (m.id)}
            <button
              class="rounded-xl border-2 p-4 text-left transition-all
                {wizard.volteConfig.mode === m.id ? 'border-primary ring-2 ring-primary/25 bg-primary/5' : 'border-border hover:border-primary/40'}"
              onclick={() => (wizard.volteConfig.mode = m.id)}
            >
              <div class="flex items-center gap-2.5">
                <m.icon size={18} class={wizard.volteConfig.mode === m.id ? "text-primary" : "text-muted-foreground"} />
                <span class="text-sm font-bold">{m.label}</span>
              </div>
              <div class="text-xs text-muted-foreground mt-1">{m.desc}</div>
            </button>
          {/each}
        </div>
      </div>
    </CardContent>
  </Card>
  </div>
</div>
