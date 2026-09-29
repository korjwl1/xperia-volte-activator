<script lang="ts">
  import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "$lib/components/ui/card";
  import { Badge } from "$lib/components/ui/badge";
  import { Signal, Scale, Zap } from "@lucide/svelte/icons";
  import { wizard } from "$lib/stores/wizard.svelte";
  import { CARRIER_LABEL, type CarrierId, type PresetMode } from "$lib/types";

  const carriers: { id: CarrierId; note?: string }[] = [
    { id: "SKT" }, { id: "KT" }, { id: "LGU" },
    { id: "LGU_V", note: "Xperia 1 V / 5 V 전용 프로파일" },
  ];
  const modes: { id: PresetMode; icon: typeof Scale; label: string; desc: string }[] = [
    { id: "balance", icon: Scale, label: "균형", desc: "배터리와 성능의 균형" },
    { id: "performance", icon: Zap, label: "실내 우선", desc: "실내 수신 개선 우선" },
  ];
</script>

<div class="space-y-4">
  <Card class="elev-1">
    <CardHeader>
      <CardTitle class="text-base">VoLTE 설정</CardTitle>
      <CardDescription>어떤 SIM에 어떤 통신사 프로파일을 적용할지 선택합니다</CardDescription>
    </CardHeader>
    <CardContent class="space-y-6">
      <!-- SIM 슬롯 -->
      <div class="space-y-2">
        <div class="text-xs font-semibold text-muted-foreground uppercase tracking-wide">SIM 슬롯</div>
        <div class="grid grid-cols-2 gap-2">
          {#each wizard.device?.sims ?? [] as sim (sim.slot)}
            <button
              class="rounded-lg border-2 p-3 text-left transition-all
                {wizard.volteConfig.simSlot === sim.slot ? 'border-primary ring-2 ring-primary/25' : 'border-transparent hover:border-border'}"
              onclick={() => (wizard.volteConfig.simSlot = sim.slot)}
            >
              <div class="flex items-center gap-2">
                <span class="text-sm font-semibold">SIM{sim.slot}</span>
                <Badge variant="outline" class="text-[10px]">{sim.type === "physical" ? "물리" : "eSIM"}</Badge>
              </div>
              <div class="text-xs text-muted-foreground mt-1">
                {sim.carrier ?? "미삽입"}
              </div>
            </button>
          {/each}
        </div>
      </div>

      <!-- 통신사 -->
      <div class="space-y-2">
        <div class="text-xs font-semibold text-muted-foreground uppercase tracking-wide">통신사</div>
        <div class="grid grid-cols-2 gap-2">
          {#each carriers as c (c.id)}
            <button
              class="rounded-lg border-2 p-3 text-left transition-all
                {wizard.volteConfig.carrier === c.id ? 'border-primary ring-2 ring-primary/25' : 'border-transparent hover:border-border'}"
              onclick={() => (wizard.volteConfig.carrier = c.id)}
            >
              <div class="flex items-center gap-2">
                <Signal size={15} class={wizard.volteConfig.carrier === c.id ? "text-primary" : "text-muted-foreground"} />
                <span class="text-sm font-semibold">{CARRIER_LABEL[c.id]}</span>
              </div>
              {#if c.note}
                <div class="text-[11px] text-muted-foreground mt-0.5">{c.note}</div>
              {/if}
            </button>
          {/each}
        </div>
      </div>

      <!-- 프로파일 모드 -->
      <div class="space-y-2">
        <div class="text-xs font-semibold text-muted-foreground uppercase tracking-wide">프로파일 모드</div>
        <div class="grid grid-cols-2 gap-2">
          {#each modes as m (m.id)}
            <button
              class="rounded-lg border-2 p-3 text-left transition-all
                {wizard.volteConfig.mode === m.id ? 'border-primary ring-2 ring-primary/25' : 'border-transparent hover:border-border'}"
              onclick={() => (wizard.volteConfig.mode = m.id)}
            >
              <div class="flex items-center gap-2">
                <m.icon size={15} class={wizard.volteConfig.mode === m.id ? "text-primary" : "text-muted-foreground"} />
                <span class="text-sm font-semibold">{m.label}</span>
              </div>
              <div class="text-[11px] text-muted-foreground mt-0.5">{m.desc}</div>
            </button>
          {/each}
        </div>
      </div>
    </CardContent>
  </Card>
</div>
