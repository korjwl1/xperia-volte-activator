<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  // 폰에서 할 설정을 번호 단계로 보여 준다. 줄바꿈으로 나눈 메시지를 순서 목록으로 렌더한다(2026-10-09).
  let { message, onComplete, onCancel }: { message: string; onComplete: () => void; onCancel: () => void } = $props();
  const lines = $derived(message.split("\n").map(line => line.trim()).filter(Boolean));
</script>

<div class="rounded-xl bg-info-container p-4 space-y-3 text-xs">
  <p class="font-semibold">폰에서 설정해 주세요</p>
  {#if lines.length > 1}
    <ol class="list-decimal space-y-1 pl-5 leading-relaxed">
      {#each lines as line (line)}<li>{line}</li>{/each}
    </ol>
  {:else}
    <p class="leading-relaxed">{message}</p>
  {/if}
  <div class="flex flex-wrap gap-2">
    <Button size="sm" onclick={onComplete}>설정을 완료했습니다</Button>
    <Button size="sm" variant="ghost" onclick={onCancel}>세트 설치 중단</Button>
  </div>
</div>
