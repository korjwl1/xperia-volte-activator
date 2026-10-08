import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { after, before, beforeEach, test } from "node:test";
import { createServer } from "vite";

const offFlags = { backup: false, restore: false, fastboot: false, relock: false, root: false, rootTools: false, volteRollback: false, verify: false, efs: false };
let server, Wizard, itemProgress, transferStatusText, api, flags, originalApi, originalFlags, createTransport, AsyncQueue, decodeJournal, buildPlan, stepHazard, firmwareUpdateProblems;
before(async () => {
  server = await createServer({ server: { middlewareMode: true, watch: null, hmr: false, ws: false }, appType: "custom" });
  ({ Wizard, itemProgress, transferStatusText } = await server.ssrLoadModule("/src/lib/stores/wizard.svelte.ts"));
  ({ api } = await server.ssrLoadModule("/src/lib/api/index.ts"));
  ({ REAL_STEPS: flags } = await server.ssrLoadModule("/src/lib/data/runMode.ts"));
  ({ createTransport } = await server.ssrLoadModule("/src/lib/api/transport.ts"));
  ({ AsyncQueue } = await server.ssrLoadModule("/src/lib/domain/asyncQueue.ts"));
  ({ decodeJournal } = await server.ssrLoadModule("/src/lib/domain/journal.ts"));
  ({ buildPlan, stepHazard } = await server.ssrLoadModule("/src/lib/domain/plan.ts"));
  ({ firmwareUpdateProblems } = await server.ssrLoadModule("/src/lib/domain/verify.ts"));
  originalApi = { ...api };
  originalFlags = { ...flags };
});
beforeEach(() => {
  Object.assign(api, originalApi, { journalSave: async () => true, runGuard: async () => true });
  Object.assign(flags, { backup: true, restore: true, fastboot: false });
});
after(async () => {
  Object.assign(api, originalApi);
  Object.assign(flags, offFlags);
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

test("unknown SIM type survives journal reload without being assigned an eSIM slot", () => {
  const valid = journal();
  valid.communication = { before: { checkedAt: "2025-01-02T03:04:05Z", outcome: "observed",
    model: "test-model", firmware: "test-fw", fingerprint: "test-build", android: "14", baseband: "test-modem",
    sims: [{ slot: 1, type: "unknown", carrier: null, state: "ABSENT", volte: "unknown" },
      { slot: 2, type: "physical", carrier: "KT", state: "LOADED", volte: "off" }], presets: [] }, latest: null, calls: [] };
  const saved = decodeJournal(JSON.stringify(valid));
  assert.equal(saved.communication.before.sims[0].type, "unknown");
  assert.equal(saved.communication.before.sims[1].type, "physical");
  valid.communication.before.sims[0].type = "unrecognized";
  assert.equal(decodeJournal(JSON.stringify(valid)), null);
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

test("relock-only restores both slots even when root detection reports an unrooted phone", () => {
  const w = wizard(); w.device.bootloader = "unlocked"; w.device.rooted = false;
  const config = { sims: [{ slot: 1, carrier: null }, { slot: 2, carrier: null }], firmware: null, bootloaderAction: "relock" };
  const steps = buildPlan(w.device, config, { unroot: false, relock: false, restore: false }, false);
  assert.ok(steps.find(s => s.id === "prep").manual.includes("firmware-select"));
  assert.ok(steps.findIndex(s => s.id === "unroot") < steps.findIndex(s => s.id === "relock"));
  assert.equal(steps.find(s => s.id === "relock").manual, undefined);
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

test("backup access warnings preserve the incomplete gate and never mask mixed integrity failures", async () => {
  const w = wizard(); w.backupDir = "backup"; w.backupPath = "C:/backups";
  w.groups = [{ id: "files", items: [{ id: "dcim", checked: true }] }];
  const step = w.runSteps[0]; step.id = "backup";
  api.onBackupProgress = async () => () => {};
  api.backupRun = async () => ({ ok: true, value: { dir: "backup", complete: false, files: 1, bytes: 1,
    errors: ["dcim: private: Permission denied"], items: [{ id: "dcim", status: "partial", files: 1, bytes: 1 }] } });
  await w.runRealBackup(step);
  assert.equal(step.status, "failed");
  assert.equal(w.backupPermissionBlocked, true);
  assert.match(w.stepError, /별도로 내보내/);
  assert.equal(w.backupSummary.complete, false);
  w.backupSummary.errors.push("dcim: photo: 크기/해시 불일치");
  assert.equal(w.backupPermissionBlocked, false);
  w.backupSummary.errors.pop();
  w.stepError = "백업 실패: USB 연결 끊김";
  assert.equal(w.backupPermissionBlocked, false, "previous permission result cannot hide a later transport failure");
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
  // 준비가 끝나면 바로 확인할 수 있다 — 체크박스 확인은 없앴다(파일 검사가 대신한다)
  assert.equal(w.manualInputReady, true);
});

test("stopping SMS preparation blocks restart until the pending device work settles", async () => {
  const w = wizard(), prepare = deferred();
  w.manualCurrent = { id: "smsie-export" };
  api.smsiePrepare = () => prepare.promise;
  const pending = w.smsiePrepare();
  await Promise.resolve();
  assert.equal(w.busy, 1);
  w.abort();
  let starts = 0;
  w.begin = () => starts++;
  w.resumeRun();
  assert.equal(starts, 0);
  assert.equal(w.busy, 1);
  prepare.resolve({ ok: true, value: [] });
  await pending;
  assert.equal(w.busy, 0);
  assert.equal(w.manualCurrent, null);
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

test("adversarial journal read and archive failures never authorize overwriting", async () => {
  const w = wizard();
  w.journalKeyReady = async () => "a".repeat(64);
  w.journalKey = "a".repeat(64);
  let writes = 0;
  api.journalSave = async () => { writes++; return true; };
  api.journalLoad = async () => ({ ok: false, error: "permission denied" });
  assert.equal(await w.checkJournal(), true);
  assert.match(w.journalError, /읽지/);
  assert.equal(await w.persist(true), false);
  assert.equal(writes, 0);
  api.journalLoad = async () => ({ ok: true, value: "{broken" });
  api.journalArchive = async () => false;
  assert.equal(await w.checkJournal(), true);
  assert.equal(await w.persist(true), false);
  api.journalArchive = async () => true;
  assert.equal(await w.checkJournal(), false);
  assert.equal(w.journalError, "");
  assert.equal(await w.persist(true), true);
  assert.equal(writes, 1);
});

test("a late journal archive cannot change a new device session", async () => {
  const w = wizard(), pending = deferred();
  w.journalKey = "a".repeat(64);
  w.view = "warning";
  api.journalArchive = () => pending.promise;
  const discard = w.discardJournal();
  await Promise.resolve();
  w.runGen++;
  w.journalKey = "b".repeat(64);
  w.view = "device";
  w.journalError = "new session";
  pending.resolve(false);
  await discard;
  assert.equal(w.view, "device");
  assert.equal(w.journalError, "new session");
  assert.equal(w.journalReadBlocked, false);
});

test("adversarial save rejection is visible and a successful retry clears the warning", async () => {
  const w = wizard();
  w.journalKey = "a".repeat(64);
  api.journalSave = async () => { throw new Error("disk full"); };
  assert.equal(await w.persist(true), false);
  assert.match(w.journalError, /저장하지/);
  api.journalSave = async () => true;
  assert.equal(await w.persist(true), true);
  assert.equal(w.journalError, "");
});

test("adversarial duplicate USB prompt checks run once and cannot advance twice", async () => {
  const w = wizard(), pending = deferred();
  let checks = 0;
  api.deviceList = () => { checks++; return pending.promise; };
  const first = w.openManual(w.runSteps[0], "usb-debug");
  const second = w.openManual(w.runSteps[0], "usb-debug");
  pending.resolve([w.device]);
  await Promise.all([first, second]);
  assert.equal(checks, 1);
  assert.equal(w.runSteps[0].manualDone, 1);
  assert.equal(w.manualCurrent, null);
});

test("adversarial complete backup from another phone or changed disk never passes wipe gate", async () => {
  const w = wizard();
  w.runSteps = [{ id: "unlock", title: "언락", status: "running", progress: 0, manualDone: 0, logs: [] },
    { id: "backup", status: "done", progress: 1, manualDone: 0, logs: [] }];
  w.backupDir = "backup";
  w.backupSummary = { complete: true };
  const ownKey = await w.deviceKeyHex();
  for (const [complete, deviceKey, accepted] of [[true, "b".repeat(64), false], [false, ownKey, false], [true, ownKey, true]]) {
    w.stepError = ""; w.runSteps[0].status = "running";
    let proceeded = false; w.begin = () => { proceeded = true; };
    api.backupManifestCheck = async () => ({ complete, deviceKey, errors: [] });
    await w.enforceBackupGate(w.runSteps[0]);
    assert.equal(proceeded, accepted);
  }
});

test("valid SMS files are collected and confirmed by file checks, without a user checkbox", async () => {
  const w = wizard();
  w.manualCurrent = { id: "smsie-export" };
  w.manualSetupState = "done";
  let confirmations = [];
  api.smsieCollect = async (_serial, _dir, confirmed) => {
    confirmations.push(confirmed);
    return { ok: true, value: { ready: true, summary: { complete: true } } };
  };
  assert.equal(w.manualInputReady, true);
  assert.equal(await w.verifyManual("smsie-export"), null);
  assert.deepEqual(confirmations, [true]);
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
  for (const kind of ["fw-flash", "root", "unroot", "efs", "restore"]) {
    assert.ok(stepHazard({ kind, wipe: false }), kind);
  }
  assert.equal(stepHazard({ kind: "unlock", wipe: true }).short, "데이터 초기화");
  for (const kind of ["backup", "setup", "verify"]) {
    assert.equal(stepHazard({ kind, wipe: false }), null, kind);
  }
});

test("backup deletion targets the run's backup folder, never the chosen parent folder", async () => {
  const w = wizard();
  w.backupPath = "C:/Users/me/Documents";
  w.backupDir = "C:/Users/me/Documents/backup-20261004-000000-XQ-DQ44";
  const asked = [];
  api.backupDelete = async dir => { asked.push(dir); return { ok: true, value: null }; };
  await w.deleteBackup();
  await w.deleteBackup(); // 삭제 후 중복 호출 없음
  assert.deepEqual(asked, [w.backupDir]);
  assert.equal(w.backupDeleteState, "deleted");

  const missing = wizard();
  missing.backupPath = "C:/Users/me/Documents";
  missing.backupDir = "";
  asked.length = 0;
  await missing.deleteBackup();
  assert.deepEqual(asked, []);
  assert.equal(missing.backupDeleteState, "failed");
});

test("backup deletion can be retried after a failure and ignores results from a previous session", async () => {
  const w = wizard();
  w.backupDir = "C:/b/backup-20261004-000000-XQ-DQ44";
  api.backupDelete = async () => ({ ok: false, error: "locked" });
  await w.deleteBackup();
  assert.equal(w.backupDeleteState, "failed");
  assert.equal(w.backupDeleteError, "locked");
  api.backupDelete = async () => ({ ok: true, value: null });
  await w.deleteBackup();
  assert.equal(w.backupDeleteState, "deleted");

  const late = wizard(), pending = deferred();
  late.backupDir = "C:/b/backup-20261004-000001-XQ-DQ44";
  api.backupDelete = () => pending.promise;
  const run = late.deleteBackup();
  late.restart(); // 삭제 중 [처음으로]
  pending.resolve({ ok: true, value: null });
  await run;
  assert.equal(late.backupDeleteState, "idle");
});

test("deleting the backup needs extra confirmation only when a wipe happened without a real restore", () => {
  const w = wizard();
  w.steps = [
    { id: "unlock", kind: "unlock", wipe: true },
    { id: "restore", kind: "restore", wipe: false },
  ];
  w.runSteps = [
    { id: "unlock", status: "done", logs: [] },
    { id: "restore", status: "failed", logs: [] },
  ];
  assert.equal(w.backupStillNeeded, true);
  w.runSteps[1].status = "done";
  flags.restore = true;
  assert.equal(w.backupStillNeeded, false);
  // 백업은 실전인데 복구가 시뮬레이션이면 실제로 복원된 것이 아니다
  flags.restore = false;
  assert.equal(w.backupStillNeeded, true);
  // 초기화가 없었으면 해당 없음
  w.runSteps[0].status = "pending";
  assert.equal(w.backupStillNeeded, false);
});

test("mock mode pre-fills the unlock code only when fastboot is simulated and the field is empty", () => {
  const w = wizard();
  w.onManualOpen("unlock-code");
  assert.equal(w.unlockCode, "0x1234567890ABCDEF");
  w.unlockCode = "0xABCDEF0123456789";
  w.onManualOpen("unlock-code");
  assert.equal(w.unlockCode, "0xABCDEF0123456789"); // 입력한 값은 덮어쓰지 않는다
  flags.fastboot = true;
  try {
    const real = wizard();
    real.onManualOpen("unlock-code");
    assert.equal(real.unlockCode, "");
  } finally {
    flags.fastboot = false;
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

const FP = (v, region = "XQ-DQ44") => `Sony/${region}/${region}:15/${v}/1234:user/release-keys`;

test("firmware update check requires the target version, its fingerprint and the same device", () => {
  const target = "67.2.A.3.200";
  const before = FP("67.2.A.3.178");
  assert.deepEqual(firmwareUpdateProblems(target, { firmware: target, fingerprint: FP(target) }, { before }), []);
  assert.equal(firmwareUpdateProblems(target, { firmware: "67.2.A.3.178", fingerprint: before }, { before }).length, 2);
  // 다른 지역 펌웨어
  assert.equal(firmwareUpdateProblems(target, { firmware: target, fingerprint: FP(target, "XQ-DQ72") }, { before }).length, 1);
  // 지문을 못 읽으면 통과시키지 않는다
  assert.equal(firmwareUpdateProblems(target, { firmware: target }, { before }).length, 1);
  // 받아 둔 대상 펌웨어 지문이 있으면 정확히 같아야 한다
  assert.deepEqual(firmwareUpdateProblems(target, { firmware: target, fingerprint: FP(target) }, { expected: FP(target) }), []);
  assert.equal(firmwareUpdateProblems(target, { firmware: target, fingerprint: FP(target) + "x" }, { expected: FP(target) }).length, 1);
});

/** 실전 확인 단계용 위자드 — openManual은 기록만, 끝나면 cleanup으로 남은 대기를 취소 */
function verifyWizard(id) {
  const w = wizard();
  w.steps = [{ id, kind: "final-verify", title: id, desc: "", risk: "safe", wipe: false, optional: false, enabled: true, estSec: 1,
    manual: id === "final-verify" ? ["ims-check"] : undefined }];
  w.runSteps = [{ id, title: id, status: "running", progress: 0, logs: [], manualDone: 0 }];
  w.cursor = 0;
  w.opened = [];
  w.openManual = async (_cur, manualId) => { w.opened.push(manualId); };
  return w;
}
const cancelWaits = w => { w.runGen++; };
/** 재부팅 요청 뒤 첫 조회 한 번만 끊긴 것으로 보이는 가짜 기기 목록(타이머 없음) */
function rebootingPhone(w, calls) {
  let offlineReads = 0;
  api.rootReboot = async (_s, target) => { calls.push(`reboot:${target}`); offlineReads = 1; return { ok: true, value: null }; };
  api.deviceList = async () => (offlineReads-- > 0 ? [] : [w.device]);
}

test("real final check reboots first, then opens the VoLTE registration prompt", async () => {
  flags.verify = true;
  const w = verifyWizard("final-verify");
  try {
    const calls = [];
    rebootingPhone(w, calls);
    await w.runRealFinalVerify(w.runSteps[0]);
    assert.deepEqual(calls, ["reboot:os"]);
    assert.deepEqual(w.opened, ["ims-check"]);
    assert.equal(w.runSteps[0].status, "manual-wait");
    w.finishRealFinalVerify(w.runSteps[0]);
    assert.equal(w.runSteps[0].status, "done");
  } finally {
    cancelWaits(w);
    flags.verify = false;
  }
});

test("resuming the final check after the reboot checkpoint does not reboot again", async () => {
  flags.verify = true;
  const w = verifyWizard("final-verify");
  try {
    const calls = [];
    rebootingPhone(w, calls);
    w.runSteps[0].sub = { list: ["재부팅", "네트워크 등록", "VoLTE 활성 확인"], done: 1 };
    await w.runRealFinalVerify(w.runSteps[0]);
    assert.deepEqual(calls, []);
    assert.deepEqual(w.opened, ["ims-check"]);
  } finally {
    cancelWaits(w);
    flags.verify = false;
  }
});

test("finishing the final check without registration is logged as unverified", () => {
  const w = verifyWizard("final-verify");
  w.runSteps[0].sub = { list: ["재부팅", "네트워크 등록", "VoLTE 활성 확인"], done: 1 };
  w.imsUnverified = true;
  w.finishRealFinalVerify(w.runSteps[0]);
  assert.equal(w.runSteps[0].sub.done, 1);
  assert.ok(w.runSteps[0].logs.some(line => line.startsWith("[미확인]")));
  assert.equal(w.runSteps[0].status, "done");
});

test("a failed reboot request fails the final check with the backend reason", async () => {
  flags.verify = true;
  const w = verifyWizard("final-verify");
  try {
    api.rootReboot = async () => ({ ok: false, error: "disabled" });
    await w.runRealFinalVerify(w.runSteps[0]);
    assert.equal(w.runSteps[0].status, "failed");
    assert.match(w.stepError, /disabled/);
  } finally {
    cancelWaits(w);
    flags.verify = false;
  }
});

test("tick runs the real final check engine before the plan's registration prompt", async () => {
  flags.verify = true;
  const w = verifyWizard("final-verify");
  try {
    w.runSteps[0].status = "pending";
    const calls = [];
    rebootingPhone(w, calls);
    w.tick();
    for (let i = 0; i < 200 && w.opened.length === 0; i++) await new Promise(resolve => setTimeout(resolve, 5));
    assert.deepEqual(calls, ["reboot:os"]);
    assert.deepEqual(w.opened, ["ims-check"]);
  } finally {
    cancelWaits(w);
    flags.verify = false;
  }
});

test("real firmware check fails fast while flashing is simulated, and compares version and fingerprint", async () => {
  flags.verify = true;
  const target = "67.2.A.3.200";
  try {
    const simulated = verifyWizard("fw-verify");
    simulated.volteConfig = { ...simulated.volteConfig, firmware: target };
    await simulated.runRealFwVerify(simulated.runSteps[0]);
    assert.equal(simulated.runSteps[0].status, "failed");
    assert.match(simulated.stepError, /펌웨어 기록이 아직/);

    const mismatch = verifyWizard("fw-verify");
    mismatch.hasRealEngine = () => true;
    mismatch.volteConfig = { ...mismatch.volteConfig, firmware: target };
    mismatch.device = { ...mismatch.device, fingerprint: FP("67.2.A.3.178") };
    api.deviceList = async () => [{ ...mismatch.device, firmware: "67.2.A.3.178", fingerprint: FP("67.2.A.3.178") }];
    await mismatch.runRealFwVerify(mismatch.runSteps[0]);
    assert.equal(mismatch.runSteps[0].status, "failed");
    assert.match(mismatch.stepError, /설치된 버전/);

    const ok = verifyWizard("fw-verify");
    ok.hasRealEngine = () => true;
    ok.volteConfig = { ...ok.volteConfig, firmware: target };
    ok.firmware = { version: target, fingerprint: FP(target) };
    api.deviceList = async () => [{ ...ok.device, firmware: target, fingerprint: FP(target) }];
    await ok.runRealFwVerify(ok.runSteps[0]);
    assert.equal(ok.runSteps[0].status, "done");
  } finally {
    flags.verify = false;
  }
});

test("the mock skip button is unavailable while real checks are on", () => {
  const w = verifyWizard("final-verify");
  w.manualCurrent = { id: "ims-check" };
  const skippable = w.manualSkippable;
  flags.verify = true;
  try {
    assert.equal(w.manualSkippable, false);
  } finally {
    flags.verify = false;
  }
  assert.equal(w.manualSkippable, skippable);
});


test("SMS confirmation cannot dismiss its guide and backup-only live mode cannot skip it", () => {
  const w = wizard(); Object.assign(flags, { backup: true, restore: false, root: false, fastboot: false, efs: false, verify: false });
  w.manualCurrent = { id: "smsie-export" };
  w.manualChecking = true; assert.equal(w.manualCanDismiss, false); assert.equal(w.manualSkippable, false);
  w.manualChecking = false; assert.equal(w.manualCanDismiss, true);
  Object.assign(flags, { backup: false }); assert.equal(w.manualSkippable, true);
});
test("a mismatched journal can be archived explicitly without overwriting it", async () => {
  const w = wizard(); const mismatched = journal(); mismatched.model = "XQ-OTHER";
  api.journalLoad = async () => ({ ok: true, value: JSON.stringify(mismatched) });
  const archived = []; api.journalArchive = async (key, tag) => { archived.push([key, tag]); return true; };
  assert.equal(await w.checkJournal(), true); assert.equal(w.journalMismatch, true); assert.equal(w.journalReadBlocked, true);
  await w.discardJournal(); assert.equal(archived.length, 1); assert.equal(archived[0][1], "discarded"); assert.equal(w.journalReadBlocked, false); assert.equal(w.journalMismatch, false);
});
test("resume waits for firmware folder validation before any build or device checks", async () => {
  const w = wizard(); w.firmwareDirState = "loading"; let calls = 0;
  api.engineCapabilities = async () => { calls++; throw Error("must not query"); }; w.begin = () => calls++;
  w.resumeRun(); await new Promise(r => setImmediate(r)); assert.equal(calls, 0);
});


test("contact import finishes cleanup after counts pass and never writes after a stale check", async () => {
  const w = wizard(); w.backupDir = "backup"; w.manualCurrent = { id: "contacts-import" };
  let cleanups = 0; api.contactsRestoreCheck = async () => ({ backedUp: 0, onDevice: 0 });
  api.contactsRestoreFinish = async (serial, dir) => { assert.equal(serial, w.device.serial); assert.equal(dir, "backup"); cleanups++; return { ok: true, value: null }; };
  assert.equal(await w.verifyManual("contacts-import"), null); assert.equal(cleanups, 1);
  api.contactsRestoreFinish = async () => ({ ok: false, error: "cleanup failed" }); assert.match(await w.verifyManual("contacts-import"), /cleanup failed/);
  const pending = deferred(); api.contactsRestoreCheck = () => pending.promise;
  const stale = w.verifyManual("contacts-import"); w.runGen++; pending.resolve({ backedUp: 0, onDevice: 0 }); await stale; assert.equal(cleanups, 1);
});

test("mock steps wait for Next and internal callbacks cannot advance the next step", async () => {
  Object.assign(flags, Object.fromEntries(Object.keys(flags).map(key => [key, false])));
  const w = wizard();
  w.begin = Wizard.prototype.begin.bind(w);
  w.runSteps = ["backup", "restore"].map(id => ({ id, title: id, status: "pending", progress: 0, logs: [], manualDone: 0 }));
  w.runSteps[0].status = "running"; w.runSteps[0].progress = 1;
  try {
    w.tick();
    assert.equal(w.awaitingNext, "backup");
    assert.equal(w.running, false);
    w.begin(); w.resumeRun(); w.tick();
    assert.equal(w.runSteps[1].status, "pending");
    assert.equal(w.running, false);
    w.nextStep(); w.nextStep();
    assert.equal(w.running, true);
    w.pause(); w.tick();
    assert.equal(w.runSteps[1].status, "running");
    assert.equal(w.cursor, 1);
    w.runSteps[1].progress = 1; w.tick();
    assert.equal(w.finished, true);
    assert.equal(w.awaitingNext, null);
  } finally { w.pause(); }
});

test("live backup exclusion notice needs acknowledgement and Next before a second engine starts", async () => {
  const w = wizard(); w.backupDir = "backup"; w.backupPath = "C:/backups";
  w.groups = [{ id: "files", items: [{ id: "app-data", checked: true }] }];
  w.runSteps = ["backup", "restore"].map(id => ({ id, title: id, status: "pending", progress: 0, logs: [], manualDone: 0 }));
  w.runSteps[0].status = "running";
  w.begin = Wizard.prototype.begin.bind(w);
  api.onBackupProgress = async () => () => {};
  api.backupRun = async () => ({ ok: true, value: { dir: "backup", complete: true, files: 1, bytes: 1, errors: [],
    items: [{ id: "app-data", status: "done", files: 1, bytes: 1 }],
    omittedApps: [{ package: "org.example.blocked", reasons: ["Permission denied"], removedFiles: 2, removedBytes: 3, cleanupPending: false }] } });
  let restores = 0, checks = 0;
  const capability = deferred();
  api.engineCapabilities = () => { checks++; return capability.promise; };
  api.onRestoreProgress = async () => () => {};
  api.restoreRun = async () => { restores++; return { ok: true, value: { logs: [], failures: [], smsiePending: false } }; };
  try {
    await w.runRealBackup(w.runSteps[0]);
    assert.equal(w.awaitingNext, "backup"); assert.equal(w.backupOmissionNotice, true);
    w.begin(); w.resumeRun(); w.nextStep(); w.tick();
    assert.equal(restores, 0); assert.equal(checks, 0);
    w.acknowledgeBackupOmissions(); w.tick();
    assert.equal(restores, 0); assert.equal(w.running, false);
    w.nextStep(); w.nextStep();
    assert.equal(checks, 1);
    assert.equal(restores, 0);
    capability.resolve({ ok: true, value: { fastbootWrite: false, rootWrite: false, efsWrite: false } });
    await new Promise(r => setImmediate(r));
    assert.equal(w.running, true);
    w.pause(); w.tick(); await new Promise(r => setImmediate(r));
    assert.equal(restores, 1);
    assert.equal(w.finished, true);
  } finally { w.pause(); }
});

test("journal resume retains Next and an unacknowledged omission notice without starting work", () => {
  const j = journal();
  const second = { ...j.steps[0], id: "restore", kind: "restore", title: "복원" };
  j.steps.push(second);
  Object.assign(j.runSteps[0], { status: "done", progress: 1 });
  j.runSteps.push({ id: "restore", title: "복원", status: "pending", progress: 0, logs: [], manualDone: 0 });
  j.cursor = 1; j.awaitingNext = "backup";
  j.backupOmissions = { pending: true, apps: [{ package: "org.example.blocked", reasons: ["Permission denied"], removedFiles: 2, removedBytes: 3, cleanupPending: false }] };
  const decoded = decodeJournal(JSON.stringify(j)); assert.ok(decoded);
  const w = wizard(); w.omdAck=true; w.riskAck=true; w.pendingJournal = decoded; w.resumeJournal();
  assert.equal(w.awaitingNext, "backup"); assert.equal(w.backupOmissionNotice, true);
  assert.equal(w.backupOmittedApps[0].package, "org.example.blocked");
  assert.equal(w.running, false);
  w.acknowledgeBackupOmissions(); assert.equal(w.awaitingNext, "backup");
  for (const mutate of [j => { j.awaitingNext = "restore"; }, j => { j.backupOmissions.apps[0].removedBytes = -1; }, j => { j.backupOmissions.pending = "true"; }]) {
    const bad = structuredClone(j); mutate(bad); assert.equal(decodeJournal(JSON.stringify(bad)), null);
  }
});

test("exclusions cannot hide a remaining backup error or claim unfinished deletion succeeded", async () => {
  for (const pending of [false, true]) {
    const w = wizard(); w.backupDir = "backup"; w.backupPath = "C:/backups";
    w.groups = [{ id: "files", items: [{ id: "app-data", checked: true }] }];
    const step = w.runSteps[0]; step.id = "backup";
    api.onBackupProgress = async () => () => {};
    api.backupRun = async () => ({ ok: true, value: { dir: "backup", complete: false, files: 1, bytes: 1,
      errors: [pending ? "앱 데이터 정리 미완료" : "dcim: 크기/해시 불일치"], items: [],
      omittedApps: [{ package: "org.example.blocked", reasons: ["Permission denied"], removedFiles: 2, removedBytes: 3, cleanupPending: pending }] } });
    await w.runRealBackup(step);
    assert.equal(step.status, "failed"); assert.equal(w.awaitingNext, null);
    assert.equal(w.backupOmissionNotice, !pending);
    w.nextStep(); assert.equal(w.finished, false);
  }
});


test("explicit standalone backup has one live step and never enables write engines", async () => {
  Object.assign(flags,Object.fromEntries(Object.keys(flags).map(key=>[key,false])));
  api.journalLoad=async()=>({ok:true,value:null});
  const w=wizard();await w.startBackupSession();w.ensureOptions(false);
  assert.equal(w.view,"step2");assert.equal(w.hasAnyTask,true);
  assert.deepEqual(w.plan.map(s=>s.id),["backup"]);
  assert.deepEqual(w.executionFlags,{backup:true,restore:false,fastboot:false,relock:false,root:false,rootTools:false,volteRollback:false,verify:false,efs:false});
  assert.equal(w.opts.restore,false);assert.equal(w.backupLive,true);
  w.startSession();assert.equal(w.opts.backupOnly,false);
});

test("backup-only journals reject every extra engine even when disabled", () => {
  const j=journal();j.opts={backupOnly:true,restore:false,unroot:false,relock:false};
  assert.ok(decodeJournal(JSON.stringify(j)));
  j.steps.push({...j.steps[0],id:"unlock",kind:"unlock",enabled:false});
  assert.equal(decodeJournal(JSON.stringify(j)),null);
});

test("SMS confirmation waits for its read-only probe and preserves actual backend errors", async () => {
  const w=wizard(),poll=deferred();w.manualCurrent={id:"smsie-export"};w.smsieExportAck=true;w.manualSetupState="done";
  let collections=0;api.smsieCollect=async()=>{collections++;return {ok:false,error:"USB disconnected"};};
  w.watchManual(w.runSteps[0],"smsie-export","probe",()=>poll.promise,5000,false);
  const confirming=w.confirmManual();await new Promise(resolve=>setTimeout(resolve,0));
  assert.equal(collections,0);assert.equal(w.manualChecking,true);
  poll.resolve(true);await confirming;w.stopWatch();
  assert.equal(collections,1);assert.match(w.manualCheckError,/USB disconnected/);
  assert.equal(w.manualCurrent.id,"smsie-export");
});

test("successful SMS polling clears its own error and preserves a failed collection error", () => {
  const w=wizard();
  w.reportSmsieProbe({ok:false,error:"probe offline"});
  assert.equal(w.manualCheckError,"probe offline");
  w.reportSmsieProbe({ok:true,value:true});
  assert.equal(w.manualCheckError,"");
  w.manualCheckError="collection failed: PC disk full";
  w.reportSmsieProbe({ok:false,error:"probe offline"});
  w.reportSmsieProbe({ok:true,value:true});
  assert.equal(w.manualCheckError,"collection failed: PC disk full");
  // 같은 잠금 오류 문구라도 수집이 쓴 사유는 감지 성공으로 지워지지 않는다
  w.manualCheckError="";
  w.reportSmsieProbe({ok:false,error:"다른 기기 변경 작업이 진행 중입니다"});
  w.manualCheckError="다른 기기 변경 작업이 진행 중입니다";
  w.reportSmsieProbe({ok:true,value:true});
  assert.equal(w.manualCheckError,"다른 기기 변경 작업이 진행 중입니다");
});

test("other items' errors do not cascade into SMS: export opens first, and completed SMS is reused on retry", async () => {
  const w=wizard();w.backupDir="backup";w.backupPath="C:/backups";
  w.groups=[{id:"files",items:[{id:"dcim",checked:true},{id:"sms",checked:true}]}];
  const step=w.runSteps[0];step.id="backup";let prompts=0;
  w.openEngineManual=()=>prompts++;api.onBackupProgress=async()=>()=>{};
  api.backupRun=async()=>({ok:true,value:{dir:"backup",complete:false,files:1,bytes:1,errors:["dcim: IO"],items:[{id:"dcim",status:"partial"},{id:"sms",status:"pending"}]}});
  // 시도도 안 한 문자를 "미완료"로 함께 실패시키지 않는다 — 문자부터 마치고 남은 오류는 마지막에 알린다
  await w.runRealBackup(step);assert.equal(prompts,1);assert.notEqual(step.status,"failed");
  w.stepError="";step.status="running";
  api.backupRun=async()=>({ok:true,value:{dir:"backup",complete:true,files:2,bytes:2,errors:[],items:[{id:"dcim",status:"done"},{id:"sms",status:"done"}]}});
  // 문자가 이미 완료면 다시 내보내기를 열지 않는다(호출 수 그대로)
  await w.runRealBackup(step);assert.equal(prompts,1);assert.equal(step.status,"done");
});


test("cancelling while an SMS probe is pending prevents subsequent collection", async () => {
  const w=wizard(),poll=deferred();w.manualCurrent={id:"smsie-export"};w.smsieExportAck=true;w.manualSetupState="done";
  let collections=0;api.smsieCollect=async()=>{collections++;return {ok:true,value:{ready:true,summary:{complete:true}}};};
  w.watchManual(w.runSteps[0],"smsie-export","probe",()=>poll.promise,5000,false);
  const confirming=w.confirmManual();w.runGen++;w.stopWatch();poll.resolve(true);await confirming;
  assert.equal(collections,0);
});


test("standalone entry cannot resume other engines without reviewing warnings", () => {
 const w=wizard(),j=journal();w.opts.backupOnly=true;w.pendingJournal=j;
 w.resumeJournal();assert.equal(w.view,"warning");assert.equal(w.running,false);
 assert.ok(w.journalError);assert.equal(w.pendingJournal,null);
});

test("zero-step launch and unresolved prior journal cannot start or save a run", () => {
 const w=wizard();w.opts.backupOnly=true;w.ensureOptions(false);
 for(const g of w.groups)for(const i of g.items)i.checked=false;
 w.launch();assert.notEqual(w.view,"step3");
 const pending=wizard();pending.opts.backupOnly=true;pending.ensureOptions(false);
 assert.ok(pending.plan.some(step=>step.enabled));
 pending.pendingJournal=journal();pending.launch();assert.notEqual(pending.view,"step3");
});


test("generated standalone plan survives journal round-trip with its real manual guide", async () => {
 const w=wizard();await w.startBackupSession();w.ensureOptions(false);
 const j=journal();j.opts={...w.opts};j.config=structuredClone(w.volteConfig);j.steps=w.plan;
 j.backupItems=w.checkedBackupItems();j.runSteps=w.plan.map(s=>({id:s.id,title:s.title,status:"pending",progress:0,logs:[],manualDone:0}));
 assert.ok(j.steps[0].manual.includes("backup-notice"));
 assert.ok(decodeJournal(JSON.stringify(j)));
});

test("a native gate error fails the step and cannot authorize resuming past verification",async()=>{
 const w=wizard();w.runSteps.unshift({id:"backup",title:"백업",status:"done",progress:1,logs:[],manualDone:0});w.cursor=1;
 const step=w.runSteps[1];w.backupDir="backup-fixture";
 api.onBackupProgress=async()=>()=>{};api.backupManifestCheck=async()=>{throw new Error("disk denied");};
 await w.enforceBackupGate(step);
 assert.equal(step.status,"failed");assert.match(w.stepError,/disk denied/);assert.equal(w.running,false);
});

test("backup percentage never goes back across scan, metadata, copy and resume verification", () => {
  const order=["apk","app-data"];
  const ev=(itemId,phase,filesDone,filesTotal,bytesDone=0,bytesTotal=0)=>({itemId,phase,filesDone,filesTotal,bytesDone,bytesTotal});
  const run=[ev("apk","scan",0,10),ev("apk","metadata",10,10),ev("apk","copy",0,10,0,100),ev("apk","copy",10,10,100,100),
    ev("app-data","scan",0,5),ev("app-data","metadata",5,5),ev("app-data","copy",0,5,0,50)];
  let last=0;for(const p of run){const v=itemProgress(p,order);assert.ok(v>=last,`${p.itemId}/${p.phase}: ${v} < ${last}`);last=v;}
  // 재개: 모든 항목 PC 검사(앞 10%) 뒤에 복사가 0%로 떨어지지 않는다. 격리 tar 검사는 계산 제외(NaN)
  const resume=[ev("apk","verify",10,10,100,100),ev("app-data","verify",5,5,50,50),ev("apk","scan",0,1),ev("apk","copy",1,10,10,100)];
  last=0;for(const p of resume){const v=itemProgress(p,order,"resume");assert.ok(v>=last);last=v;}
  assert.ok(Number.isNaN(itemProgress(ev("quarantine","verify",1,2,1,2),order,"resume")));
});

test("current transfer shows item name with done / total files", () => {
  assert.match(transferStatusText({itemId:"apk",phase:"copy",filesDone:12,filesTotal:92,bytesDone:1,bytesTotal:9,file:"base.apk"}),/\[12 \/ 92\]$/);
  assert.match(transferStatusText({itemId:"app-data",phase:"verify",filesDone:3,filesTotal:1000,bytesDone:0,bytesTotal:0}),/PC 검사 \[3 \/ 1,000\]$/);
  assert.equal(transferStatusText({itemId:"apk",phase:"done",filesDone:92,filesTotal:92,bytesDone:9,bytesTotal:9}),"");
});

test("continue after a failed backup finishes the step and the wipe gate honors that decision only for the same phone", async () => {
  const w=wizard();
  w.runSteps=[{id:"backup",title:"백업",status:"failed",progress:0.4,logs:[],manualDone:0},{id:"unlock",title:"언락",status:"pending",progress:0,logs:[],manualDone:0}];
  w.cursor=0;w.backupDir="backup-fixture";w.stepError="백업 미완결 — dcim: IO";
  w.backupSummary={complete:false,errors:["dcim: IO"],items:[]};
  assert.ok(w.canContinueAfterFailure);assert.ok(w.wipeAhead);
  w.continueAfterFailure();
  assert.equal(w.runSteps[0].status,"done");assert.equal(w.stepError,"");assert.ok(w.backupIncompleteAccepted);
  const unlock=w.runSteps[1];w.cursor=1;
  api.onBackupProgress=async()=>()=>{};
  api.backupManifestCheck=async()=>({complete:false,errors:["dcim: IO"],deviceKey:"AA",items:[]});
  w.deviceKeyHex=async()=>"aa";let resumed=0;w.begin=()=>resumed++;
  await w.enforceBackupGate(unlock);assert.equal(resumed,1);assert.notEqual(unlock.status,"failed");
  // 다른 폰이면 인정했어도 통과하지 않는다
  w.deviceKeyHex=async()=>"bb";unlock.status="running";
  await w.enforceBackupGate(unlock);assert.equal(unlock.status,"failed");
});

test("SMS export advances by itself once the files are seen twice and pass collection — no checkbox", async () => {
  const w=wizard();w.opts.backupOnly=true;w.backupDir="backup";
  w.groups=[{id:"files",items:[{id:"sms",checked:true}]}];
  const step=w.runSteps[0];step.id="backup";step.status="manual-wait";
  let collects=0;const confirms=[];
  api.smsiePrepare=async()=>({ok:true,value:["준비"]});
  api.smsieProbe=async()=>({ok:true,value:true});
  api.smsieCollect=async(_s,_d,confirm)=>{collects++;confirms.push(confirm);return {ok:true,value:{ready:true,summary:{complete:true,errors:[],items:[]}}};};
  let tick;const original=globalThis.setInterval;globalThis.setInterval=(fn)=>{tick=fn;return 1;};
  try {
    await w.openManual(step,"smsie-export");
    for (let i=0;i<5 && w.manualSetupState!=="done";i++) await new Promise(r=>setTimeout(r,0));
    assert.equal(w.manualSetupState,"done");
    await tick();assert.equal(collects,0,"one sighting is not enough — the app may still be writing");
    await tick();assert.equal(collects,1);assert.deepEqual(confirms,[true]);
    assert.equal(w.manualCurrent,null,"advanced without any checkbox or button");
  } finally { globalThis.setInterval=original; }
});

test("a found xva-<model>-backup folder is updated in place, while a journal folder is resumed", async () => {
  const w=wizard();w.opts.backupOnly=true;w.backupPath="D:/backups";
  w.groups=[{id:"files",items:[{id:"dcim",checked:true}]}];
  const step=w.runSteps[0];step.id="backup";const runs=[];
  api.onBackupProgress=async()=>()=>{};api.journalSave=async()=>({ok:true,value:null});
  api.backupPrepare=async()=>({ok:true,value:{dir:"D:/backups/xva-XQ-DQ44-backup",existing:true}});
  api.backupRun=async(_s,_i,_d,_r,resumeDir)=>{runs.push(resumeDir);return {ok:true,value:{dir:"D:/backups/xva-XQ-DQ44-backup",complete:true,files:1,bytes:1,errors:[],items:[{id:"dcim",status:"done"}]}};};
  await w.runRealBackup(step);
  assert.equal(w.backupDir,"D:/backups/xva-XQ-DQ44-backup");
  assert.deepEqual(runs,[undefined],"the engine re-finds the folder and refreshes completed items");
  assert.ok(step.logs.some(l=>l.includes("기존 백업을 찾았습니다")));
  // 진행 기록에서 이어 받는 끊긴 실행은 같은 폴더를 재개 경로로 넘긴다(완료 항목은 건너뜀)
  const resumed=wizard();resumed.opts.backupOnly=true;resumed.backupPath="D:/backups";resumed.backupDir="D:/backups/xva-XQ-DQ44-backup";
  resumed.groups=w.groups;const step2=resumed.runSteps[0];step2.id="backup";
  await resumed.runRealBackup(step2);assert.deepEqual(runs,[undefined,"D:/backups/xva-XQ-DQ44-backup"]);
});

test("bootloader without a Windows fastboot driver installs the Sony driver once and keeps waiting", async () => {
  const w=wizard();w.device.productName="Xperia 1 V";w.runSteps=[{id:"unlock",title:"언락",status:"manual-wait",progress:0,logs:[],manualDone:0}];w.cursor=0;
  const asked=[];api.usbModes=async()=>[{mode:"fastboot-nodriver",vendorId:0x0FCE,productId:0x0DDE}];
  api.fastbootDriverEnsure=async(name)=>{asked.push(name);return {ok:true,value:"Sony 공식 드라이버(xperia-1-v-driver)를 fastboot 장치에 연결했습니다"};};
  assert.equal(await w.usbModeIs("fastboot"),false,"keeps waiting until libusb sees the fastboot interface");
  assert.equal(await w.usbModeIs("fastboot"),false);
  assert.deepEqual(asked,["Xperia 1 V"],"UAC is requested only once per run");
  assert.ok(w.runSteps[0].logs.some(l=>l.includes("xperia-1-v-driver")));
  api.usbModes=async()=>[{mode:"fastboot",vendorId:0x0FCE,productId:0x0DDE}];
  assert.equal(await w.usbModeIs("fastboot"),true);
});
