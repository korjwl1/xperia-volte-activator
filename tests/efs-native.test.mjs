import assert from "node:assert/strict";
import { before, beforeEach, after, test } from "node:test";
import { createServer } from "vite";

let server, Wizard, api, flags, transport, originalApi, originalFlags, originalInvoke, buildPlan, bootPartition, decodeJournal;
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
  ({ bootPartition } = await server.ssrLoadModule("/src/lib/data/devices.ts"));
  ({ decodeJournal } = await server.ssrLoadModule("/src/lib/domain/journal.ts"));
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

test("absent or differently provisioned SIMs never override or block the selected write target", async () => {
  for (const sims of [[], [{ slot: 1, state: "ABSENT", carrier: null }], [{ slot: 1, state: "LOADED", carrier: "KT", plmn: "45008" }]]) {
    let queries = 0;
    const writes = [];
    api.deviceList = async () => { queries++; return []; };
    api.efsSnapshot = async () => ok({ path: "snapshot", warnings: [] });
    api.efsUpload = async folder => { writes.push(folder); return ok({ errors: [], filesSeen: 82, planned: 82, skipped: 0, warnings: [] }); };
    const w = wizard("efs");
    w.device.sims = sims;
    w.volteConfig.sims = [{ slot: 1, carrier: "SKT" }, { slot: 2, carrier: null }];
    w.tick(); await settled(w);
    assert.equal(w.runSteps[0].status, "done");
    assert.equal(writes.length, 2);
    assert.ok(writes.every(folder => folder.endsWith("XPERIAsSKT1")));
    assert.deepEqual(w.volteConfig.sims, [{ slot: 1, carrier: "SKT" }, { slot: 2, carrier: null }]);
    assert.equal(queries, 0);
  }
});

test("missing model procedures stop before local configuration, DIAG, snapshots or writes", async () => {
  for (const [model, carrier] of [["XQ-AT72", "SKT"], ["XQ-AS72", "KT"], ["XQ-CT72", "KT"], ["XQ-CQ72", "LGU"]]) {
    for (const id of ["efs-input", "efs-preflight", "efs"]) {
      let touched = 0;
      api.efsConfiguration = api.efsDiagOpen = api.efsSnapshot = api.efsUpload = async () => { touched++; return ok(configured); };
      const w = wizard(id); w.device.model = model;
      w.volteConfig.sims = [{ slot: 1, carrier }, { slot: 2, carrier: null }];
      w.tick(); await settled(w);
      assert.equal(w.runSteps[0].status, "failed", model + "/" + id);
      assert.equal(touched, 0);
    }
  }
});

test("Mark IV SKT retains the native procedure and PRO-I uses the boot partition", async () => {
  const w = wizard("efs-preflight"); w.device.model = "XQ-CT72";
  w.volteConfig.sims = [{ slot: 1, carrier: "SKT" }, { slot: 2, carrier: null }];
  w.tick(); await settled(w);
  assert.equal(w.runSteps[0].status, "done");
  assert.equal(bootPartition("XQ-BE72"), "boot");
  assert.equal(bootPartition("XQ-DQ44"), "init_boot");
});

test("final IMS polling observes registration without automatically completing the prompt", async () => {
  const previousInterval = globalThis.setInterval, previousClear = globalThis.clearInterval;
  let poll;
  globalThis.setInterval = callback => { poll = callback; return 1; };
  globalThis.clearInterval = () => {};
  const w = wizard("final-verify");
  w.device.sims = [{ slot: 1, state: "LOADED", carrier: "SKT", volte: "on" }, { slot: 2, state: "LOADED", carrier: "SKT", volte: "on" }];
  api.deviceList = async () => [w.device];
  try {
    await w.openManual(w.runSteps[0], "ims-check");
    await new Promise(resolve => setImmediate(resolve));
    await poll();
    assert.equal(w.manualCurrent.id, "ims-check");
    assert.equal(w.runSteps[0].manualDone, 0);
    assert.equal(w.imsRegistered, true);
    assert.equal(w.imsVerified, false);
    assert.equal(w.callVerified, false);
    await w.confirmManual();
    assert.equal(w.imsVerified, true);
    assert.equal(w.callVerified, false);
    assert.equal(w.manualCurrent, null);
  } finally {
    w.stopWatch(); globalThis.setInterval = previousInterval; globalThis.clearInterval = previousClear;
  }
});

test("final confirmation records user call verification separately from IMS", async () => {
  const w = wizard("final-verify");
  w.device.sims = [{ slot: 1, volte: "on" }, { slot: 2, volte: "on" }];
  api.deviceList = async () => [w.device];
  w.manualCurrent = { id: "ims-check" };
  for (const slot of [1, 2]) for (const item of ["outgoing", "incoming", "audio"]) w.setCallCheck(slot, item, true);
  await w.confirmManual();
  assert.equal(w.imsVerified, true);
  assert.equal(w.callVerified, true);
  assert.equal(w.imsUnverified, false);
});

test("no SIM can finish communication checking immediately without losing file verification", () => {
  const w = wizard("final-verify");
  const verified = { id: "verify", status: "done" };
  w.runSteps.push(verified);
  w.manualCurrent = { id: "ims-check" };
  w.finishWithoutIms();
  assert.equal(w.manualCurrent, null);
  assert.equal(w.imsUnverified, true);
  assert.equal(w.imsVerified, false);
  assert.equal(w.callVerified, false);
  assert.equal(verified.status, "done");
  assert.equal(w.runSteps[0].manualDone, 1);
});

test("skipping the pre-unroot network check does not block device work or authorize relock", async () => {
  const w = wizard("comm-check");
  w.device.sims = [];
  w.manualCurrent = { id: "ims-precheck" };
  w.finishWithoutIms();
  assert.equal(w.runSteps[0].communicationSkipped, true);
  api.deviceList = async () => { assert.fail("skipped network check must not query SIMs"); };
  await w.runRealCommCheck(w.runSteps[0]);
  assert.equal(w.runSteps[0].status, "done");
  assert.equal(w.imsVerified, false);
  assert.equal(w.callVerified, false);
  assert.equal(w.imsUnverified, false);
  const relock = wizard("relock");
  flags.fastboot = true;
  let writes = 0;
  api.fastbootRelock = async () => { writes++; return ok(null); };
  relock.tick(); await settled(relock);
  assert.equal(writes, 0);
  assert.equal(relock.runSteps[0].status, "failed");
});

test("late IMS confirmation cannot mark a restarted session as verified", async () => {
  const pending = deferred();
  const w = wizard("final-verify");
  w.manualCurrent = { id: "ims-check" };
  for (const slot of [1, 2]) for (const item of ["outgoing", "incoming", "audio"]) w.setCallCheck(slot, item, true);
  api.deviceList = () => pending.promise;
  const confirming = w.confirmManual();
  w.restart();
  pending.resolve([{ ...w.device, state: "device", serial: "TEST-SERIAL", sims: [{ slot: 1, volte: "on" }, { slot: 2, volte: "on" }] }]);
  await confirming;
  assert.equal(w.imsVerified, false);
  assert.equal(w.callVerified, false);
});

test("legacy or changed-SIM journals recheck communication without rewriting user selection or file checks", () => {
  for (const changedSim of [false, true]) {
    const w = wizard("final-verify");
    w.device.sims = [{ slot: 1, carrier: "KT", state: "LOADED" }];
    const config = { sims: [{ slot: 1, carrier: "SKT" }, { slot: 2, carrier: null }], firmware: null, bootloaderAction: null };
    const steps = ["verify", "final-verify"].map(id => ({ id, kind: id === "verify" ? "verify" : "final-verify", title: id, desc: "", risk: "safe", wipe: false, estSec: 1, optional: false, enabled: true, ...(id === "final-verify" ? { manual: ["ims-check"] } : {}) }));
    const journal = { version: 1, model: w.device.model, productName: "Xperia", serialMasked: "TEST", startedAt: "", updatedAt: "", backupPath: "", firmwareDir: "", config, opts: { unroot: false, relock: false, restore: false }, backupItems: [], steps, runSteps: steps.map(s => ({ id: s.id, title: s.title, status: "done", progress: 1, logs: [], manualDone: s.manual?.length ?? 0 })), cursor: 2, firmware: null, stop: null,
      ...(changedSim ? { sims: [{ slot: 1, carrier: "SKT", state: "LOADED" }], imsVerified: true, callVerified: true } : {}) };
    assert.ok(decodeJournal(JSON.stringify(journal)));
    assert.equal(decodeJournal(JSON.stringify({ ...journal, callVerified: "true" })), null);
    w.pendingJournal = journal;
    w.resumeJournal();
    assert.equal(w.runSteps[0].status, "done");
    assert.equal(w.runSteps[1].status, "pending");
    assert.equal(w.imsVerified, false);
    assert.equal(w.callVerified, false);
    assert.deepEqual(w.volteConfig.sims, config.sims);
  }
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

const registeredSim = slot => ({ slot, type: "physical", carrier: "KT", state: "LOADED", volte: "on", ims: { status: "registered", registration: "registered", voice: true, sms: false, transport: "cellular", technology: "lte" } });
function diagnosticDevice(w, sims = [registeredSim(1), registeredSim(2)]) {
  return { ...w.device, productName: "Xperia", serialMasked: "TEST****", firmware: "test-fw", fingerprint: "test-build", android: "15", baseband: "test-modem", sims };
}

test("read-only preflight records metadata and selected preset without SIM overrides or identifiers", async () => {
  const w = wizard("efs-input");
  w.device = diagnosticDevice(w, [{ ...registeredSim(1), _plmn: "45008", subscriberId: "must-not-persist" }, { slot: 2, type: "esim", carrier: null, state: "ABSENT", volte: "unknown" }]);
  w.volteConfig.sims = [{ slot: 1, carrier: "SKT" }, { slot: 2, carrier: null }];
  api.deviceList = async () => [w.device];
  const originalSelection = structuredClone(w.volteConfig);
  assert.equal(await w.refreshCommunication("before"), true);
  assert.equal(w.communicationBefore.baseband, "test-modem");
  assert.equal(w.communicationBefore.presets[0].carrier, "SKT");
  assert.match(w.communicationBefore.presets[0].sha256, /^[a-f0-9]{64}$/);
  assert.equal(w.communicationBefore.sims[0].carrier, "KT");
  assert.doesNotMatch(JSON.stringify(w.communicationBefore), /subscriberId|_plmn|TEST-SERIAL/);
  assert.deepEqual(w.volteConfig, originalSelection);
  const before = w.communicationBefore;
  w.steps = []; w.prepareRun();
  assert.deepEqual(w.communicationBefore, before);
});

test("enriched IMS read failures take precedence over old on flags and clear stale final badges", async () => {
  const w = wizard("final-verify"); w.device = diagnosticDevice(w); w.finished = true;
  api.deviceList = async () => [w.device];
  assert.equal(await w.refreshCommunication(), true);
  w.imsVerified = true;
  api.deviceList = async () => [{ ...w.device, sims: [ { ...registeredSim(1), ims: { ...registeredSim(1).ims, status: "query-failed" } }, registeredSim(2) ] }];
  assert.equal(await w.refreshCommunication(), false);
  assert.equal(w.imsVerified, false);
  api.deviceList = async () => { throw Error("read failed"); };
  assert.equal(await w.refreshCommunication(), false);
  assert.equal(w.communicationLatest.outcome, "query-failed");
  assert.deepEqual(w.imsSims, []);
  assert.match(w.communicationError, /조회 실패/);
  assert.equal(w.imsVerified, false);
});

test("communication verdict uses selected slots, ignoring another slot's Wi-Fi registration", async () => {
  const w = wizard("final-verify");
  w.device = diagnosticDevice(w, [registeredSim(1), { ...registeredSim(2), volte: "wifi", ims: { ...registeredSim(2).ims, status: "wifi-only", transport: "wifi" } }]);
  w.volteConfig.sims = [{ slot: 1, carrier: "SKT" }, { slot: 2, carrier: null }];
  api.deviceList = async () => [w.device];
  assert.equal(await w.refreshCommunication(), true);
  w.volteConfig.sims[1].carrier = "SKT";
  assert.equal(await w.refreshCommunication(), false);
});

test("slot call proof requires outgoing, incoming and two-way audio; reboot and idle stay independent", () => {
  const w = wizard("final-verify");
  for (const slot of [1, 2]) for (const item of ["outgoing", "incoming", "afterReboot", "afterIdle"]) w.setCallCheck(slot, item, true);
  assert.equal(w.callAck, false);
  w.setCallCheck(1, "audio", true); assert.equal(w.callAck, false);
  w.setCallCheck(2, "audio", true); assert.equal(w.callAck, true);
  w.setCallCheck(1, "afterIdle", false); assert.equal(w.callAck, true);
  w.finished = true; w.setCallCheck(2, "incoming", false);
  assert.equal(w.callVerified, false);
});

test("completion recheck updates communication only, invalidating changed SIM proof", async () => {
  const w = wizard("final-verify"); w.finished = true; w.device = diagnosticDevice(w);
  const fileCheck = { id: "verify", status: "done", logs: [] }; w.runSteps.push(fileCheck);
  api.deviceList = async () => [w.device];
  api.efsUpload = api.rootReboot = async () => assert.fail("read-only checks cannot write or reboot");
  await w.refreshCommunication();
  for (const slot of [1, 2]) for (const item of ["outgoing", "incoming", "audio"]) w.setCallCheck(slot, item, true);
  assert.equal(w.callVerified, true);
  api.deviceList = async () => [diagnosticDevice(w, [{ ...registeredSim(1), carrier: "SK Telecom" }, registeredSim(2)])];
  await w.refreshCommunication();
  assert.equal(w.imsVerified, true); assert.equal(w.callVerified, false);
  assert.deepEqual(w.callChecks, []); assert.equal(fileCheck.status, "done");
});

test("late diagnostic replies cannot replace a newer observation or a changed target selection", async () => {
  const w = wizard("final-verify"); w.device = diagnosticDevice(w);
  const old = deferred(); api.deviceList = () => old.promise;
  const waiting = w.refreshCommunication();
  api.deviceList = async () => [];
  await w.refreshCommunication();
  old.resolve([w.device]); assert.equal(await waiting, false);
  assert.equal(w.communicationLatest.outcome, "disconnected");
  const changed = deferred(); api.deviceList = () => changed.promise;
  const stale = w.refreshCommunication(); w.volteConfig.sims[0].carrier = "KT";
  changed.resolve([w.device]); assert.equal(await stale, false);
  assert.equal(w.communicationLatest.outcome, "disconnected");
  assert.equal(w.communicationLoading, false);
});

test("completed diagnostics persist as archived records with validated slot evidence", async () => {
  const w = wizard("final-verify"); w.device = diagnosticDevice(w);
  w.journalKey = "test-key"; w.finished = true;
  w.steps = [{ id: "final-verify", kind: "final-verify", title: "final", desc: "", risk: "safe", wipe: false, estSec: 1, optional: false, enabled: true }];
  w.runSteps[0].status = "done";
  const writes = [];
  api.journalSave = async (_key, data) => { writes.push(["save", data]); return true; };
  api.journalArchive = async () => { writes.push(["archive"]); return true; };
  api.deviceList = async () => [w.device];
  await w.refreshCommunication(); await w.persist(true);
  assert.deepEqual(writes.map(x => x[0]), ["save", "archive", "save", "archive"]);
  const journal = JSON.parse(writes[0][1]);
  assert.ok(decodeJournal(JSON.stringify(journal)));
  journal.communication.calls = [{ slot: 1, outgoing: true, incoming: true, audio: "true", afterReboot: false, afterIdle: false }];
  assert.equal(decodeJournal(JSON.stringify(journal)), null);
  journal.communication.calls[0].audio = true;
  assert.ok(decodeJournal(JSON.stringify(journal)));
  journal.communication.calls.push(journal.communication.calls[0]);
  assert.equal(decodeJournal(JSON.stringify(journal)), null);
  const before = writes.length; api.journalSave = async () => false;
  assert.equal(await w.persist(true), false); assert.equal(writes.length, before);
});

test("a poll arriving after communication was skipped cannot turn the finish screen green", async () => {
  const w = wizard("final-verify"); w.device = diagnosticDevice(w);
  const pending = deferred(); api.deviceList = () => pending.promise;
  w.manualCurrent = { id: "ims-check" };
  const checking = w.refreshCommunication();
  w.finishWithoutIms(); w.finished = true;
  pending.resolve([w.device]);
  assert.equal(await checking, false);
  assert.equal(w.imsVerified, false); assert.equal(w.imsUnverified, true);
  assert.equal(w.communicationLatest, null);
});

test("unroot-only communication uses freshly observed slots without inventing patch targets", async () => {
  const w = wizard("comm-check"); w.device = diagnosticDevice(w, []);
  w.volteConfig.sims = [{ slot: 1, carrier: null }, { slot: 2, carrier: null }];
  api.deviceList = async () => [diagnosticDevice(w, [registeredSim(2)])];
  assert.equal(await w.refreshCommunication(), true);
  assert.deepEqual(w.communicationSlots, [2]);
  for (const item of ["outgoing", "incoming", "audio"]) w.setCallCheck(2, item, true);
  assert.equal(w.callAck, true);
  assert.ok(w.volteConfig.sims.every(s => s.carrier === null));
});

test("changed baseband on resume invalidates detailed communication proof but preserves file checkpoints", async () => {
  const w = wizard("final-verify"); w.device = diagnosticDevice(w); api.deviceList = async () => [w.device];
  await w.refreshCommunication();
  w.communicationLatest.baseband = "old-modem";
  const steps = ["verify", "final-verify"].map(id => ({ id, kind: id === "verify" ? "verify" : "final-verify", title: id, desc: "", risk: "safe", wipe: false, estSec: 1, optional: false, enabled: true, ...(id === "final-verify" ? { manual: ["ims-check"] } : {}) }));
  const journal = { version: 1, model: w.device.model, productName: "Xperia", serialMasked: "TEST", startedAt: "", updatedAt: "", backupPath: "", firmwareDir: "", config: { ...w.volteConfig }, opts: { unroot: false, relock: false, restore: false }, backupItems: [], steps, runSteps: steps.map(s => ({ id: s.id, title: s.title, status: "done", progress: 1, logs: [], manualDone: s.manual?.length ?? 0 })), cursor: 2, firmware: null, stop: null, imsVerified: true, callVerified: true,
    communication: { before: null, latest: w.communicationLatest, calls: [1, 2].map(slot => ({ slot, outgoing: true, incoming: true, audio: true, afterReboot: false, afterIdle: false })) } };
  assert.ok(decodeJournal(JSON.stringify(journal)));
  w.pendingJournal = journal; w.resumeJournal();
  assert.equal(w.runSteps[0].status, "done"); assert.equal(w.runSteps[1].status, "pending");
  assert.equal(w.imsVerified, false); assert.equal(w.callVerified, false);
  assert.equal(w.communicationLatest, null); assert.deepEqual(w.callChecks, []);
});
