// 루트 특성화(golden) 테스트 — 실기기로 "이 루트면 폰이 받아준다"가 검증된 경로가
// 리팩터 후에도 그대로 나오는지 감시한다. golden은 단언이 아니라 현재 동작에서 생성한 것이며,
// 루트를 의도적으로 바꾸면 이 테스트가 실패해 "검증된 기기 루트가 변했다"를 반드시 인지하게 한다.
// (XQ-DQ44 기준 검증 범위는 .plans/04-engine/device-test-checklist.md 참조)
import assert from "node:assert/strict";
import { after, before, beforeEach, test } from "node:test";
import { createServer } from "vite";

let server, Wizard, buildPlan, api, flags, originalApi, originalFlags;
before(async () => {
  server = await createServer({ server: { middlewareMode: true, watch: null, hmr: false, ws: false }, appType: "custom" });
  ({ Wizard } = await server.ssrLoadModule("/src/lib/stores/wizard.svelte.ts"));
  ({ buildPlan } = await server.ssrLoadModule("/src/lib/domain/plan.ts"));
  ({ api } = await server.ssrLoadModule("/src/lib/api/index.ts"));
  ({ REAL_STEPS: flags } = await server.ssrLoadModule("/src/lib/data/runMode.ts"));
  originalApi = { ...api }; originalFlags = { ...flags };
});
beforeEach(() => { delete globalThis.window; Object.assign(api, originalApi); });
after(async () => { Object.assign(api, originalApi); Object.assign(flags, originalFlags); await server.close(); });

// ── Part A: 플랜 루트 — 검증된 작업별 단계 id 순서(순수, buildPlan) ──
const device = (extra = {}) => ({ model: "XQ-DQ44", productName: "Xperia", serial: "sample", serialMasked: "sa****", firmware: "v1", android: "15", state: "device", rooted: true, bootloader: "unlocked", prep: { developerOptions: true, usbDebugging: true, oemUnlockAllowed: true }, sims: [{ slot: 1, state: "LOADED", carrier: "SKT", volte: "on" }], ...extra });
const config = () => ({ sims: [{ slot: 1, carrier: "SKT" }, { slot: 2, carrier: null }], firmware: "v2", bootloaderAction: "unlock" });
const popts = (o = {}) => ({ mode: "manual", manualTask: null, unroot: false, relock: false, restore: false, ...o });

// XQ-DQ44 실기기 검증 루트(2026-10, SIM1 SKT·init_boot). 리락은 미검증이라 기본 루트에 없다.
const PLAN_ROUTES = {
  "manual:unlock": { args: [device({ bootloader: "locked" }), config(), popts({ manualTask: "unlock" }), true], route: ["prep", "unlock", "setup-min"] },
  "manual:root": { args: [device({ rooted: false }), config(), popts({ manualTask: "root" }), true], route: ["prep", "root"] },
  "manual:unroot": { args: [device(), config(), popts({ manualTask: "unroot" }), true], route: ["prep", "unroot"] },
  "manual:volte": { args: [device(), config(), popts({ manualTask: "volte" }), true], route: ["efs-input", "efs-preflight", "efs", "verify", "volte-props", "final-verify"] },
  "manual:volte-rollback": { args: [device(), config(), popts({ manualTask: "volte-rollback" }), true], route: ["efs-rollback"] },
  "automatic": { args: [device({ bootloader: "locked", rooted: false }), config(), popts({ mode: "automatic", unroot: true, restore: true }), true], route: ["efs-input", "prep", "backup", "unlock", "setup-min", "root", "efs-preflight", "efs", "verify", "volte-props", "comm-check", "unroot", "restore", "final-verify"] },
};
for (const [name, { args, route }] of Object.entries(PLAN_ROUTES)) {
  test(`plan route stays: ${name}`, () => {
    assert.deepEqual(buildPlan(...args).map(s => s.id), route);
  });
}

// ── Part B: 실행 명령 루트 — EFS/VoLTE 단계별 백엔드 호출 순서(폰이 받는 명령 시퀀스) ──
// waitFor는 타이밍을 분리하려 true 고정(대기의 "존재"는 efs-native의 게이트 테스트가 따로 검증).
const ok = v => ({ ok: true, value: v });
function recWizard(id, calls) {
  flags.efs = true;
  const w = new Wizard();
  w.device = { model: "XQ-DQ44", serial: "TEST-SERIAL", state: "device" };
  w.volteConfig.sims = [{ slot: 1, carrier: "SKT" }, { slot: 2, carrier: null }];
  w.runSteps = [{ id, title: id, status: "running", progress: 0, logs: [], manualDone: 0 }];
  w.begin = () => {}; w.setGuard = () => {}; w.stepDone = c => { c.status = "done"; };
  w.waitFor = async () => true; w.persist = async () => true;
  const rec = (name, ret) => (...a) => { calls.push(name); return ret(...a); };
  Object.assign(api, originalApi, {
    efsConfiguration: rec("efsConfiguration", async () => ok({ port: "COM9", presetRoot: "C:/b", snapshotRoot: "C:/s" })),
    efsValidatePresets: rec("efsValidatePresets", async () => ok(null)),
    efsDiagOpen: rec("efsDiagOpen", async () => ok(null)),
    efsPreflight: rec("efsPreflight", async () => ok({ log: [], errors: [], warnings: [], parameters: [] })),
    efsSnapshot: rec("efsSnapshot", async () => ok({ path: "C:/s/snap", warnings: [] })),
    efsUpload: rec("efsUpload", async () => ok({ warnings: [], errors: [], filesSeen: 82, planned: 82, skipped: 0 })),
    efsVerify: rec("efsVerify", async () => ok({ matched: 82, total: 82, mismatches: [], warnings: [], excluded: 0 })),
    voltePropsSet: rec("voltePropsSet", async () => ok(["persist.dbg.volte_avail_ovr"])),
    efsRollback: rec("efsRollback", async () => ok({ warnings: [], errors: [] })),
    rootReboot: rec("rootReboot", async () => ok(null)),
    deviceWaitReady: rec("deviceWaitReady", async () => ok(null)),
    deviceList: rec("deviceList", async () => [w.device]),
    deviceRecordGet: rec("deviceRecordGet", async () => ok({ voltePatches: [{ at: "2026-10-07", slot: 1, carrier: "SKT", snapshot: "C:/s/snap1" }] })),
    deviceRecordMarkRolledBack: rec("deviceRecordMarkRolledBack", async () => ok({ voltePatches: [] })),
    onEfsLog: async () => () => {}, onEfsProgress: async () => () => {},
    journalSave: async () => true, runGuard: async () => true,
  });
  return w;
}
async function settled(w) { for (let i = 0; i < 200; i++) { await new Promise(r => setImmediate(r)); if (w.busy === 0) return; } assert.fail("engine did not settle"); }

// 검증된 EFS/VoLTE 단계의 명령 순서. DIAG→EFS→setprop→재부팅 순서가 폰이 받아준 루트다.
const EXEC_ROUTES = {
  "efs-preflight": ["efsConfiguration", "efsValidatePresets", "efsDiagOpen", "efsPreflight"],
  "efs": ["efsConfiguration", "efsValidatePresets", "efsSnapshot", "efsUpload", "efsUpload"],
  "volte-props": ["voltePropsSet", "deviceWaitReady"],
  "efs-rollback": ["deviceRecordGet", "efsDiagOpen", "efsRollback", "deviceRecordMarkRolledBack", "rootReboot", "deviceWaitReady"],
};
for (const [id, route] of Object.entries(EXEC_ROUTES)) {
  test(`exec command route stays: ${id}`, async () => {
    const calls = []; const w = recWizard(id, calls);
    w.tick(); await settled(w);
    assert.equal(w.runSteps[0].status, "done", `step ${id} did not complete: ${w.stepError ?? ""}`);
    assert.deepEqual(calls, route);
  });
}
