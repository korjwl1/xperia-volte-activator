import type { EfsConfiguration, EfsError, EfsLogEvent, EfsPreflight, EfsProgress, EfsResult, EfsSnapshotResult, EfsToolCheck, EfsUploadResult, EfsVerifyReport } from "$lib/types";
import { EFS_PRESETS } from "$lib/data/efsPresets";

const inTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
let browserConfiguration: EfsConfiguration | null = null;
async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<EfsResult<T>> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    return { ok: true, value: await invoke<T>(command, args) };
  } catch (e) {
    if (e && typeof e === "object" && "code" in e && "message" in e) {
      const details = e as EfsError;
      const cleanup = details.cleanup?.length ? `; cleanup: ${details.cleanup.join(" / ")}` : "";
      return { ok: false, details, error: `[${details.code}] ${details.operation}: ${details.message}${cleanup}` };
    }
    return { ok: false, error: typeof e === "string" ? e : String(e) };
  }
}
async function configuration(): Promise<EfsResult<EfsConfiguration>> {
  const r = await efsApi.efsConfiguration();
  if (!r.ok) return r;
  return r.value ? { ok: true, value: r.value } : { ok: false, error: "EFS 설정이 없습니다. api.efsConfigure({ port, presetRoot, snapshotRoot })로 명시적으로 지정해 주세요" };
}
async function argsFor(presetDir: string): Promise<EfsResult<{ port: string; presetDir: string }>> {
  const cfg = await configuration();
  if (!cfg.ok) return cfg;
  const path = await invoke<string>("efs_resolve_preset", { folder: presetDir });
  return path.ok ? { ok: true, value: { port: cfg.value.port, presetDir: path.value } } : path;
}
const mockWarning = { code: "simulation", target: "mock", message: "브라우저 시뮬레이션 — 기기에서 쓰거나 검증한 결과가 아닙니다" };
function mockUpload(presetDir: string): EfsUploadResult {
  const count = EFS_PRESETS.find(p => p.folder === presetDir)?.files ?? 0;
  const skipped = presetDir.includes("XPERIAsonyKT") ? 2 : 0;
  const warnings = [mockWarning];
  if (skipped) for (const id of [6789, 6849]) warnings.push({ code: "emptyNvSkipped", target: `NV ${id}`, message: "0바이트 NV 항목 — 시뮬레이션에서도 쓰기·검증 수에서 제외" });
  return { errors: [], filesSeen: count - skipped, planned: count, skipped, warnings };
}
async function listen<T>(event: string, cb: (value: T) => void): Promise<() => void> {
  if (!inTauri()) return () => {};
  try { const { listen } = await import("@tauri-apps/api/event"); return await listen<T>(event, e => cb(e.payload)); }
  catch { return () => {}; }
}
export interface EfsApi {
  efsValidatePresets(folders: string[]): Promise<EfsResult<null>>;
  efsConfigure(configuration: EfsConfiguration): Promise<EfsResult<null>>;
  efsConfiguration(): Promise<EfsResult<EfsConfiguration | null>>;
  efsToolCheck(): Promise<EfsResult<EfsToolCheck>>;
  efsDiagOpen(serial: string | undefined): Promise<EfsResult<null>>;
  efsPreflight(): Promise<EfsResult<EfsPreflight>>;
  efsUpload(presetDir: string): Promise<EfsResult<EfsUploadResult>>;
  efsVerify(presetDir: string): Promise<EfsResult<EfsVerifyReport>>;
  efsSnapshot(dest: string, presetDir: string): Promise<EfsResult<EfsSnapshotResult>>;
  efsRollback(snapshot: string): Promise<EfsResult<EfsUploadResult>>;
  efsCancel(): Promise<EfsResult<null>>;
  onEfsLog(cb: (event: EfsLogEvent) => void): Promise<() => void>;
  onEfsProgress(cb: (event: EfsProgress) => void): Promise<() => void>;
}
export const efsApi: EfsApi = {
  async efsValidatePresets(folders) {
    if (!inTauri()) return { ok: true, value: null };
    const presetDirs: string[] = [];
    for (const folder of folders) {
      const r = await argsFor(folder); if (!r.ok) return r;
      presetDirs.push(r.value.presetDir);
    }
    return invoke("efs_validate_presets", { presetDirs });
  },
  async efsConfigure(configuration) {
    if (!inTauri()) { browserConfiguration = configuration; return { ok: true, value: null }; }
    return invoke("efs_config_set", { configuration });
  },
  async efsConfiguration() { return inTauri() ? invoke("efs_config_get") : { ok: true, value: browserConfiguration }; },
  async efsToolCheck() { return inTauri() ? invoke("efs_tool_check") : { ok: true, value: { version: "native-rust-v1 (mock)", path: "built-in", native: true, deviceExecution: false } }; },
  async efsDiagOpen(serial) {
    if (!inTauri()) return { ok: true, value: null };
    if (!serial) return { ok: false, error: "DIAG 전환에는 선택한 기기의 ADB 일련번호가 필요합니다" };
    return invoke("efs_diag_open", { serial });
  },
  async efsPreflight() {
    if (!inTauri()) return { ok: true, value: { log: ["시뮬레이션 EFS 점검"], errors: [], warnings: [mockWarning.message], parameters: [] } };
    const cfg = await configuration(); return cfg.ok ? invoke("efs_preflight", { port: cfg.value.port }) : cfg;
  },
  async efsUpload(presetDir) {
    if (!inTauri()) return { ok: true, value: mockUpload(presetDir) };
    const args = await argsFor(presetDir); return args.ok ? invoke("efs_upload", args.value) : args;
  },
  async efsVerify(presetDir) {
    if (!inTauri()) { const mock = mockUpload(presetDir); return { ok: true, value: { ok: true, files: mock.filesSeen, matched: mock.filesSeen, planned: mock.planned, skipped: mock.skipped, missing: [], mismatches: [], warnings: mock.warnings } }; }
    const args = await argsFor(presetDir); return args.ok ? invoke("efs_verify", args.value) : args;
  },
  async efsSnapshot(dest, presetDir) {
    if (!inTauri()) return { ok: true, value: { path: dest, filesSeen: mockUpload(presetDir).filesSeen, warnings: [mockWarning] } };
    const args = await argsFor(presetDir); return args.ok ? invoke("efs_snapshot", { ...args.value, dest }) : args;
  },
  async efsRollback(snapshot) {
    if (!inTauri()) return { ok: true, value: mockUpload("") };
    const cfg = await configuration(); return cfg.ok ? invoke("efs_rollback", { port: cfg.value.port, snapshot }) : cfg;
  },
  async efsCancel() { return inTauri() ? invoke("efs_cancel") : { ok: true, value: null }; },
  onEfsLog: cb => listen("efs:log", cb),
  onEfsProgress: cb => listen("efs:progress", cb),
};
