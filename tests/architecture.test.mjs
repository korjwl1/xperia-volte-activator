import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { after, before, beforeEach, test } from "node:test";
import { createServer } from "vite";

let server, Wizard, api, flags, originalApi, originalFlags, createTransport, AsyncQueue, decodeJournal, buildPlan, stepHazard;
before(async () => {
  server = await createServer({ server: { middlewareMode: true, watch: null, hmr: false, ws: false }, appType: "custom" });
  ({ Wizard } = await server.ssrLoadModule("/src/lib/stores/wizard.svelte.ts"));
  ({ api } = await server.ssrLoadModule("/src/lib/api/index.ts"));
  ({ REAL_STEPS: flags } = await server.ssrLoadModule("/src/lib/data/runMode.ts"));
  ({ createTransport } = await server.ssrLoadModule("/src/lib/api/transport.ts"));
  ({ AsyncQueue } = await server.ssrLoadModule("/src/lib/domain/asyncQueue.ts"));
  ({ decodeJournal } = await server.ssrLoadModule("/src/lib/domain/journal.ts"));
  ({ buildPlan, stepHazard } = await server.ssrLoadModule("/src/lib/domain/plan.ts"));
  originalApi = { ...api };
  originalFlags = { ...flags };
});
beforeEach(() => {
  Object.assign(api, originalApi, { journalSave: async () => true, runGuard: async () => true });
  Object.assign(flags, { backup: true, restore: true, fastboot: false });
});
after(async () => {
  Object.assign(api, originalApi);
  Object.assign(flags, originalFlags);
  await server?.close();
});
const deferred = () => {
  let resolve;
  const promise = new Promise(r => { resolve = r; });
  return { promise, resolve };
};
function wizard() {
  const w = new Wizard();
  w.device = { model: "XQ-DQ44", productName: "Xperia", serial: "AB123456789", serialMasked: "AB1234****",
    firmware: "v1", bootloader: "locked", rooted: false, state: "device",
    prep: { developerOptions: true, usbDebugging: true, oemUnlockAllowed: true }, sims: [] };
  w.begin = () => {};
  w.setGuard = () => {};
  w.runSteps = [{ id: "restore", title: "복원", status: "running", progress: 0, logs: [], manualDone: 0 }];
  return w;
}
function journal() {
  const w = wizard();
  const steps = [{ id: "backup", kind: "backup", title: "백업", desc: "", risk: "safe",
    enabled: true, optional: false, wipe: false, estSec: 1 }];
  return { version: 1, model: w.device.model, productName: "Xperia", serialMasked: w.device.serialMasked,
    startedAt: "2026-10-04", updatedAt: "2026-10-04", backupPath: "", firmwareDir: "",
    config: { sims: [{ slot: 1, carrier: null }, { slot: 2, carrier: null }], firmware: null, bootloaderAction: null },
    opts: { unroot: false, relock: false, restore: true }, backupItems: ["dcim"], steps,
    runSteps: [{ id: "backup", title: "백업", status: "pending", progress: 0, logs: [], manualDone: 0 }],
    cursor: 0, firmware: null, stop: null };
}

test("transport never touches a backend during SSR and preserves operation errors", async () => {
  let calls = 0;
  const port = { available: () => false, invoke: async () => { calls++; }, listen: async () => { calls++; } };
  const transport = createTransport(port);
  assert.equal((await transport.result("write")).ok, false);
  assert.equal(await transport.optional("read"), null);
  (await transport.subscribe("event", () => {}))();
  assert.equal(calls, 0);
  port.available = () => true;
  port.invoke = async () => { throw new Error("disconnected"); };
  assert.deepEqual(await transport.result("write"), { ok: false, error: "disconnected" });
});

test("queued writes and archive retain order even after a failed write", async () => {
  const queue = new AsyncQueue();
  const gate = deferred(), events = [];
  const first = queue.push(async () => { events.push("save-start"); await gate.promise; throw new Error("disk"); });
  const firstResult = first.catch(error => error.message);
  const archive = queue.push(async () => { events.push("archive"); return true; });
  await Promise.resolve();
  assert.deepEqual(events, ["save-start"]);
  gate.resolve();
  assert.equal(await firstResult, "disk");
  assert.equal(await archive, true);
  assert.deepEqual(events, ["save-start", "archive"]);
});

test("disk journal rejects malformed configuration, cursor and execution structure", () => {
  const valid = journal();
  assert.ok(decodeJournal(JSON.stringify(valid)));
  for (const mutate of [
    j => { j.cursor = 2; },
    j => { j.config.sims[1].slot = 1; },
    j => { j.runSteps[0].id = "unlock"; },
    j => { j.runSteps[0].progress = 1.01; },
    j => { j.runSteps[0].manualDone = 1; },
    j => { j.opts.restore = "true"; },
    j => { j.patchedImage = {}; },
  ]) {
    const copy = structuredClone(valid); mutate(copy);
    assert.equal(decodeJournal(JSON.stringify(copy)), null);
  }
  assert.equal(decodeJournal("{"), null);
});

test("journals for generated unlock, relock and patch plans pass structural validation", () => {
  for (const action of ["unlock", "relock", "patch"]) {
    const w = wizard(), j = journal();
    w.device.rooted = false;
    if (action === "relock") w.device.bootloader = "unlocked";
    if (action === "patch") j.config.sims[0].carrier = "SKT";
    else j.config.bootloaderAction = action;
    j.steps = buildPlan(w.device, j.config, j.opts, true);
    j.runSteps = j.steps.filter(step => step.enabled).map(step =>
      ({ id: step.id, title: step.title, status: "pending", progress: 0, logs: [], manualDone: 0 }));
    assert.ok(decodeJournal(JSON.stringify(j)), action);
  }
});

test("plan generator is deterministic and leaves inputs unchanged", () => {
  const w = wizard(), config = journal().config;
  config.bootloaderAction = "unlock";
  const before = JSON.stringify([w.device, config, w.opts]);
  const a = buildPlan(w.device, config, w.opts, true);
  assert.deepEqual(a, buildPlan(w.device, config, w.opts, true));
  assert.equal(JSON.stringify([w.device, config, w.opts]), before);
  assert.ok(a.some(step => step.id === "unlock"));
});

test("restore failures cannot be reported as completed and listeners are released", async () => {
  const w = wizard(); w.backupDir = "backup";
  let off = 0;
  api.onRestoreProgress = async () => () => off++;
  api.restoreRun = async () => ({ ok: true, value: { logs: [], failures: ["APK install failed"], smsiePending: false } });
  await w.runRealRestore(w.runSteps[0]);
  assert.equal(w.runSteps[0].status, "failed");
  assert.match(w.stepError, /APK install failed/);
  assert.equal(off, 1);
});

test("copy counters never mark a failed backup item as a completed checkpoint", async () => {
  const w = wizard(); w.backupDir = "backup"; w.backupPath = "C:/backups";
  w.groups = [{ id: "files", items: [{ id: "dcim", checked: true }] }];
  const step = w.runSteps[0]; step.id = "backup";
  step.sub = { list: ["사진"], done: 0 };
  api.onBackupProgress = async callback => {
    callback({ itemId: "dcim", phase: "copy", filesDone: 1, filesTotal: 1, bytesDone: 1, bytesTotal: 1 });
    return () => {};
  };
  api.backupRun = async () => ({ ok: true, value: { dir: "backup", complete: false, files: 1, bytes: 1,
    errors: ["IO"], items: [{ id: "dcim", status: "partial", files: 1, bytes: 1 }] } });
  await w.runRealBackup(step);
  assert.equal(step.status, "failed");
  assert.equal(step.sub.done, 0);
  assert.ok(!step.logs.some(line => line.includes("[체크포인트]")));
});

test("stopping during a backup cancels exactly that backup run", async () => {
  const w = wizard(); w.backupDir = "backup"; w.backupPath = "C:/backups";
  w.groups = [{ id: "files", items: [{ id: "dcim", checked: true }] }];
  const step = w.runSteps[0]; step.id = "backup";
  const run = deferred();
  let runId, cancelledId = "none";
  api.onBackupProgress = async () => () => {};
  api.backupRun = (_serial, _items, _dest, id) => { runId = id; return run.promise; };
  api.backupCancel = async id => { cancelledId = id; };
  const pending = w.runRealBackup(step);
  await new Promise(resolve => setTimeout(resolve, 0));
  assert.ok(typeof runId === "string" && runId.length > 0);
  w.abort();
  assert.equal(cancelledId, runId);
  run.resolve({ ok: false, error: "cancelled" });
  await pending;
});

test("SMS preparation blocks confirmation until it succeeds and permits retry", async () => {
  const w = wizard();
  w.manualCurrent = { id: "smsie-export" };
  const first = deferred();
  api.smsiePrepare = () => first.promise;
  const pending = w.smsiePrepare();
  assert.equal(w.manualInputReady, false);
  first.resolve({ ok: false, error: "permission denied" });
  await pending;
  assert.equal(w.manualInputReady, false);
  assert.equal(w.manualCheckError, "permission denied");
  api.smsiePrepare = async () => ({ ok: true, value: [] });
  await w.smsiePrepare();
  assert.equal(w.manualInputReady, true);
});

test("reset invalidates pending IMEI and folder responses", async () => {
  const w = wizard(), imei = deferred(), folder = deferred();
  api.readImei1 = () => imei.promise;
  api.firmwareDirCheck = () => folder.promise;
  const a = w.loadImei(), b = w.setFirmwareDir("folder");
  w.runGen++;
  imei.resolve("123456789012345");
  folder.resolve({ ok: true, value: { file: "boot.img", imageBytes: 1 } });
  await Promise.all([a, b]);
  assert.equal(w.imei1, null);
  assert.equal(w.firmwareDirInfo, null);
});

test("new firmware request wins when downloads finish out of order", async () => {
  const w = wizard(), first = deferred(), second = deferred();
  let count = 0;
  api.firmwareFetch = () => count++ === 0 ? first.promise : second.promise;
  const a = w.fetchFirmware(), b = w.fetchFirmware();
  second.resolve({ ok: true, value: { version: "latest" } }); await b;
  first.resolve({ ok: true, value: { version: "old" } }); await a;
  assert.equal(w.firmware.version, "latest");
});

test("stale event registration cannot start a restore after reset", async () => {
  const w = wizard(), registration = deferred();
  w.backupDir = "backup";
  let started = 0, off = 0;
  api.onRestoreProgress = () => registration.promise;
  api.restoreRun = async () => { started++; };
  const pending = w.runRealRestore(w.runSteps[0]);
  w.runGen++;
  registration.resolve(() => off++);
  await pending;
  assert.equal(started, 0);
  assert.equal(off, 1);
});

test("failed exit save leaves a resumable pending step and invalidates late work", async () => {
  const w = wizard(), registration = deferred();
  w.backupDir = "backup";
  w.journalKey = "a".repeat(64);
  w.runSteps[0].manualDone = 1;
  w.manualCurrent = { id: "smsie-import" };
  let started = 0, off = 0, cancelled = 0;
  api.onRestoreProgress = () => registration.promise;
  api.restoreRun = async () => { started++; };
  api.backupCancel = async () => { cancelled++; };
  api.journalSave = async () => false;
  const pending = w.runRealRestore(w.runSteps[0]);
  assert.equal(await w.closeForExit(), false);
  registration.resolve(() => off++);
  await pending;
  assert.equal(started, 0);
  assert.equal(off, 1);
  assert.equal(cancelled, 1);
  assert.equal(w.runSteps[0].status, "pending");
  assert.equal(w.runSteps[0].manualDone, 0);
  assert.equal(w.manualCurrent, null);
  assert.equal(w.running, false);
  assert.ok(w.stopInfo);
});

test("exit rejects a running destructive step before cancelling or saving", async () => {
  const w = wizard();
  w.runSteps[0].id = "unlock";
  w.steps = [{ id: "unlock", risk: "danger" }];
  let touched = 0;
  api.backupCancel = api.journalSave = async () => { touched++; return true; };
  assert.equal(await w.closeForExit(), false);
  assert.equal(touched, 0);
  assert.equal(w.runSteps[0].status, "running");
});

test("views and domain keep desktop IPC behind the API boundary", () => {
  const root = path.resolve("src");
  const walk = dir => fs.readdirSync(dir, { withFileTypes: true }).flatMap(entry =>
    entry.isDirectory() ? walk(path.join(dir, entry.name)) : [path.join(dir, entry.name)]);
  for (const file of walk(root).filter(file => /\.(ts|svelte)$/.test(file))) {
    const code = fs.readFileSync(file, "utf8");
    if (!file.includes(path.join("lib", "api"))) assert.doesNotMatch(code, /from\s+["']@tauri-apps|import\(["']@tauri-apps/, file);
    if (file.includes(path.join("lib", "domain"))) assert.doesNotMatch(code, /from\s+["'](?:@tauri-apps|\$lib\/(?:api|stores|mock))/, file);
  }
});

test("every concrete IPC command used by the facade is registered in Rust", () => {
  const facade = fs.readFileSync("src/lib/api/index.ts", "utf8");
  const backend = fs.readFileSync("src-tauri/src/lib.rs", "utf8");
  const commands = [...facade.matchAll(/(?:invokeBackend|invokeResult)[^\n]*?\("([a-z_]+)"/g)].map(match => match[1]);
  assert.ok(commands.length >= 20);
  for (const command of commands) assert.match(backend, new RegExp("\\b\\w+::" + command + "\\b"), command);
});

test("run confirmation covers wipes, firmware, boot image writes and EFS edits", () => {
  for (const kind of ["fw-flash", "root", "unroot", "efs"]) {
    assert.ok(stepHazard({ kind, wipe: false }), kind);
  }
  assert.equal(stepHazard({ kind: "unlock", wipe: true }).short, "데이터 초기화");
  for (const kind of ["backup", "setup", "verify", "restore"]) {
    assert.equal(stepHazard({ kind, wipe: false }), null, kind);
  }
});

test("DIAG driver notice shows only when the driver is confirmed missing and ignores stale checks", async () => {
  const w = wizard();
  const first = deferred(), second = deferred();
  const replies = [first.promise, second.promise];
  api.envCheck = () => replies.shift();
  const stale = w.loadEnv();
  const fresh = w.loadEnv();
  second.resolve([{ id: "diag-driver", label: "드라이버", state: "pass", detail: "", fixable: false }]);
  await fresh;
  first.resolve([{ id: "diag-driver", label: "드라이버", state: "warn", detail: "없음", fixable: false }]);
  await stale;
  assert.equal(w.diagDriverMissing, null); // 늦게 온 이전 결과가 덮어쓰지 않는다
  assert.equal(w.envLoading, false);

  api.envCheck = async () => [{ id: "diag-driver", label: "드라이버", state: "info", detail: "확인 불가", fixable: false }];
  await w.loadEnv();
  assert.equal(w.diagDriverMissing, null); // 확인 불가는 없다고 단정하지 않는다
  api.envCheck = async () => [{ id: "diag-driver", label: "드라이버", state: "warn", detail: "없음", fixable: false }];
  await w.loadEnv();
  assert.equal(w.diagDriverMissing?.detail, "없음");
});

test("a failed driver re-check keeps the previous notice instead of hiding it", async () => {
  const w = wizard();
  api.envCheck = async () => [{ id: "diag-driver", label: "드라이버", state: "warn", detail: "없음", fixable: false }];
  await w.loadEnv();
  api.envCheck = async () => null;
  await w.loadEnv();
  assert.equal(w.diagDriverMissing?.detail, "없음");
  assert.equal(w.envLoading, false);
});
