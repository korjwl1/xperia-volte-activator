import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import { createServer } from "vite";
let server, domain, buildPlan, Wizard, executionPlanProblem, decodeJournal, api, originalApi;
before(async () => {
  server = await createServer({ server: { middlewareMode: true, watch: null, hmr: false, ws: false }, appType: "custom" });
  domain = await server.ssrLoadModule("/src/lib/domain/moduleSets.ts");
  ({ buildPlan } = await server.ssrLoadModule("/src/lib/domain/plan.ts"));
  ({ Wizard } = await server.ssrLoadModule("/src/lib/stores/wizard.svelte.ts"));
  ({ executionPlanProblem } = await server.ssrLoadModule("/src/lib/domain/execution.ts"));
  ({ decodeJournal } = await server.ssrLoadModule("/src/lib/domain/journal.ts"));
  ({ api } = await server.ssrLoadModule("/src/lib/api/index.ts")); originalApi = { ...api };
});
after(async () => { Object.assign(api, originalApi); await server.close(); });
const device = () => ({ state: "device", serial: "sample", serialMasked: "sa****", model: "XQ-DQ44", rooted: true, bootloader: "unlocked", firmware: "v1", prep: {}, sims: [{ slot: 1, carrier: "SKT", volte: "on" }] });
const config = () => ({ sims: [{ slot: 1, carrier: "SKT" }, { slot: 2, carrier: null }], firmware: null, bootloaderAction: null });
const selection = () => ({ ...domain.toggleModuleSet(domain.emptyModuleSelection(), "evasion", true), settingsAck: true });
function fake(engine = "magisk", fail = "") {
  const events = []; let current, boots = 0, inventory = { engine, modules: [], installed: {}, rebootRequired: false, uncertain: false, bootId: "boot-0" };
  const port = {
    rootInspect: async () => ({ ok: true, value: { access: "granted", engine } }),
    rootModulesInspect: async () => ({ ok: true, value: inventory }),
    rootPackagePrepare: async id => { events.push(`prepare:${id}`); current = id; return { ok: true, value: { id, moduleId: id, sha256: "a".repeat(64), external: true } }; },
    rootModuleInstall: async () => {
      events.push(`install:${current}`); if (current === fail) return { ok: false, error: "failure" };
      inventory = { ...inventory, modules: [...inventory.modules, { id: current, state: "enabled" }], installed: { ...inventory.installed, [current]: current }, rebootRequired: true };
      return { ok: true, value: inventory };
    },
    rootReboot: async () => { events.push("reboot"); inventory = { ...inventory, rebootRequired: false, bootId: `boot-${++boots}` }; return { ok: true, value: null }; },
    rootPresetPush: async () => { events.push("preset"); return { ok: true, value: "/storage/emulated/0/Download/hma.json" }; },
  };
  const hooks = { check() {}, progress() {}, rebooted: async () => { events.push("verify"); return inventory; }, instruction: async message => { events.push(`instruction:${message}`); } };
  return { port, hooks, events, get inventory() { return inventory; } };
}
test("set dependencies are automatic, cannot be deselected under dependents and use engine-specific packages", () => {
  const s = selection(); assert.deepEqual(s.sets, ["foundation", "evasion"]);
  assert.deepEqual(domain.toggleModuleSet(s, "foundation", false).sets, s.sets);
  assert.deepEqual(domain.toggleModuleSet(s, "evasion", false).sets, ["foundation"]);
  const magisk = domain.moduleSetPackages(s, "magisk"), ksu = domain.moduleSetPackages(s, "kernelsu-family");
  assert.equal(ksu[0], "overlayfs"); assert.ok(!magisk.includes("overlayfs"));
  assert.ok(magisk.indexOf("tricky-store") < magisk.indexOf("tricky-addon"));
  // 세트 안 구성은 고정 프리셋 — 예전 기록의 자유 선택값은 무시한다(2026-10-08 사용자 결정)
  assert.deepEqual(magisk, ["neozygisk", "bootloop-protector", "play-integrity-fork", "tricky-store", "tricky-addon", "hma"]);
  assert.deepEqual(domain.moduleSetPackages({ ...s, zygisk: "zygisk-next", integrity: "integrity-box", extras: ["shamiko"] }, "magisk"), magisk);
});
test("automatic modules follow VoLTE configuration, disappear for unroot/relock/unlock and never enter update/manual plans", () => {
  const opts = { mode: "automatic", unroot: false, relock: false, restore: false, modules: selection() };
  const ids = buildPlan(device(), config(), opts, false).map(step => step.id);
  assert.ok(ids.indexOf("root-modules") > ids.indexOf("volte-props")); assert.ok(ids.indexOf("root-modules") < ids.indexOf("final-verify"));
  for (const extra of [{ unroot: true }, { relock: true }]) assert.ok(!buildPlan(device(), config(), { ...opts, ...extra }, false).some(step => step.id === "root-modules"));
  assert.ok(!buildPlan({ ...device(), bootloader: "locked" }, config(), opts, false).some(step => step.id === "root-modules"));
  assert.ok(!buildPlan(device(), config(), { ...opts, mode: "manual", manualTask: "volte" }, false).some(step => step.id === "root-modules"));
  const w = new Wizard(); w.device = device(); w.volteConfig = config(); w.opts = opts;
  w.opts.relock = true; w.clearModuleSets(); assert.equal(w.moduleSelection.sets.length, 0); w.setModuleSelection(selection()); assert.equal(w.moduleSelection.sets.length, 0);
  const flags = Object.fromEntries(["backup", "restore", "fastboot", "root", "verify", "efs"].map(id => [id, false]));
  assert.ok(executionPlanProblem(["root-modules"], flags));
});
test("set installation verifies every reboot and executes phone-setting gates", async () => {
  const f = fake("kernelsu-family"); const result = await domain.installModuleSets(f.port, "sample", selection(), f.hooks);
  assert.equal(result.rebootRequired, false); assert.match(f.events[0], /^instruction:ReSukiSU/); assert.equal(f.events[1], "prepare:overlayfs");
  for (const [index, event] of f.events.entries()) if (event.startsWith("install:")) assert.deepEqual(f.events.slice(index + 1, index + 3), ["reboot", "verify"]);
  // 매니저 설정(설치 전) + PIF·TrickyAddon·HMA 설정
  assert.equal(f.events.filter(event => event.startsWith("instruction:")).length, 4);
  // HMA는 카페 프리셋을 폰 Download에 넣은 뒤 가져오기만 안내한다
  assert.ok(f.events.indexOf("preset") >= 0 && f.events.indexOf("preset") < f.events.findIndex(event => event.includes("카페 HMA 프리셋")));
});
test("failed, ambiguous or cancelled module work never installs the next dependency", async () => {
  const failed = fake("magisk", "neozygisk"); await assert.rejects(domain.installModuleSets(failed.port, "sample", selection(), failed.hooks), /failure/);
  assert.deepEqual(failed.events.filter(event => !event.startsWith("instruction:")), ["prepare:neozygisk", "install:neozygisk"]);
  const cancelled = fake(); let stopped = false;
  cancelled.hooks.instruction = async () => { stopped = true; }; cancelled.hooks.check = () => { if (stopped) throw new Error("cancelled"); };
  await assert.rejects(domain.installModuleSets(cancelled.port, "sample", selection(), cancelled.hooks), /cancelled/);
  assert.ok(!cancelled.events.includes("prepare:tricky-store"));
  const uncertain = fake(); uncertain.hooks.rebooted = async () => ({ ...uncertain.inventory, uncertain: true });
  await assert.rejects(domain.installModuleSets(uncertain.port, "sample", selection(), uncertain.hooks)); assert.ok(!uncertain.events.includes("prepare:bootloop-protector"));
});
test("resume uses installation receipts but still shows manager and phone-setting instructions", async () => {
  const f = fake(); await domain.installModuleSets(f.port, "sample", selection(), f.hooks); f.events.length = 0;
  await domain.installModuleSets(f.port, "sample", selection(), f.hooks);
  assert.ok(!f.events.some(event => event.startsWith("install:"))); assert.equal(f.events.filter(event => event.startsWith("instruction:")).length, 4);
  f.inventory.installed.neozygisk = "another-module";
  f.events.length = 0; await domain.installModuleSets(f.port, "sample", selection(), f.hooks); assert.ok(f.events.includes("install:neozygisk"));
});
test("a reboot ACK or a settings acknowledgement cannot replace a changed boot identity", async () => {
  const noBoot = fake(); noBoot.port.rootReboot = async () => ({ ok: true, value: null });
  await assert.rejects(domain.installModuleSets(noBoot.port, "sample", selection(), noBoot.hooks), /새 OS 부팅/);
  assert.ok(!noBoot.events.includes("prepare:bootloop-protector"));
  const settings = fake(); let stopBoots = false; const reboot = settings.port.rootReboot;
  settings.hooks.instruction = async () => { stopBoots = true; };
  settings.port.rootReboot = async (...args) => stopBoots ? { ok: true, value: null } : reboot(...args);
  await assert.rejects(domain.installModuleSets(settings.port, "sample", selection(), settings.hooks), /새 OS 부팅/);
  assert.ok(!settings.events.includes("prepare:tricky-store"));
});
test("feature and setting preflight block automatic modules before any phone operation", async () => {
  const w = new Wizard(); w.device = device(); w.volteConfig = config(); w.opts = { mode: "automatic", unroot: false, relock: false, restore: false, modules: selection() };
  w.steps = w.plan; w.prepareRun(); w.setGuard = () => {}; let phoneCalls = 0;
  api.rootToolsCapabilities = async () => ({ ok: true, value: { writeEnabled: false, switchEnabled: false } });
  api.engineCapabilities = async () => { phoneCalls++; throw new Error("must not run"); }; api.rootInspect = async () => { phoneCalls++; throw new Error("must not run"); };
  await w.startCheckedRun(); assert.equal(phoneCalls, 0); assert.equal(w.runSteps[0].status, "failed");
  Object.assign(api, originalApi);
});
test("module journals retain set dependencies and reject incompatible post-processing or unselected module steps", () => {
  const opts = { mode: "automatic", unroot: false, relock: false, restore: false, modules: selection() };
  const plan = buildPlan(device(), config(), opts, false);
  const journal = { version: 1, model: "XQ-DQ44", productName: "Xperia", serialMasked: "sa****", startedAt: "2026-10-08", updatedAt: "2026-10-08", config: config(), opts, backupPath: "", firmwareDir: "", firmware: null, backupItems: [], cursor: 0, steps: plan, runSteps: plan.map(step => ({ id: step.id, title: step.title, status: "pending", progress: 0, manualDone: 0, logs: [] })), stop: null };
  assert.deepEqual(decodeJournal(JSON.stringify(journal)).opts.modules.sets, ["foundation", "evasion"]);
  for (const change of [{ modules: { ...selection(), sets: ["evasion"] } }, { relock: true }, { unroot: true }, { modules: domain.emptyModuleSelection() }]) {
    assert.equal(decodeJournal(JSON.stringify({ ...journal, opts: { ...opts, ...change } })), null);
  }
});
