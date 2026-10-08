import assert from "node:assert/strict";
import fs from "node:fs";
import { after, before, beforeEach, test } from "node:test";
import { createServer } from "vite";

let server, api, flags, defaults, Wizard;
let invocations;
before(async () => {
  server = await createServer({ server: { middlewareMode: true, watch: null, hmr: false, ws: false }, appType: "custom" });
  ({ api } = await server.ssrLoadModule("/src/lib/api/index.ts"));
  ({ REAL_STEPS: flags } = await server.ssrLoadModule("/src/lib/data/runMode.ts"));
  ({ Wizard } = await server.ssrLoadModule("/src/lib/stores/wizard.svelte.ts"));
  defaults = { ...flags };
});
beforeEach(() => {
  Object.assign(flags, { backup: false, restore: false, fastboot: false, root: false, verify: false, efs: false });
  invocations = [];
  globalThis.window = { __TAURI_INTERNALS__: { invoke: async (command, args) => { invocations.push([command, args]); return null; } } };
});
after(async () => { Object.assign(flags, defaults); delete globalThis.window; await server?.close(); });

// 2026-10-08: 실기기 검증을 마친 단계만 기본 실전(사용자 결정). 리락·루팅 도구·업데이트 확인과 루팅 도구 쓰기 기능은 끈다.
test("release defaults enable only device-verified live steps and write features", () => {
  assert.deepEqual(defaults, { backup: true, restore: true, fastboot: true, relock: false, root: true, rootTools: false, verify: false, efs: true });
  const features = fs.readFileSync("src-tauri/Cargo.toml", "utf8").match(/\[features\][\s\S]*?\ndefault\s*=\s*\[([^\]]*)\]/)[1];
  assert.deepEqual(features.split(",").map(s => s.trim().replace(/"/g, "")).filter(Boolean), ["fastboot-write", "root-write", "efs-write"]);
});

test("actual API facade rejects disabled writes before invoking the native bridge", async () => {
  for (const work of [
    () => api.fastbootUnlock("1234567890abcdef", true, "A"),
    () => api.fastbootFlash("init_boot", "image", true, "A", "a".repeat(64)),
    () => api.fastbootLock(true, "init_boot", "stock", "A"),
    () => api.fastbootReboot("os", "A"),
    () => api.magiskPatch({ serial: "A" }),
    () => api.magiskInstall("A", "apk", "hash"),
    () => api.rootReboot("A", "os"),
    () => api.contactsRestoreFinish("A", "backup"),
    () => api.efsDiagOpen("A"),
    () => api.efsUpload("preset"),
    () => api.efsVerify("preset"),
    () => api.efsSnapshot("dest", "preset"),
    () => api.efsRollback("snapshot"),
    () => api.voltePropsSet("A"),
  ]) assert.equal((await work()).ok, false);
  assert.deepEqual(invocations, []);
});

test("EFS-only run can finish its OS reboot while bootloader reboot stays disabled", async () => {
  flags.efs = true;
  assert.equal((await api.rootReboot("A", "os")).ok, true);
  assert.deepEqual(invocations, [["root_reboot", { serial: "A", target: "os" }]]);
  assert.equal((await api.rootReboot("A", "bootloader")).ok, false);
  assert.equal((await api.rootReboot("A", "fastboot")).ok, false);
  assert.equal(invocations.length, 1);
  assert.equal(new Wizard().simulationControlsVisible, false);
});

test("journal facade preserves native read errors and absence as different results", async () => {
  assert.deepEqual(await api.journalLoad("key"), { ok: true, value: null });
  window.__TAURI_INTERNALS__.invoke = async () => { throw new Error("denied"); };
  assert.deepEqual(await api.journalLoad("key"), { ok: false, error: "denied" });
});
