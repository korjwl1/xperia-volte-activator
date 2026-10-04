import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { after, before, beforeEach, test } from "node:test";
import { createServer } from "vite";

let server, Wizard, api, flags, originalApi, originalFlags, createTransport, AsyncQueue, decodeJournal, buildPlan, stepHazard, firmwareUpdateProblems;
before(async () => {
  server = await createServer({ server: { middlewareMode: true, watch: null, hmr: false, ws: false }, appType: "custom" });
  ({ Wizard } = await server.ssrLoadModule("/src/lib/stores/wizard.svelte.ts"));
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
