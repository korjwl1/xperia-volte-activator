import type { ApiResult, DeviceStatus, RootModuleInventory, RootModuleSelection, RootPackage, RootState, VolteConfig } from "$lib/types";
import { requireResult } from "$lib/domain/rootTools";

export const MODULE_SETS = [
  { id: "foundation", name: "Set A · 기초 모듈", detail: "Zygisk · 부트루프 보호 · KernelSU 계열은 OverlayFS 선행", depends: [] },
  { id: "evasion", name: "Set B · 루팅 감지 회피", detail: "Integrity · TrickyStore · TrickyAddon · HMA 카페 프리셋", depends: ["foundation"] },
] as const;
export const emptyModuleSelection = (): RootModuleSelection => ({ sets: [], zygisk: "neozygisk", integrity: "play-integrity-fork", extras: [], settingsAck: false });
export function selectedModuleSets(selection?: RootModuleSelection): RootModuleSelection["sets"] {
  const picked = new Set(selection?.sets ?? []);
  for (const set of [...MODULE_SETS].reverse()) if (picked.has(set.id)) for (const dependency of set.depends) picked.add(dependency);
  return MODULE_SETS.filter(set => picked.has(set.id)).map(set => set.id);
}
export function toggleModuleSet(selection: RootModuleSelection, id: "foundation" | "evasion", checked: boolean): RootModuleSelection {
  const sets = selectedModuleSets(selection);
  if (!checked && MODULE_SETS.some(set => sets.includes(set.id) && set.depends.some(dependency => dependency === id))) return selection;
  const next = selectedModuleSets({ ...selection, sets: checked ? [...sets, id] : sets.filter(set => set !== id) });
  return { ...selection, sets: next, extras: selection.extras.filter(extra => extra === "play-store-fix" ? next.includes("foundation") : next.includes("evasion")), settingsAck: false };
}
export function isModuleSelection(value: unknown): value is RootModuleSelection {
  if (!value || typeof value !== "object") return false;
  const s = value as RootModuleSelection;
  return Array.isArray(s.sets) && s.sets.every(id => MODULE_SETS.some(set => set.id === id)) && new Set(s.sets).size === s.sets.length
    && (!s.sets.includes("evasion") || s.sets.includes("foundation")) && ["neozygisk", "rezygisk", "zygisk-next"].includes(s.zygisk)
    && ["play-integrity-fork", "integrity-box"].includes(s.integrity) && Array.isArray(s.extras)
    && s.extras.every(id => ["play-store-fix", "zygisk-assistant", "shamiko"].includes(id)) && new Set(s.extras).size === s.extras.length
    && (!s.extras.includes("shamiko") || s.zygisk === "zygisk-next") && typeof s.settingsAck === "boolean";
}
export function automaticModulesProblem(device: DeviceStatus | null, config: VolteConfig, opts: { unroot: boolean; relock: boolean }): string | null {
  if (!device || device.state !== "device") return "연결된 기기를 확인하세요";
  if (opts.unroot || opts.relock) return "언루팅·리락 뒤에는 모듈을 사용할 수 없어 선택할 수 없습니다";
  if (device.bootloader !== "unlocked") return "언락·초기화가 포함되거나 잠금 상태가 불명확하면 모듈을 선택할 수 없습니다";
  return config.sims.some(sim => sim.carrier !== null) ? null : "자동 VoLTE 작업을 먼저 선택하세요";
}
export function moduleSetPackages(selection: RootModuleSelection, engine: RootState["engine"]): string[] {
  if (!isModuleSelection(selection)) throw new Error("모듈 세트 선택값이 올바르지 않습니다");
  if (!["magisk", "kernelsu-family"].includes(engine)) throw new Error("지원하는 단일 루트 엔진을 확인하세요");
  if (engine !== "magisk" && selection.extras.includes("shamiko")) throw new Error("Shamiko는 Magisk + Zygisk Next 전용입니다");
  const sets = selectedModuleSets(selection);
  const packages: string[] = [];
  if (sets.includes("foundation")) packages.push(...(engine === "kernelsu-family" ? ["overlayfs"] : []), selection.zygisk, "bootloop-protector", ...selection.extras.filter(id => id === "play-store-fix"));
  if (sets.includes("evasion")) packages.push(selection.integrity, "tricky-store", "tricky-addon", "hma", ...selection.extras.filter(id => id !== "play-store-fix"));
  return [...new Set(packages)];
}
export interface ModuleSetPort {
  rootInspect(serial: string): Promise<ApiResult<RootState>>;
  rootModulesInspect(serial: string): Promise<ApiResult<RootModuleInventory>>;
  rootPackagePrepare(id: string): Promise<ApiResult<RootPackage>>;
  rootModuleInstall(serial: string, sha256: string, confirm: boolean, confirmExternal: boolean): Promise<ApiResult<RootModuleInventory>>;
  rootReboot(serial: string, target: "os"): Promise<ApiResult<unknown>>;
}
export interface ModuleSetHooks {
  check(): void;
  progress(message: string, done: number, total: number): void;
  rebooted(engine: RootState["engine"], previousBootId: string): Promise<RootModuleInventory>;
  instruction(message: string): Promise<void>;
}
/** Native receipts and each reboot are checked before continuing. No automatic conflict removal/retry. */
export async function installModuleSets(port: ModuleSetPort, serial: string, selection: RootModuleSelection, hooks: ModuleSetHooks): Promise<RootModuleInventory> {
  hooks.check();
  if (!serial || !selection.settingsAck) throw new Error("기기와 매니저 설정 확인이 필요합니다");
  const root = requireResult(await port.rootInspect(serial)); hooks.check();
  if (root.access !== "granted") throw new Error("Shell 루트 권한을 허용하세요");
  const packages = moduleSetPackages(selection, root.engine);
  let inventory = requireResult(await port.rootModulesInspect(serial)); hooks.check();
  const reboot = async () => {
    const before = requireResult(await port.rootModulesInspect(serial)); hooks.check();
    if (!before.bootId || before.uncertain || before.engine !== root.engine) throw new Error("재부팅 전 부팅 ID·엔진·모듈 상태를 확인하지 못했습니다");
    requireResult(await port.rootReboot(serial, "os")); hooks.check();
    const after = await hooks.rebooted(root.engine, before.bootId); hooks.check();
    if (!after.bootId || after.bootId === before.bootId) throw new Error("새 OS 부팅을 확인하지 못했습니다");
    return after;
  };
  for (const [index, id] of packages.entries()) {
    if (!inventory.bootId || inventory.engine !== root.engine || inventory.uncertain || inventory.rebootRequired) throw new Error("이전 모듈 설치의 부팅 ID·재부팅·불확정 결과를 먼저 확인하세요");
    const owned = inventory.installed?.[id];
    let appliedId = owned;
    if (owned && inventory.modules.some(module => module.id === owned && module.state === "enabled")) {
      hooks.progress(`${id}: 기존 설치와 적용 확인`, index, packages.length);
    } else {
      hooks.progress(`${id}: 파일 준비`, index, packages.length);
      const prepared = requireResult(await port.rootPackagePrepare(id)); hooks.check();
      if (prepared.id !== id || !prepared.moduleId) throw new Error("준비한 모듈의 종류가 선택과 다릅니다");
      appliedId = prepared.moduleId;
      hooks.progress(`${id}: 설치 중`, index, packages.length);
      inventory = requireResult(await port.rootModuleInstall(serial, prepared.sha256, true, prepared.external)); hooks.check();
      if (inventory.uncertain || !inventory.rebootRequired || inventory.engine !== root.engine) throw new Error("모듈 설치 결과를 확인할 수 없습니다");
      hooks.progress(`${id}: 재부팅·적용 확인 중`, index, packages.length);
      inventory = await reboot();
      if (inventory.uncertain || inventory.rebootRequired || inventory.engine !== root.engine || !inventory.modules.some(module => module.id === prepared.moduleId && module.state === "enabled")) throw new Error("재부팅 후 모듈 적용을 확인하지 못했습니다");
    }
    if (id === "play-integrity-fork") await hooks.instruction("폰 매니저에서 PlayIntegrityFork Action(autopif)을 실행하고 설정을 확인하세요.");
    if (id === "tricky-addon") await hooks.instruction("폰에 WebUI를 설치한 뒤 TrickyAddon의 target·keybox를 직접 설정하세요. 설정 완료 후 계속하세요.");
    if (id === "hma") await hooks.instruction("HMA에 동봉된 소니 카페 JSON을 그대로 가져오고 필요한 은행 앱 scope를 확인하세요. 설정을 직접 구성해 대체하지 않습니다.");
    if (["play-integrity-fork", "tricky-addon", "hma"].includes(id)) {
      hooks.progress(`${id}: 폰 설정 후 재부팅·적용 확인 중`, index, packages.length);
      hooks.check(); inventory = await reboot();
      if (inventory.uncertain || inventory.rebootRequired || inventory.engine !== root.engine || !inventory.modules.some(module => module.id === appliedId && module.state === "enabled")) throw new Error("설정 후 재부팅 결과를 확인하지 못했습니다");
    }
    hooks.check(); hooks.progress(`${id}: 재부팅·적용 확인`, index + 1, packages.length);
  }
  return inventory;
}
export async function waitForModuleReboot(port: ModuleSetPort, serial: string, engine: RootState["engine"], check: () => void, previousBootId: string, timeoutMs = 180000): Promise<RootModuleInventory> {
  const until = Date.now() + timeoutMs;
  do {
    check();
    const result = await port.rootModulesInspect(serial); check();
    if (result.ok && !result.value.bootId) throw new Error("부팅 ID를 조회하지 못했습니다");
    if (result.ok && result.value.bootId !== previousBootId && !result.value.rebootRequired) {
      if (result.value.uncertain || result.value.engine !== engine) throw new Error("재부팅 후 모듈·엔진 상태가 불확정입니다");
      return result.value;
    }
    await new Promise(resolve => setTimeout(resolve, 1500));
  } while (Date.now() < until);
  throw new Error("재부팅 후 같은 폰의 루트 권한·모듈 적용을 확인하지 못했습니다. Shell 권한과 연결을 확인하세요");
}
