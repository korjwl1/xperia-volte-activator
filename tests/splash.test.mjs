import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createServer } from "vite";

let server, splash;
before(async () => {
  server = await createServer({ server: { middlewareMode: true, watch: null, hmr: false, ws: false }, appType: "custom" });
  splash = await server.ssrLoadModule("/src/lib/splash/draw.ts");
});
after(async () => { await server?.close(); });

// Canvas 2D 호출을 받아 두기만 하는 가짜 컨텍스트
function stubContext() {
  const calls = [];
  const gradient = { addColorStop() {} };
  const target = { measureText: text => ({ width: text.length * 8 }), createLinearGradient: () => gradient, createRadialGradient: () => gradient };
  return { calls, ctx: new Proxy(target, {
    get(t, key) { if (key in t) return t[key]; return (...args) => { calls.push(String(key)); return undefined; }; },
    set(t, key, value) { t[key] = value; return true; },
  }) };
}

test("splash draws every phase without the phone image and never fakes progress text", () => {
  for (const t of [0, 0.5, 2, 3.8, splash.SPLASH_STILL_TIME, 30]) {
    const { ctx, calls } = stubContext();
    splash.drawSplash(ctx, t, null);
    assert.ok(calls.includes("fillRect"));
    assert.ok(!calls.includes("drawImage"), "이미지가 없으면 폰을 그리지 않는다");
  }
});

test("splash draws the phone once the image has loaded", () => {
  const { ctx, calls } = stubContext();
  splash.drawSplash(ctx, 4, { complete: true, naturalWidth: 300, naturalHeight: 700 });
  assert.ok(calls.includes("drawImage"));
});
