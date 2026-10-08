// 시작 스플래시 그림 — 사용자가 승인한 시안(output/splash-preview/preview.html, 2026-10-08)의 Canvas 코드를 옮긴 것.
// 960×600 논리 좌표에 그린다. 3.8초 등장 뒤 3.2초 주기의 은은한 대기 모션. 진행률을 흉내 내지 않는다.

export const SPLASH_WIDTH = 960;
export const SPLASH_HEIGHT = 600;
/** 움직임 줄이기 설정일 때 보여 줄 정지 시점(등장이 끝난 상태) */
export const SPLASH_STILL_TIME = 4.6;

const TAU = Math.PI * 2;
const cyan = "#65ceff";
const amber = "#ffae59";
const clamp = (x: number) => Math.min(1, Math.max(0, x));
const smooth = (x: number) => { x = clamp(x); return x * x * (3 - 2 * x); };
const reveal = (t: number, start: number, duration: number) => smooth((t - start) / duration);
const ease = (x: number) => 1 - Math.pow(1 - clamp(x), 3);
const particles = Array.from({ length: 35 }, (_, i) => ({
  x: 98 + ((i * 173.17) % 770),
  y: 90 + ((i * 97.39) % 420),
  r: i % 5 === 0 ? 1.2 : 0.6,
  phase: i * 1.77,
}));

function glow(ctx: CanvasRenderingContext2D, x: number, y: number, r: number, color: string, opacity = 1, squash = 1) {
  ctx.save(); ctx.globalAlpha = opacity; ctx.translate(x, y); ctx.scale(1, squash);
  const g = ctx.createRadialGradient(0, 0, 0, 0, 0, r);
  g.addColorStop(0, color); g.addColorStop(1, "transparent");
  ctx.fillStyle = g; ctx.fillRect(-r, -r, 2 * r, 2 * r); ctx.restore();
}

function tracked(ctx: CanvasRenderingContext2D, text: string, x: number, y: number, size: number, spacing: number, color: string, weight = 300) {
  ctx.font = `${weight} ${size}px "Segoe UI", sans-serif`; ctx.fillStyle = color;
  for (const ch of text) { ctx.fillText(ch, x, y); x += ctx.measureText(ch).width + spacing; }
}

function orbit(ctx: CanvasRenderingContext2D, t: number, front: boolean) {
  const birth = reveal(t, 0.45, 1.65);
  if (!birth) return;
  const cx = 311, cy = 298, rx = 190, ry = 129;
  const phase = t < 3.8 ? ease(t / 3.8) * 0.92 : 0.92 + (t - 3.8) * TAU / 12.8;
  ctx.save(); ctx.globalAlpha = birth;
  for (let track = 0; track < 2; track++) {
    const shift = track * Math.PI;
    const col = track === 0 ? cyan : amber;
    const offset = track === 0 ? -0.11 : 0.16;
    const aStart = phase + shift;
    const len = (front ? 0.58 : 1.65) * birth;
    ctx.lineWidth = front ? 1.7 : 1.25; ctx.strokeStyle = col;
    ctx.shadowColor = col; ctx.shadowBlur = front ? 10 : 6;
    // 밝은 머리와 가늘어지는 꼬리
    for (let j = 0; j < 38; j++) {
      const a = aStart - len + j * len / 38, b = aStart - len + (j + 1) * len / 38;
      const z = Math.sin(a + offset);
      if (front ? z < 0.32 : z >= 0.32) continue;
      ctx.globalAlpha = birth * (0.04 + 0.48 * Math.pow(j / 38, 2));
      ctx.beginPath();
      ctx.moveTo(cx + rx * Math.cos(a), cy + ry * 0.62 * Math.sin(a) + 39 * Math.cos(a));
      ctx.lineTo(cx + rx * Math.cos(b), cy + ry * 0.62 * Math.sin(b) + 39 * Math.cos(b));
      ctx.stroke();
    }
    const z = Math.sin(aStart + offset);
    if (front ? z >= 0.32 : z < 0.32) {
      const x = cx + rx * Math.cos(aStart), y = cy + ry * 0.62 * Math.sin(aStart) + 39 * Math.cos(aStart);
      ctx.globalAlpha = birth; ctx.beginPath(); ctx.arc(x, y, 2.1, 0, TAU); ctx.fillStyle = col; ctx.fill();
      glow(ctx, x, y, 17, col, 0.18);
    }
  }
  ctx.restore();
}

function waveform(ctx: CanvasRenderingContext2D, t: number, opacity: number) {
  ctx.save(); ctx.globalAlpha = opacity; ctx.translate(560, 409);
  ctx.strokeStyle = "#243345"; ctx.lineWidth = 1;
  ctx.beginPath(); ctx.moveTo(0, 0); ctx.lineTo(212, 0); ctx.stroke();
  // 불확정 신호 — 실제 진행률처럼 보이지 않게 계속 흐른다
  const phase = (Math.max(0, t - 2.1) % 3.2) / 3.2;
  const head = -44 + phase * 300;
  const grad = ctx.createLinearGradient(head - 44, 0, head + 44, 0);
  grad.addColorStop(0, "transparent"); grad.addColorStop(0.4, cyan);
  grad.addColorStop(0.72, amber); grad.addColorStop(1, "transparent");
  ctx.strokeStyle = grad; ctx.lineWidth = 1.6; ctx.beginPath();
  for (let x = 0; x <= 212; x += 2) {
    const envelope = Math.exp(-Math.pow((x - head) / 25, 2));
    const y = Math.sin((x - head) * 0.24) * 8 * envelope;
    if (x === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
  }
  ctx.stroke(); ctx.restore();
}

/** t초 시점의 한 프레임. phone이 없으면(불러오기 실패) 폰 없이 나머지만 그린다 */
export function drawSplash(ctx: CanvasRenderingContext2D, time: number, phone: HTMLImageElement | null) {
  const t = Math.max(0, time);
  ctx.globalAlpha = 1; ctx.shadowBlur = 0; ctx.fillStyle = "#080b12"; ctx.fillRect(0, 0, SPLASH_WIDTH, SPLASH_HEIGHT);
  const bg = ctx.createLinearGradient(0, 0, SPLASH_WIDTH, SPLASH_HEIGHT);
  bg.addColorStop(0, "#0b1420"); bg.addColorStop(0.52, "#090e17"); bg.addColorStop(1, "#100e13");
  ctx.fillStyle = bg; ctx.fillRect(0, 0, SPLASH_WIDTH, SPLASH_HEIGHT);
  const life = reveal(t, 0, 1.4);
  const breathe = 0.91 + 0.09 * Math.sin(Math.max(0, t - 3.8) * TAU / 3.2);
  glow(ctx, 256, 267, 263, "#103d5b", 0.28 * life * breathe);
  glow(ctx, 417, 347, 215, "#63341d", 0.105 * life * breathe);
  ctx.save(); ctx.globalAlpha = life;
  for (const p of particles) {
    const a = 0.06 + 0.08 * (0.5 + 0.5 * Math.sin(t * 0.55 + p.phase));
    ctx.globalAlpha = a * life; ctx.fillStyle = p.x < 460 ? cyan : "#a2afc2";
    ctx.beginPath(); ctx.arc(p.x, p.y + Math.sin(t * 0.3 + p.phase) * 2, p.r, 0, TAU); ctx.fill();
  }
  ctx.restore();
  // 폰 뒤의 가는 보조 원
  ctx.save(); ctx.globalAlpha = 0.18 * reveal(t, 0.6, 1.7); ctx.strokeStyle = "#35536c"; ctx.lineWidth = 0.6;
  ctx.beginPath(); ctx.ellipse(311, 289, 185, 185, 0, 0, TAU); ctx.stroke();
  ctx.globalAlpha = 0.1 * reveal(t, 0.8, 1.7);
  ctx.beginPath(); ctx.ellipse(311, 289, 202, 202, 0, 0, TAU); ctx.stroke(); ctx.restore();
  orbit(ctx, t, false);
  if (phone && phone.complete && phone.naturalWidth) {
    const phoneIn = ease((t - 0.18) / 1.4);
    const float = t > 3.8 ? Math.sin((t - 3.8) * TAU / 6.4) * 2.8 : 0;
    ctx.save(); ctx.globalAlpha = reveal(t, 0.16, 1.15);
    ctx.translate(311, 293 + (1 - phoneIn) * 31 + float);
    ctx.scale(0.96 + 0.04 * phoneIn, 0.96 + 0.04 * phoneIn);
    const h = 495, w = h * phone.naturalWidth / phone.naturalHeight;
    ctx.drawImage(phone, -w / 2, -h / 2, w, h); ctx.restore();
  }
  orbit(ctx, t, true);
  const textIn = reveal(t, 1.02, 1.1);
  ctx.save(); ctx.globalAlpha = textIn;
  ctx.translate(0, (1 - ease((t - 1.02) / 1.1)) * 10);
  tracked(ctx, "X P E R I A", 558, 256, 43, 0.7, "#f0f5fb", 300);
  ctx.font = '400 28px "Segoe UI", sans-serif'; ctx.fillStyle = "#d4deeb";
  ctx.fillText("VoLTE Activator", 560, 300);
  ctx.restore();
  ctx.save(); ctx.globalAlpha = reveal(t, 1.55, 0.9);
  tracked(ctx, "SIGNAL WAKE", 562, 207, 10.5, 2.2, "#96a9bc", 500);
  ctx.fillStyle = amber; ctx.fillRect(561, 326, 27, 2); ctx.restore();
  const loading = reveal(t, 2.15, 0.9);
  ctx.save(); ctx.globalAlpha = loading;
  ctx.font = '400 13px "Segoe UI","Malgun Gothic",sans-serif'; ctx.fillStyle = "#8e9fb2";
  ctx.fillText("시작 준비 중", 560, 383);
  for (let i = 0; i < 3; i++) {
    const a = 0.23 + 0.65 * (0.5 + 0.5 * Math.sin(t * TAU / 1.6 - i * 0.7));
    ctx.globalAlpha = loading * a; ctx.fillStyle = i === 2 ? amber : cyan;
    ctx.beginPath(); ctx.arc(751 + i * 8, 379, 1.6, 0, TAU); ctx.fill();
  }
  ctx.restore(); waveform(ctx, t, loading);
  ctx.save(); ctx.globalAlpha = 0.62 * reveal(t, 2.3, 1);
  tracked(ctx, "XPERIA  /  XDA INSPIRED", 560, 472, 9, 1.25, "#6d7d91", 400);
  ctx.restore();
}
