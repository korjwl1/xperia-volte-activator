import assert from "node:assert/strict";
import { before, beforeEach, after, test } from "node:test";
import { createServer } from "vite";

let server, Wizard, api, flags, transport, originalApi, originalFlags, originalInvoke, buildPlan;
const configured = { port: "COM9", presetRoot: "C:/bundle", snapshotRoot: "C:/snapshots" };
const ok = value => ({ ok: true, value });
const warning = { code: "nvPrefixVerification", target: "NV 71", message: "Only explicit bytes verified" };
before(async () => {
  server = await createServer({ server: { middlewareMode: true, watch: null, hmr: false, ws: false }, appType: "custom" });
  ({ Wizard } = await server.ssrLoadModule("/src/lib/stores/wizard.svelte.ts"));
  ({ api } = await server.ssrLoadModule("/src/lib/api/index.ts"));
  ({ REAL_STEPS: flags } = await server.ssrLoadModule("/src/lib/data/runMode.ts"));
  ({ transport } = await server.ssrLoadModule("/src/lib/api/transport.ts"));
  ({ buildPlan } = await server.ssrLoadModule("/src/lib/domain/plan.ts"));
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
    efsToolCheck: async () => ok({ native: true, deviceExecution: true, rootExecution: true, fastbootExecution: true, path: "built-in", version: "test" }),
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
  const check = wizard("comm-check"); check.imsReady = async () => false;
  check.tick(); await settled(check);
  assert.equal(check.runSteps[0].status, "failed");
  const final = wizard("final-verify"); final.waitFor = async () => true;
  let opened;
  api.rootReboot = async () => ok(null);
  final.openManual = async (_step, id) => { opened = id; };
  final.tick(); await settled(final);
  assert.equal(opened, "ims-check");
  assert.equal(final.runSteps[0].status, "manual-wait");
});

test("preset conflict is checked before unlock, rooting, backup or DIAG", async () => {
  const w = wizard("efs-input");
  const plan = buildPlan({ ...w.device, bootloader: "locked", rooted: false, prep: {} }, w.volteConfig, { unroot: true, relock: true, restore: true }, true);
  assert.equal(plan[0].id, "efs-input");
  assert.ok(plan.findIndex(step => step.id === "efs-input") < plan.findIndex(step => step.id === "unlock"));
  assert.deepEqual(plan.find(step => step.id === "efs-preflight").manual, ["su-grant"]);
  let calls = 0;
  api.efsDiagOpen = api.efsUpload = api.fastbootUnlock = async () => { calls++; return ok(null); };
  api.efsValidatePresets = async () => ({ ok: false, error: "presetConflict" });
  w.tick(); await settled(w);
  assert.equal(w.runSteps[0].status, "failed");
  assert.equal(calls, 0);
});

test("real EFS disables mock errors and manual skipping", async () => {
  const w = wizard("efs"); w.simulateEfsFail = true; w.simulateUsbError = true;
  api.efsSnapshot = async () => ok({ path: "snapshot", warnings: [] });
  api.efsUpload = async () => ok({ errors: [], filesSeen: 1, planned: 1, skipped: 0, warnings: [] });
  w.tick(); await settled(w);
  assert.equal(w.runSteps[0].status, "done");
  assert.equal(w.usbError, false);
  w.manualCurrent = { id: "su-grant" };
  assert.equal(w.manualSkippable, false);
});

test("native EFS refuses simulated prerequisite engines before local or device work", async () => {
  for (const id of ["unlock", "root", "unroot", "fw-flash"]) {
    let calls = 0;
    api.efsConfiguration = api.efsDiagOpen = api.efsUpload = async () => { calls++; return ok(configured); };
    const w = wizard("efs-input");
    w.runSteps.push({ id, title: id, status: "pending", progress: 0, logs: [], manualDone: 0 });
    w.tick(); await settled(w);
    assert.equal(w.runSteps[0].status, "failed", id);
    assert.equal(calls, 0, id);
  }
});

test("retry, repatch and USB resume preserve state until previous I/O settles", () => {
  for (const action of ["retryStep", "repatch", "dismissUsbError"]) {
    const w = wizard("efs-preflight");
    w.runSteps[0].status = "failed"; w.stepError = "failed"; w.usbError = true;
    w.busy = 1; let begun = 0; w.begin = () => begun++;
    w[action]();
    assert.equal(w.runSteps[0].status, "failed", action);
    assert.equal(w.usbError, true, action);
    assert.equal(w.stepError, "failed", action);
    assert.equal(begun, 0, action);
  }
});

test("missing native build features block EFS plans before unlock or root", async () => {
  flags.root = true; flags.fastboot = true;
  for (const [id, missing] of [["unlock", "fastbootExecution"], ["root", "rootExecution"], ["unroot", "fastbootExecution"]]) {
    let writes = 0;
    api.fastbootUnlock = api.efsDiagOpen = api.efsUpload = async () => { writes++; return ok(null); };
    api.efsToolCheck = async () => ok({ deviceExecution: true, rootExecution: true, fastbootExecution: true, [missing]: false });
    const w = wizard("efs-input");
    w.runSteps.push({ id, title: id, status: "pending", progress: 0, logs: [], manualDone: 0 });
    w.tick(); await settled(w);
    assert.equal(w.runSteps[0].status, "failed", id);
    assert.match(w.stepError, /기능이 빌드에 없습니다/, id);
    assert.equal(writes, 0, id);
  }
});

test("VoLTE completion requires a disconnect and reconnection", async () => {
  const w = wizard("volte-props");
  api.voltePropsSet = async () => ok([]);
  api.deviceList = async () => [w.device];
  w.waitFor = async (_gen, check) => check();
  w.tick(); await settled(w);
  assert.equal(w.runSteps[0].status, "failed");
  assert.match(w.stepError, /재부팅이 감지되지/);
});

test("configured run keeps its original COM and preset root after settings change", async () => {
  const w = wizard("efs");
  const used = [];
  api.efsSnapshot = async (_dest, _folder, cfg) => { used.push(cfg); return ok({ path: "snapshot", warnings: [] }); };
  api.efsUpload = async (_folder, cfg) => {
    used.push(cfg);
    api.efsConfiguration = async () => ok({ ...configured, port: "COM22", presetRoot: "C:/another" });
    return ok({ errors: [], filesSeen: 1, planned: 1, skipped: 0, warnings: [] });
  };
  w.tick(); await settled(w);
  assert.ok(used.length > 2);
  assert.ok(used.every(cfg => cfg.port === "COM9" && cfg.presetRoot === "C:/bundle"));
  assert.deepEqual(await w.validatedEfsInputs(w.runGen), configured);
});
