import assert from "node:assert/strict";
import { after, before, beforeEach, test } from "node:test";
import { createServer } from "vite";

let server, Wizard, api, flags, originalApi, originalFlags, waitUntil, calls;
const sourceHash = "a".repeat(64), patchHash = "b".repeat(64), apkHash = "c".repeat(64);
const deferred = () => { let resolve; const promise = new Promise(r => { resolve = r; }); return { promise, resolve }; };
before(async () => {
  server = await createServer({ server: { middlewareMode: true, watch: null, hmr: false, ws: false }, appType: "custom" });
  ({ Wizard } = await server.ssrLoadModule("/src/lib/stores/wizard.svelte.ts"));
  ({ api } = await server.ssrLoadModule("/src/lib/api/index.ts"));
  ({ REAL_STEPS: flags } = await server.ssrLoadModule("/src/lib/data/runMode.ts"));
  ({ waitUntil } = await server.ssrLoadModule("/src/lib/domain/waitUntil.ts"));
  originalApi = { ...api }; originalFlags = { ...flags };
});
after(async () => { Object.assign(api, originalApi); Object.assign(flags, originalFlags); await server?.close(); });
beforeEach(() => {
  calls = { checked: 0, patch: [], flash: [], install: 0, reboot: 0, unsubscribed: 0 };
  Object.assign(flags, { backup: false, restore: false, root: true, fastboot: true });
  Object.assign(api, originalApi, {
    bootImageCheck: async () => { calls.checked++; return { ok: true, value: sourceHash }; },
    magiskPrepare: async () => ({ ok: true, value: { version: "v30.7", apkPath: "Magisk.apk", sha256: apkHash } }),
    onMagiskLog: async () => () => calls.unsubscribed++,
    onFastbootLog: async () => () => calls.unsubscribed++,
    magiskPatch: async (...args) => { calls.patch.push(args); return { ok: true, value: { path: "patched.img", patchedSha256: patchHash, origSha256: sourceHash, bytes: 4096, log: [] } }; },
    fastbootFlash: async (...args) => { calls.flash.push(args); return { ok: true, value: null }; },
    rootReboot: async () => ({ ok: true, value: null }),
    fastbootReboot: async () => { calls.reboot++; return true; },
    magiskInstall: async () => { calls.install++; return { ok: true, value: null }; },
    rootCheck: async () => true,
    backupCancel: async () => true,
  });
});
function wizard(id = "root") {
  const w = new Wizard();
  w.device = { model: "XQ-DQ44", productName: "Xperia", serial: "A", serialMasked: "A****", firmware: "current", bootloader: "unlocked", root: false, state: "device", prep: { developerOptions: true, usbDebugging: true, oemUnlockAllowed: true }, sims: [] };
  w.firmware = { path: "stock.img", fingerprint: "Sony/current", partition: "init_boot", version: "current", imageBytes: 4096, downloadedBytes: 4096 };
  w.steps = [{ id, enabled: true, manual: [] }];
  w.runSteps = [{ id, title: id, status: "running", progress: 0, logs: [], manualDone: 0 }];
  w.persist = async () => true; w.setGuard = () => {}; w.begin = () => {}; w.waitFor = async () => true;
  w.openManual = async (cur, id) => { w.manualCurrent = { id }; };
  return w;
}

test("root passes source, APK, result hashes and the selected device through the full pipeline", async () => {
  const w = wizard(); await w.runRealRoot(w.runSteps[0]);
  assert.equal(w.runSteps[0].status, "manual-wait");
  assert.equal(w.manualCurrent.id, "su-grant");
  assert.deepEqual(calls.patch[0], [{ serial: "A", apkPath: "Magisk.apk", imagePath: "stock.img", partition: "init_boot", imageSha256: sourceHash, fingerprint: "Sony/current", apkSha256: apkHash }]);
  assert.deepEqual(calls.flash[0], ["init_boot", "patched.img", true, "A", patchHash]);
  assert.equal(calls.install, 1); assert.equal(calls.unsubscribed, 2);
  await w.finishRealRoot(w.runSteps[0]); assert.equal(w.runSteps[0].status, "done");
});
test("manual firmware sends the extracted IMG rather than the original SIN", async () => {
  const w = wizard("unroot"); w.firmwareDir = "manual"; api.rootCheck = async () => false;
  w.firmwareDirInfo = { file: "init_boot.sin", path: "extracted.img", fingerprint: "Sony/current", imageBytes: 4096 };
  await w.runRealUnroot(w.runSteps[0]);
  assert.deepEqual(calls.flash[0], ["init_boot", "extracted.img", true, "A", sourceHash]);
  assert.equal(w.runSteps[0].status, "done");
});
test("unroot cannot complete when root access remains after reboot", async () => {
  const w = wizard("unroot"); await w.runRealUnroot(w.runSteps[0]);
  assert.equal(w.runSteps[0].status, "failed"); assert.equal(calls.flash.length, 1);
});
test("mixed real flags fail root and unroot before any checks or writes", () => {
  for (const id of ["root", "unroot"]) for (const values of [{ root: false, fastboot: true }, { root: true, fastboot: false }]) {
    Object.assign(flags, values); const w = wizard(id); w.tick();
    assert.equal(w.runSteps[0].status, "failed"); assert.equal(calls.checked, 0);
  }
  assert.equal(calls.patch.length, 0); assert.equal(calls.flash.length, 0);
});
test("a simulated firmware update cannot authorize a real boot flash", async () => {
  const w = wizard(); w.steps.push({ id: "fw-flash", enabled: true });
  await w.runRealRoot(w.runSteps[0]); assert.equal(w.runSteps[0].status, "failed"); assert.equal(calls.checked, 0);
});
test("firmware mismatch stops before patching or rebooting", async () => {
  const w = wizard(); api.bootImageCheck = async () => ({ ok: false, error: "firmware mismatch" });
  await w.runRealRoot(w.runSteps[0]); assert.equal(w.runSteps[0].status, "failed");
  assert.equal(calls.patch.length, 0); assert.equal(calls.reboot, 0);
});
test("cancellation during log registration prevents the next device write", async () => {
  for (const id of ["root", "unroot"]) {
    const w = wizard(id), subscription = deferred(), entered = deferred();
    const name = id === "root" ? "onMagiskLog" : "onFastbootLog";
    api[name] = () => { entered.resolve(); return subscription.promise; };
    const run = id === "root" ? w.runRealRoot(w.runSteps[0]) : w.runRealUnroot(w.runSteps[0]);
    await entered.promise; w.runGen++; subscription.resolve(() => calls.unsubscribed++); await run;
  }
  assert.equal(calls.patch.length, 0); assert.equal(calls.flash.length, 0); assert.equal(calls.unsubscribed, 2);
});
test("reboot failure blocks install and root/unroot completion", async () => {
  api.fastbootReboot = async () => false;
  for (const id of ["root", "unroot"]) {
    const w = wizard(id); await (id === "root" ? w.runRealRoot(w.runSteps[0]) : w.runRealUnroot(w.runSteps[0]));
    assert.equal(w.runSteps[0].status, "failed");
  }
  assert.equal(calls.install, 0);
});
test("a late root check cannot finish a new run", async () => {
  const w = wizard(), check = deferred(); api.rootCheck = () => check.promise;
  const run = w.finishRealRoot(w.runSteps[0]); w.runGen++; check.resolve(true); await run;
  assert.equal(w.runSteps[0].status, "running");
});
test("retry removes engine-added manual guides until staging completes again", () => {
  for (const [id, manuals] of [["root", ["su-grant"]], ["backup", ["backup-notice", "smsie-export"]], ["restore", ["contacts-import", "smsie-import"]]]) {
    const w = wizard(id); w.steps[0].manual = manuals; w.runSteps[0].status = "failed"; w.retryStep();
    assert.deepEqual(w.steps[0].manual, id === "backup" ? ["backup-notice"] : []);
  }
});
test("relock is blocked before mode-wait can reboot the phone", () => {
  const w = wizard("relock"); w.steps[0].manual = ["mode-wait"]; w.tick();
  assert.equal(w.runSteps[0].status, "failed"); assert.equal(w.manualCurrent, null); assert.equal(calls.reboot, 0);
});
test("real boot flows cannot skip phone verification", () => {
  const w = wizard(); w.manualCurrent = { id: "su-grant" }; assert.equal(w.manualSkippable, false);
});
test("reset clears patched artifacts and selecting an empty folder clears its status", async () => {
  const w = wizard(); w.patchedImage = "old.img"; w.restart(); assert.equal(w.patchedImage, "");
  await w.setFirmwareDir(""); assert.equal(w.firmwareDirState, "idle");
});
test("engine exceptions leave a failed step rather than an unhandled rejection", async () => {
  const w = wizard();
  w.dispatchEngine(async () => { throw new Error("transport failed"); });
  await Promise.resolve(); assert.equal(w.runSteps[0].status, "failed");
});
test("polling has an independent deadline for repeated errors and a hung query", async () => {
  assert.equal(await waitUntil(async () => { throw new Error("disconnect"); }, () => true, 30, 2), false);
  let queries = 0;
  assert.equal(await waitUntil(() => { queries++; return new Promise(() => {}); }, () => true, 30, 2), false);
  assert.equal(queries, 1);
});
test("polling cancellation discards late success without overlapping requests", async () => {
  let active = true, queries = 0; const pending = deferred();
  const result = waitUntil(() => { queries++; return pending.promise; }, () => active, 300, 2);
  active = false; assert.equal(await result, false); pending.resolve(true); assert.equal(queries, 1);
});
