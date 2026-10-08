<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { drawSplash, SPLASH_HEIGHT, SPLASH_STILL_TIME, SPLASH_WIDTH } from "$lib/splash/draw";

  // 앱 첫 실행 때 첫 기기 조회가 끝날 때까지 빈 화면을 가린다(2026-10-08 사용자 승인 시안).
  // ready가 되면 곧바로 사라진다 — 3.8초 등장은 시안의 연출 길이일 뿐 최소 대기 시간이 아니다.
  let { ready, onDone }: { ready: boolean; onDone: () => void } = $props();

  let canvas: HTMLCanvasElement;
  let leaving = $state(false);
  const FADE_MS = 450;

  // 주의: leaving을 바꾸면 이 effect가 다시 돌므로 정리 함수로 타이머를 지우면 onDone이 영영 불리지 않는다
  // (2026-10-08 실측: 스플래시가 투명한 채 남아 시작 팝업이 안 뜨고 애니메이션이 계속 돌았다). 타이머는 화면을 떠날 때만 지운다
  let doneTimer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    if (ready && !leaving) {
      leaving = true;
      doneTimer = setTimeout(onDone, FADE_MS);
    }
  });
  onDestroy(() => clearTimeout(doneTimer));

  onMount(() => {
    const ctx = canvas.getContext("2d", { alpha: false });
    if (!ctx) { onDone(); return; }
    const phone = new Image();
    let phoneReady: HTMLImageElement | null = null;
    phone.onload = () => { phoneReady = phone; if (reduced.matches) paint(SPLASH_STILL_TIME); };
    phone.onerror = () => { phoneReady = null; }; // 이미지가 없어도 나머지 화면은 그린다
    phone.src = "/splash/xperia-phone.png";

    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)");
    const start = performance.now();
    let raf = 0;

    // 창 크기에 맞춰 8:5 화면을 선명하게(기기 픽셀 비율) 그린다
    function fit() {
      const ratio = window.devicePixelRatio || 1;
      const scale = Math.min(window.innerWidth / SPLASH_WIDTH, window.innerHeight / SPLASH_HEIGHT);
      const w = Math.round(SPLASH_WIDTH * scale), h = Math.round(SPLASH_HEIGHT * scale);
      canvas.style.width = `${w}px`; canvas.style.height = `${h}px`;
      canvas.width = Math.round(w * ratio); canvas.height = Math.round(h * ratio);
      ctx!.setTransform(canvas.width / SPLASH_WIDTH, 0, 0, canvas.height / SPLASH_HEIGHT, 0, 0);
    }
    function paint(t: number) { drawSplash(ctx!, t, phoneReady); }
    function frame(now: number) {
      paint((now - start) / 1000);
      raf = requestAnimationFrame(frame);
    }
    function run() {
      cancelAnimationFrame(raf);
      // 움직임 줄이기: 등장이 끝난 정지 화면 한 장만
      if (reduced.matches) paint(SPLASH_STILL_TIME);
      else raf = requestAnimationFrame(frame);
    }
    const onResize = () => { fit(); if (reduced.matches) paint(SPLASH_STILL_TIME); };
    fit(); run();
    window.addEventListener("resize", onResize);
    reduced.addEventListener("change", run);
    return () => {
      cancelAnimationFrame(raf);
      window.removeEventListener("resize", onResize);
      reduced.removeEventListener("change", run);
      phone.onload = null; phone.onerror = null;
    };
  });
</script>

<div
  class="fixed inset-0 z-[100] flex items-center justify-center transition-opacity ease-out motion-reduce:transition-none {leaving ? 'opacity-0 pointer-events-none' : 'opacity-100'}"
  style="background:#080b12; transition-duration:{FADE_MS}ms"
  role="status"
  aria-live="polite"
  aria-label="시작 준비 중"
>
  <canvas bind:this={canvas} aria-hidden="true"></canvas>
</div>
