<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import { api } from "$lib/api";
  import { requireResult } from "$lib/domain/rootTools";
  let { message, onComplete, onCancel }: { message: string; onComplete: () => void; onCancel: () => void } = $props();
  let saving = $state(false), saved = $state(""), error = $state("");
  async function savePreset() {
    if (saving) return; saving = true; error = "";
    try { const dest = await api.pickFolder(); if (dest) saved = requireResult(await api.rootPresetExport(dest)); }
    catch (e) { error = e instanceof Error ? e.message : String(e); }
    finally { saving = false; }
  }
</script>

<div class="rounded-xl bg-info-container p-4 space-y-3 text-xs">
  <p class="font-semibold">폰에서 설정을 확인하세요</p><p>{message}</p>
  <div class="flex flex-wrap gap-2">
    {#if message.includes("HMA")}<Button size="sm" variant="outline" disabled={saving} onclick={savePreset}>카페 HMA 프리셋 PC 저장</Button>{/if}
    {#if message.includes("WebUI")}<Button size="sm" variant="outline" onclick={() => api.openExternal("https://github.com/MeowDump/KsuWebUIStandalone/releases")}>WebUI 설치 안내</Button>{/if}
    <Button size="sm" disabled={saving} onclick={onComplete}>설정을 완료했습니다</Button><Button size="sm" variant="ghost" disabled={saving} onclick={onCancel}>세트 설치 중단</Button>
  </div>
  {#if saved}<p class="break-all">저장 위치: {saved}</p>{/if}{#if error}<p role="alert" class="text-warning">{error}</p>{/if}
</div>
