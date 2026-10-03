<script lang="ts">
  import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "$lib/components/ui/card";
  import { Badge } from "$lib/components/ui/badge";
  import { LoaderCircle, LockOpen, Lock, TriangleAlert, ExternalLink } from "@lucide/svelte/icons";
  import { modelSupport } from "$lib/data/devices";
  import { api } from "$lib/api";
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
    if (id !== null) wizard.volteConfig.bootloaderAction = null; // 패치를 고르면 부트로더만 작업은 해제
  }

  // 기종별 지원 범위 — 기기에 따라 고정(선택에 따라 바뀌지 않음)
  const support = $derived(modelSupport(wizard.device?.model ?? ""));

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
  const pickVersion = (v: string) => {
    wizard.volteConfig.firmware = v === installed ? null : v;
    if (v !== installed) wizard.volteConfig.bootloaderAction = null; // 업데이트와 함께 진행하지 않음
  };

  // 부트로더만 작업 — 모든 슬롯 "패치 안 함" + 업데이트 없음일 때만, 기기 상태에 맞는 쪽만
  const blAvailable = $derived(!wizard.hasPatchTarget && wizard.updateVersion === null);
  const bl = $derived(wizard.device?.bootloader ?? "unknown");
  const blActions = $derived([
    {
      id: "unlock" as const,
      label: "언락만 진행하기",
      desc: "부트로더 언락만 진행합니다 — 기기가 초기화됩니다",
      icon: LockOpen,
      enabled: blAvailable && bl === "locked",
      reason: bl === "unlocked" ? "이미 언락된 기기입니다" : bl === "unknown" ? "부트로더 상태를 확인할 수 없습니다" : "",
    },
    {
      id: "relock" as const,
      label: "리락만 진행하기",
      desc: "필요하면 언루팅 후 부트로더를 다시 잠급니다 — 기기가 초기화되며 VoLTE 패치는 유지됩니다",
      icon: Lock,
      enabled: blAvailable && bl === "unlocked",
      reason: bl === "locked" ? "이미 잠긴 기기입니다" : bl === "unknown" ? "부트로더 상태를 확인할 수 없습니다" : "",
    },
  ]);
  const pickAction = (id: "unlock" | "relock") =>
    (wizard.volteConfig.bootloaderAction = wizard.volteConfig.bootloaderAction === id ? null : id);
  // 조건이 깨지면(패치·업데이트 선택) 선택도 해제된 것으로 표시
  const actionPicked = (id: "unlock" | "relock") => wizard.bootloaderOnly === id;

  // 표시 기준: LGU_V도 "LG U+" 버튼이 선택된 것으로
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

  <!-- 부트로더만 작업 — 모든 SIM이 "패치 안 함"이고 업데이트가 없을 때만 -->
  <Card class="elev-1">
    <CardHeader>
      <CardTitle class="text-base">부트로더만 작업</CardTitle>
      <CardDescription>VoLTE 패치 없이 언락 또는 리락만 진행합니다 — 모든 SIM이 '패치 안 함'이고 펌웨어가 현재 버전일 때 선택할 수 있습니다</CardDescription>
    </CardHeader>
    <CardContent class="grid grid-cols-1 sm:grid-cols-2 gap-3">
      {#each blActions as a (a.id)}
        {@const picked = actionPicked(a.id)}
        <button
          class="flex items-start gap-3 rounded-xl border-2 px-4 py-3 text-left transition-all
            {picked ? 'border-primary ring-2 ring-primary/25 bg-primary/5' : 'border-border'}
            {a.enabled ? (picked ? '' : 'hover:border-primary/40') : 'opacity-50 cursor-not-allowed'}"
          disabled={!a.enabled}
          title={a.enabled ? undefined : a.reason || undefined}
          onclick={() => pickAction(a.id)}
        >
          <a.icon size={16} class="mt-0.5 shrink-0 {picked ? 'text-primary' : 'text-muted-foreground'}" />
          <div class="min-w-0 space-y-0.5">
            <div class="text-sm font-semibold">{a.label}</div>
            <div class="text-[11px] text-muted-foreground">{a.desc}</div>
          </div>
        </button>
      {/each}
    </CardContent>
  </Card>
  </div>
</div>
