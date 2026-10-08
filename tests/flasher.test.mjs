import assert from "node:assert/strict";
import { before, after, afterEach, test } from "node:test";
import { createServer } from "vite";

let server, api;
before(async () => {
  server = await createServer({ server: { middlewareMode: true, watch: null, hmr: false, ws: false }, appType: "custom" });
  ({ api } = await server.ssrLoadModule("/src/lib/api/index.ts"));
});
afterEach(() => { delete globalThis.window; });
after(async () => { delete globalThis.window; await server.close(); });

test("full firmware inspection has no simulated success in browser development", async () => {
  const result = await api.firmwarePackageInspect("C:/firmware", "Sony/target");
  assert.equal(result.ok, false);
  assert.match(result.error, /데스크톱/);
});
test("inspection preserves native blockers and cannot become a write permission", async () => {
  const calls = [];
  const report = { upstreamCommit: "pinned", targetFingerprint: "Sony/target", manifestSha256: "a".repeat(64), totalBytes: 10, candidateBytes: 0, files: [], blockers: ["device-identity-region-profile-not-validated"], writeReady: false };
  globalThis.window = { __TAURI_INTERNALS__: { invoke: async (command, args) => { calls.push({ command, args }); return report; } } };
  assert.deepEqual(await api.firmwarePackageInspect("C:/한글 firmware", "Sony/target"), { ok: true, value: report });
  assert.deepEqual(calls, [{ command: "firmware_package_inspect", args: { dir: "C:/한글 firmware", targetFingerprint: "Sony/target" } }]);
});
test("native inspection failures remain errors without fallback or device actions", async () => {
  let calls = 0;
  globalThis.window = { __TAURI_INTERNALS__: { invoke: async () => { calls++; throw "FLASH_FINGERPRINT|Target mismatch"; } } };
  assert.deepEqual(await api.firmwarePackageInspect("C:/firmware", "Sony/wrong"), { ok: false, error: "FLASH_FINGERPRINT|Target mismatch" });
  assert.equal(calls, 1);
});
