import assert from "node:assert/strict";
import { before, beforeEach, after, test } from "node:test";
import { createServer } from "vite";

let server, Wizard, api, flags, transport, originalApi, originalFlags, originalInvoke;
const configured = { port: "COM9", presetRoot: "C:/bundle", snapshotRoot: "C:/snapshots" };
const ok = value => ({ ok: true, value });
const warning = { code: "nvPrefixVerification", target: "NV 71", message: "Only explicit bytes verified" };
before(async () => {
  server = await createServer({ server: { middlewareMode: true, watch: null, hmr: false, ws: false }, appType: "custom" });
  ({ Wizard } = await server.ssrLoadModule("/src/lib/stores/wizard.svelte.ts"));
  ({ api } = await server.ssrLoadModule("/src/lib/api/index.ts"));
  ({ REAL_STEPS: flags } = await server.ssrLoadModule("/src/lib/data/runMode.ts"));
  ({ transport } = await server.ssrLoadModule("/src/lib/api/transport.ts"));
  originalApi = { ...api }; originalFlags = { ...flags }; originalInvoke = transport.invoke;
});
beforeEach(() => {
  delete globalThis.window;
  Object.assign(flags, originalFlags);
  Object.assign(api, originalApi, {
    journalSave: async () => true, runGuard: async () => true,
    efsConfiguration: async () => ok(configured),
    efsValidatePresets: async () => ok(null),
    efsDiagOpen: async () => ok(null),
    efsPreflight: async () => ok({ log: [], errors: [], warnings: [], parameters: [] }),
    onEfsLog: async () => () => {}, onEfsProgress: async () => () => {},
    efsCancel: async () => ok(null),
  });
  transport.invoke = originalInvoke;
});
after(async () => {
  delete globalThis.window;
  Object.assign(api, originalApi); Object.assign(flags, originalFlags);
  transport.invoke = originalInvoke;
  await server?.close();
});
const deferred = () => {
  let resolve; const promise = new Promise(r => { resolve = r; });
  return { promise, resolve };
};
function wizard(id) {
  flags.efs = true;
  const w = new Wizard();
  w.device = { model: "XQ-DQ44", serial: "TEST-SERIAL", state: "device" };
  w.volteConfig.sims = [{ slot: 1, carrier: "SKT" }, { slot: 2, carrier: "SKT" }];
  w.runSteps = [{ id, title: id, status: "running", progress: 0, logs: [], manualDone: 0 }];
  w.begin = () => {}; w.setGuard = () => {};
  w.stepDone = cur => { cur.status = "done"; };
  return w;
}
async function settled(w) {
  for (let i = 0; i < 100; i++) {
    await new Promise(resolve => setImmediate(resolve));
    if (w.busy === 0) return;
  }
  assert.fail("EFS engine did not settle");
}

test("EFS facade gates device calls while cancellation remains available", async () => {
  Object.assign(api, originalApi);
  globalThis.window = { __TAURI_INTERNALS__: {} };
  const calls = [];
  transport.invoke = async command => { calls.push(command); return null; };
  for (const [method, args] of [
    ["efsDiagOpen", ["TEST-SERIAL"]], ["efsPreflight", []], ["efsUpload", ["preset"]],
    ["efsVerify", ["preset"]], ["efsSnapshot", ["dest", "preset"]],
    ["efsRollback", ["snapshot"]], ["voltePropsSet", ["TEST-SERIAL"]],
  ]) assert.equal((await api[method](...args)).ok, false, method);
  assert.deepEqual(calls, []);
  assert.equal((await api.efsCancel()).ok, true);
  assert.deepEqual(calls, ["efs_cancel"]);
});

test("configured native facade preserves structured errors and explicit COM arguments", async () => {
  Object.assign(api, originalApi); flags.efs = true;
  globalThis.window = { __TAURI_INTERNALS__: {} };
  const nativeError = { code: "deviceStatus", operation: "NV write", message: "Rejected", status: 5, cleanup: ["close failed"] };
  const calls = [];
  transport.invoke = async (command, args) => {
    calls.push([command, args]);
    if (command === "efs_config_get") return configured;
    if (command === "efs_resolve_preset") return "C:/bundle/approved-preset";
    throw nativeError;
  };
  const result = await api.efsUpload("./util/SonyEFS/approved");
  assert.equal(result.ok, false); assert.deepEqual(result.details, nativeError);
  assert.match(result.error, /deviceStatus.*NV write.*close failed/);
  assert.deepEqual(calls.at(-1), ["efs_upload", { port: "COM9", presetDir: "C:/bundle/approved-preset" }]);
});

test("reset during EFS listener registration prevents DIAG and releases the late listener", async () => {
  const pending = deferred(); let closed = 0, diag = 0;
  api.onEfsLog = () => pending.promise;
  api.efsDiagOpen = async () => { diag++; return ok(null); };
  const w = wizard("efs-preflight");
  w.tick();
  for (let i = 0; i < 4; i++) await Promise.resolve();
  w.restart();
  pending.resolve(() => { closed++; });
  await settled(w);
  assert.equal(diag, 0); assert.equal(closed, 1);
});

test("preset conflict blocks the whole selection before any device step", async () => {
  let diag = 0;
  api.efsValidatePresets = async () => ({ ok: false, error: "presetConflict: NV 71" });
  api.efsDiagOpen = async () => { diag++; return ok(null); };
  const w = wizard("efs-preflight"); w.tick(); await settled(w);
  assert.equal(diag, 0); assert.equal(w.runSteps[0].status, "failed");
  assert.match(w.stepError, /presetConflict/);
});

test("native upload snapshots each slot before two passes and dispatches only once", async () => {
  const calls = [];
  api.efsSnapshot = async (_dest, folder) => { calls.push(["snapshot", folder]); return ok({ path: "snapshot", filesSeen: 82, warnings: [] }); };
  api.efsUpload = async folder => { calls.push(["upload", folder]); return ok({ errors: [], filesSeen: 82, planned: 82, skipped: 0, warnings: [warning] }); };
  const w = wizard("efs"); w.tick(); w.tick(); await settled(w);
  assert.deepEqual(calls.map(([kind]) => kind), ["snapshot", "upload", "upload", "snapshot", "upload", "upload"]);
  assert.equal(calls[0][1], calls[1][1]); assert.equal(calls[3][1], calls[4][1]);
  assert.notEqual(calls[0][1], calls[3][1]);
  assert.equal(w.runSteps[0].status, "done");
  assert.ok(w.runSteps[0].logs.some(line => line.includes("nvPrefixVerification")));
});

test("VoLTE settings and IMS confirmation stages survive wrapper replacement", async () => {
  const w = wizard("volte-props"); let serial;
  api.voltePropsSet = async selected => { serial = selected; return ok(["persist.dbg.volte_avail_ovr"]); };
  w.waitFor = async () => true;
  w.tick(); await settled(w);
  assert.equal(serial, "TEST-SERIAL"); assert.equal(w.runSteps[0].status, "done");
  for (const id of ["comm-check", "final-verify"]) {
    const check = wizard(id); check.imsReady = async () => false;
    check.tick(); await settled(check);
    assert.equal(check.runSteps[0].status, "failed", id);
  }
});
