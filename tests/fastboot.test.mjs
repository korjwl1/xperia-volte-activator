import assert from "node:assert/strict";
import { after, before, beforeEach, test } from "node:test";
import { createServer } from "vite";

// Vite/Svelte가 실제 store를 컴파일한다. API는 전부 메모리 가짜로 교체한다.
let server, Wizard, api, REAL_STEPS, originalApi, originalFlags, calls;
before(async () => {
  server = await createServer({ server: { middlewareMode: true, watch: null, hmr: false, ws: false }, appType: "custom" });
  ({ Wizard } = await server.ssrLoadModule("/src/lib/stores/wizard.svelte.ts"));
  ({ api } = await server.ssrLoadModule("/src/lib/api/index.ts"));
  ({ REAL_STEPS } = await server.ssrLoadModule("/src/lib/data/runMode.ts"));
  originalApi = { ...api };
  originalFlags = { ...REAL_STEPS };
});
after(async () => {
  if (api) Object.assign(api, originalApi);
  if (REAL_STEPS) Object.assign(REAL_STEPS, originalFlags);
  await server?.close();
});
beforeEach(() => {
  calls = { unlock: 0, reboot: 0, probe: 0, unsubscribe: 0 };
  Object.assign(REAL_STEPS, { backup: false, restore: false, fastboot: true });
  Object.assign(api, originalApi, {
    onFastbootLog: async () => () => calls.unsubscribe++,
    fastbootGetvar: async () => { calls.probe++; return { unlocked: "no", "is-userspace": "no" }; },
    fastbootUnlock: async () => { calls.unlock++; return { ok: true, value: { unlocked: true } }; },
    fastbootReboot: async () => { calls.reboot++; return { ok: true, value: null }; },
  });
});
function wizard() {
  const w = new Wizard();
  w.unlockCode = "1234567890abcdef";
  w.runSteps = [{ id: "unlock", title: "언락", status: "running", progress: 0, logs: [], manualDone: 0 }];
  w.persist = async () => true;
  w.setGuard = () => {};
  w.begin = () => { w.running = true; };
  return w;
}

test("fastboot alone still requires a real complete backup when selected", async () => {
  const w = wizard();
  w.runSteps.push({ id: "backup", title: "백업", status: "done", progress: 1, logs: [], manualDone: 0 });
  w.runSteps[0].status = "pending";
  w.tick();
  // 게이트는 비동기 함수 — 판정이 끝날 때까지 기다린다
  while (w.busy > 0) await new Promise(resolve => setTimeout(resolve, 0));
  assert.equal(w.runSteps[0].status, "failed");
  assert.match(w.stepError, /백업/);
  assert.equal(calls.unlock, 0);
});

test("re-entering an already dispatched real step cannot simulate completion", () => {
  const w = wizard();
  w.engineRan.set("unlock", w.runGen);
  w.runSteps[0].progress = 0.99;
  w.tick();
  assert.equal(w.runSteps[0].progress, 0.99);
  assert.equal(w.runSteps[0].status, "running");
});

test("unknown state and fastbootd never dispatch unlock", async () => {
  for (const vars of [{ "is-userspace": "no" }, { unlocked: "no", "is-userspace": "yes" }]) {
    const w = wizard();
    api.fastbootGetvar = async () => vars;
    await w.runRealUnlock(w.runSteps[0]);
    assert.equal(w.runSteps[0].status, "failed");
  }
  assert.equal(calls.unlock, 0);
  assert.equal(calls.reboot, 0);
});

test("unconfirmed unlock never reboots", async () => {
  const w = wizard();
  api.fastbootUnlock = async () => ({ ok: true, value: { unlocked: false } });
  await w.runRealUnlock(w.runSteps[0]);
  assert.equal(w.runSteps[0].status, "failed");
  assert.equal(calls.reboot, 0);
  assert.equal(calls.unsubscribe, 1);
});

test("already unlocked skips the destructive command and confirms reboot", async () => {
  const w = wizard();
  api.fastbootGetvar = async () => ({ unlocked: " YES ", "is-userspace": "no" });
  await w.runRealUnlock(w.runSteps[0]);
  assert.equal(calls.unlock, 0);
  assert.equal(calls.reboot, 1);
  assert.equal(w.runSteps[0].status, "done");
});

test("unlock waits for reboot before advancing, and reboot failure stops it", async () => {
  for (const success of [true, false]) {
    const w = wizard();
    let finishReboot, enteredReboot;
    const entered = new Promise(resolve => { enteredReboot = resolve; });
    api.fastbootReboot = () => { enteredReboot(); return new Promise(resolve => { finishReboot = resolve; }); };
    const run = w.runRealUnlock(w.runSteps[0]);
    await entered;
    assert.equal(w.runSteps[0].status, "running");
    finishReboot(success ? { ok: true, value: null } : { ok: false, error: "no OKAY" });
    await run;
    assert.equal(w.runSteps[0].status, success ? "done" : "failed");
  }
});

test("cancel while subscribing prevents even the subsequent probe", async () => {
  const w = wizard();
  let finishSubscribe;
  api.onFastbootLog = () => new Promise(resolve => { finishSubscribe = resolve; });
  const run = w.runRealUnlock(w.runSteps[0]);
  w.runGen++;
  finishSubscribe(() => calls.unsubscribe++);
  await run;
  assert.equal(calls.probe, 0);
  assert.equal(calls.unlock, 0);
  assert.equal(calls.unsubscribe, 1);
});

test("simulated unroot completion cannot authorize relock", async () => {
  const w = wizard();
  w.runSteps[0].id = "relock";
  w.runSteps.push({ id: "unroot", title: "언루팅", status: "done", progress: 1, logs: [], manualDone: 0 });
  let locks = 0;
  api.fastbootLock = async () => { locks++; return { ok: true, value: { unlocked: false } }; };
  await w.runRealRelock(w.runSteps[0]);
  assert.equal(w.runSteps[0].status, "failed");
  assert.equal(locks, 0);
});
