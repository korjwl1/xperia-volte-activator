<script lang="ts">
  // 순서대로 따라 하는 조작 안내 — 카드 한 장씩 넘김 (이전/다음, 점 표시, ←/→ 키)
  import { fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { ChevronLeft, ChevronRight } from "@lucide/svelte/icons";
  import type { GuideSlide } from "$lib/data/guides";

  let { slides }: { slides: GuideSlide[] } = $props();

  let index = $state(0);
  let dir = $state(1);
  const slide = $derived(slides[Math.min(index, slides.length - 1)]);

  function go(next: number) {
    if (next < 0 || next >= slides.length || next === index) return;
    dir = next > index ? 1 : -1;
    index = next;
  }

  function onkeydown(e: KeyboardEvent) {
    const t = e.target as HTMLElement | null;
    if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA")) return;
    if (e.key === "ArrowRight") go(index + 1);
    else if (e.key === "ArrowLeft") go(index - 1);
  }
</script>

<svelte:window {onkeydown} />

<div class="rounded-xl border bg-muted/40 overflow-hidden">
  <div class="relative overflow-hidden">
    {#key index}
      <div class="p-3" in:fly={{ x: 40 * dir, duration: 220, easing: cubicOut }}>
        <!-- 이미지 + 테마 색 강조 테두리 -->
        <div class="relative overflow-hidden rounded-lg border bg-background {slide.kind === 'illustration' ? 'bg-white' : ''}">
          <img src={slide.image} alt={slide.title} class="block w-full select-none" draggable="false" />
          {#if slide.highlight}
            {@const h = slide.highlight}
            <div
              class="guide-ring pointer-events-none absolute rounded-xl"
              style="left:{h.x}%; top:{h.y}%; width:{h.w}%; height:{h.h}%"
            ></div>
            {#if slide.badge}
              <span
                class="pointer-events-none absolute -translate-y-full rounded-md bg-primary px-1.5 py-0.5 text-[10px] font-semibold text-primary-foreground"
                style="left:{h.x}%; top:{h.y}%"
              >{slide.badge}</span>
            {/if}
          {/if}
        </div>
        <div class="mt-3 flex items-start gap-2.5">
          {#if slides.length > 1}<span class="flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-primary text-primary-foreground text-[11px] font-bold">{index + 1}</span>{/if}
          <div class="space-y-0.5">
            <p class="text-sm font-semibold">{slide.title}</p>
            <p class="text-[13px] leading-relaxed text-muted-foreground">{slide.caption}</p>
          </div>
        </div>
      </div>
    {/key}
  </div>

  {#if slides.length > 1}
  <div class="flex items-center justify-between border-t px-2 py-1.5">
    <button
      type="button"
      class="flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground disabled:opacity-30 disabled:pointer-events-none"
      disabled={index === 0}
      onclick={() => go(index - 1)}
      aria-label="이전"
    ><ChevronLeft size={16} /></button>
    <div class="flex items-center gap-1.5">
      {#each slides as s, i (i)}
        <button
          type="button"
          class="h-1.5 rounded-full transition-all {i === index ? 'w-5 bg-primary' : 'w-1.5 bg-muted-foreground/30 hover:bg-muted-foreground/60'}"
          onclick={() => go(i)}
          aria-label="{i + 1}. {s.title}"
        ></button>
      {/each}
      <span class="ml-1.5 text-[11px] tabular-nums text-muted-foreground">{index + 1} / {slides.length}</span>
    </div>
    <button
      type="button"
      class="flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground disabled:opacity-30 disabled:pointer-events-none"
      disabled={index === slides.length - 1}
      onclick={() => go(index + 1)}
      aria-label="다음"
    ><ChevronRight size={16} /></button>
  </div>
  {/if}
</div>

<style>
  .guide-ring {
    box-shadow:
      0 0 0 2px var(--primary),
      0 0 0 6px color-mix(in oklch, var(--primary) 25%, transparent);
    animation: guide-pulse 1.8s ease-in-out infinite;
  }
  @keyframes guide-pulse {
    50% {
      box-shadow:
        0 0 0 2px var(--primary),
        0 0 0 10px color-mix(in oklch, var(--primary) 8%, transparent);
    }
  }
</style>
