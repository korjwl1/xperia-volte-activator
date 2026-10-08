import assert from "node:assert/strict";
import { after, before, beforeEach, test } from "node:test";
import { createServer } from "vite";

let server, Wizard, buildPlan, manualTaskProblem, updateProblem, decodeJournal, api, flags, originalApi, originalFlags;
before(async () => {
  server = await createServer({ server: { middlewareMode: true, watch: null, hmr: false, ws: false }, appType: "custom" });
  ({ Wizard } = await server.ssrLoadModule("/src/lib/stores/wizard.svelte.ts"));
  ({ buildPlan } = await server.ssrLoadModule("/src/lib/domain/plan.ts"));
  ({ manualTaskProblem, updateProblem } = await server.ssrLoadModule("/src/lib/domain/workflow.ts"));
  ({ decodeJournal } = await server.ssrLoadModule("/src/lib/domain/journal.ts"));
  ({ api } = await server.ssrLoadModule("/src/lib/api/index.ts"));
  ({ REAL_STEPS: flags } = await server.ssrLoadModule("/src/lib/data/runMode.ts"));
  originalApi = { ...api }; originalFlags = { ...flags };
});
beforeEach(() => {
  Object.assign(api, originalApi, { efsCancel: async () => true, backupCancel: async () => true });
  Object.assign(flags, Object.fromEntries(Object.keys(flags).map(key => [key, false])));
});
after(async () => { Object.assign(api, originalApi); Object.assign(flags, originalFlags); await server.close(); });
const device = (extra = {}) => ({ model: "XQ-DQ44", productName: "Xperia", serial: "sample", serialMasked: "sa****", firmware: "v1", android: "15", state: "device", rooted: true, bootloader: "unlocked", prep: { developerOptions: true, usbDebugging: true, oemUnlockAllowed: true }, sims: [{ slot: 1, state: "LOADED", carrier: "SKT", volte: "on" }], ...extra });
const config = () => ({ sims: [{ slot: 1, carrier: "SKT" }, { slot: 2, carrier: null }], firmware: "v2", bootloaderAction: "unlock" });
const opts = (task) => ({ mode: "manual", manualTask: task, unroot: true, relock: true, restore: true });
const deferred = () => { let resolve; const promise = new Promise(r => resolve = r); return { promise, resolve }; };
function runner(ids, mode = "automatic") {
  const w = new Wizard(); w.device = device(); w.opts = { mode, unroot: false, relock: false, restore: false };
  w.steps = ids.map(id => ({ id, kind: "setup", enabled: true, manual: [] }));
  w.runSteps = ids.map(id => ({ id, title: id, status: "pending", progress: 0, logs: [], manualDone: 0 }));
  w.runSteps[0].status = "running";
  w.persist = async () => true; w.setGuard = () => {}; w.begin = () => { w.started = (w.started ?? 0) + 1; };
  return w;
}

test("Start opens the mode selector and changing paths removes unrelated selections", async () => {
  const w = new Wizard(); w.device = device(); w.setGuard = () => {};
  w.startSession(); assert.equal(w.view, "mode-select");
  w.chooseMode("manual"); assert.equal(w.view, "manual-tasks");
  await w.chooseManualTask("unroot"); assert.equal(w.view, "warning"); assert.equal(w.manualTask, "unroot");
  w.volteConfig = config(); w.opts.relock = true;
  w.chooseMode("update"); assert.equal(w.view, "step1");
  assert.deepEqual(w.volteConfig.sims.map(s => s.carrier), [null, null]);
  assert.equal(w.volteConfig.bootloaderAction, null); assert.equal(w.opts.relock, false);
  w.chooseMode("automatic"); assert.equal(w.view, "warning"); assert.equal(w.manualTask, null);
});
test("update requires cellular IMS, accepts one ready dual-SIM slot and rejects conflicting evidence", () => {
  assert.equal(updateProblem(device()), null);
  assert.equal(updateProblem(device({ sims: [{ slot: 1, volte: "off" }, { slot: 2, volte: "on" }] })), null);
  for (const sims of [[], [{ volte: "off" }], [{ volte: "wifi" }], [{ volte: "unknown" }], [{ volte: "on", ims: { status: "wifi-only", registration: "registered", voice: true, transport: "wifi" } }]]) assert.ok(updateProblem(device({ sims })));
  assert.ok(updateProblem(null)); assert.ok(updateProblem(device({ state: "unauthorized" })));
});
test("manual card eligibility handles lock, root, unknown and unsupported-device conditions", () => {
  assert.ok(manualTaskProblem("unlock", device()));
  assert.equal(manualTaskProblem("unlock", device({ bootloader: "locked" })), null);
  for (const task of ["relock", "root", "unroot", "volte"]) assert.ok(manualTaskProblem(task, device({ bootloader: "locked" })));
  assert.equal(manualTaskProblem("root", device({ rooted: false })), null);
  assert.ok(manualTaskProblem("unroot", device({ rooted: false })));
  assert.ok(manualTaskProblem("volte", device({ rooted: null })));
  assert.ok(manualTaskProblem("root", device({ model: "unknown" })));
  assert.equal(manualTaskProblem("backup", device({ rooted: false, bootloader: "locked" })), null);
});
test("single manual task plans ignore leftover SIM, firmware and post-processing options", () => {
  for (const [task, expected] of [["restore", ["restore"]], ["root", ["prep", "backup", "root"]], ["unroot", ["prep", "backup", "unroot"]], ["relock", ["prep", "backup", "unroot", "relock", "setup-relock"]]]) {
    const d = device({ rooted: task === "root" ? false : true });
    assert.deepEqual(buildPlan(d, config(), opts(task), true).map(s => s.id), expected);
  }
  const patch = buildPlan(device(), config(), opts("volte"), true).map(s => s.id);
  for (const forbidden of ["unlock", "unroot", "relock", "root", "restore", "fw-download", "fw-flash"]) assert.ok(!patch.includes(forbidden), forbidden);
  assert.deepEqual(buildPlan(device({ bootloader: "locked" }), config(), opts("root"), true), []);
});
test("update plan excludes other device writes and direct launch stays blocked", () => {
  const w = new Wizard(); w.device = device(); w.volteConfig = config();
  w.opts = { mode: "update", unroot: true, relock: true, restore: true };
  w.groups = [{ items: [{ checked: true }] }];
  assert.deepEqual(w.plan.map(s => s.id), ["backup", "fw-download", "fw-flash", "fw-verify", "final-verify"]);
  w.launch(); assert.equal(w.view, "device"); assert.equal(w.runSteps.length, 0);
});
test("automatic firmware preparation uses the installed version and ignores stale update choices", () => {
  const w = new Wizard(); w.device = device(); w.volteConfig = config();
  w.opts = { mode: "automatic", unroot: false, relock: false, restore: false };
  assert.equal(w.updateVersion, null);
  for (const step of w.plan) assert.ok(!["fw-download", "fw-flash", "fw-verify", "unlock"].includes(step.id), step.id);
  w.volteConfig.sims.forEach(sim => sim.carrier = null);
  assert.equal(w.hasAnyTask, false); assert.deepEqual(w.plan, []);
  const old = runner(["fw-flash"]); old.begin = Wizard.prototype.begin;
  old.begin(); assert.equal(old.runSteps[0].status, "failed"); assert.equal(old.running, false);
});
test("firmware server version lookup is available only in the dedicated update route", async () => {
  let calls = 0; api.firmwareVersions = async () => { calls++; return { versions: [] }; };
  const w = new Wizard(); w.device = device(); w.opts = { mode: "automatic", unroot: false, relock: false, restore: false };
  w.ensureFirmwareVersions(); assert.equal(calls, 0);
  w.opts.mode = "manual"; w.ensureFirmwareVersions(); assert.equal(calls, 0);
  w.opts.mode = "update"; w.ensureFirmwareVersions(); await Promise.resolve(); assert.equal(calls, 1);
});
test("automatic and manual runners advance non-backup completion but wait after backup", async () => {
  for (const mode of ["automatic", "manual"]) {
    const w = runner(["prep", "backup", "root"], mode);
    w.stepDone(w.runSteps[0]); await Promise.resolve();
    assert.equal(w.awaitingNext, null); assert.equal(w.started, 1);
    w.runSteps[1].status = "running"; w.stepDone(w.runSteps[1]); await Promise.resolve();
    assert.equal(w.awaitingNext, "backup"); assert.equal(w.started, 1);
    w.nextStep(); await Promise.resolve(); assert.equal(w.awaitingNext, null);
    const last = runner(["backup"], mode); last.stepDone(last.runSteps[0]);
    assert.equal(last.finished, true); assert.equal(last.awaitingNext, null); assert.equal(last.started, undefined);
  }
});
test("next step waits for engine cleanup and abort invalidates its continuation", async () => {
  for (const cancel of [false, true]) {
    const w = runner(["prep", "root"]); const gate = deferred(); const order = [];
    w.begin = () => order.push("next");
    w.dispatchEngine(async () => {
      try { w.stepDone(w.runSteps[0]); await gate.promise; }
      finally { order.push("cleanup"); }
    });
    await Promise.resolve(); assert.deepEqual(order, []); assert.equal(w.busy, 1);
    if (cancel) w.abort(); gate.resolve();
    await new Promise(resolve => setImmediate(resolve));
    assert.deepEqual(order, cancel ? ["cleanup"] : ["cleanup", "next"]);
    assert.equal(w.busy, 0); assert.equal(w.dangerBusy, 0);
  }
});
test("manual restore rejects incomplete, foreign and changed backups before any restore write", async () => {
  const w = runner(["restore"], "manual"); w.opts.manualTask = "restore";
  w.deviceKeyHex = async () => "a".repeat(64);
  w.groups = [{ items: [{ id: "dcim", checked: false }] }];
  const valid = { complete: true, deviceKey: "a".repeat(64), errors: [], items: [{ id: "dcim", status: "done" }] };
  for (const invalid of [{ ...valid, complete: false }, { ...valid, deviceKey: "b".repeat(64) }]) {
    api.backupManifestCheck = async () => invalid;
    await w.loadRestoreSource("backup"); assert.equal(w.backupDir, ""); assert.equal(w.restoreSourceState, "failed");
  }
  api.backupManifestCheck = async () => valid;
  await w.loadRestoreSource("backup"); assert.equal(w.restoreSourceState, "done"); assert.equal(w.groups[0].items[0].checked, true);
  let writes = 0; api.restoreRun = async () => { writes++; };
  api.backupManifestCheck = async () => ({ ...valid, complete: false });
  await w.runRealRestore(w.runSteps[0]); assert.equal(writes, 0); assert.equal(w.runSteps[0].status, "failed");
});
test("workflow journals preserve mode/task, accept old records and reject extra single-task writes", () => {
  const d = device({ rooted: false }); const plan = buildPlan(d, { ...config(), firmware: null, bootloaderAction: null }, { ...opts("root"), unroot: false, relock: false, restore: false }, false);
  const j = { version: 1, model: d.model, productName: d.productName, serialMasked: d.serialMasked, startedAt: "2026-10-08", updatedAt: "2026-10-08", config: { sims: [{ slot: 1, carrier: null }, { slot: 2, carrier: null }], firmware: null, bootloaderAction: null }, opts: { mode: "manual", manualTask: "root", unroot: false, relock: false, restore: false }, backupPath: "", firmwareDir: "", firmware: null, backupItems: [], cursor: 0, steps: plan, runSteps: plan.map(s => ({ id: s.id, title: s.title, status: "pending", progress: 0, manualDone: 0, logs: [] })), stop: null };
  assert.ok(decodeJournal(JSON.stringify(j)));
  j.opts.manualTask = "restore"; assert.equal(decodeJournal(JSON.stringify(j)), null);
  delete j.opts.mode; delete j.opts.manualTask; assert.ok(decodeJournal(JSON.stringify(j)));
});
