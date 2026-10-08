import type { ApiResult, DeviceStatus, RootModuleInventory, RootModuleSelection, RootPackage, RootState, VolteConfig } from "$lib/types";
import { requireResult } from "$lib/domain/rootTools";

export const MODULE_SETS = [
  { id: "foundation", name: "Set A · 기초 모듈", detail: "Zygisk와 부트루프 보호", depends: [] },
  { id: "evasion", name: "Set B · 루팅 감지 회피", detail: "금융 앱·Play Integrity 대응 (Set A 포함)", depends: ["foundation"] },
] as const;

// 세트 안 구성은 엔진별로 고정한다(2026-10-08 사용자 결정) — 비전문가에게 조합을 고르게 하지 않는다.
// for-rooted-phone.md 설치 순서의 추천값: NeoZygisk(두 엔진 공통 추천) · PIF · TrickyStore → TrickyAddon · HMA(카페 JSON).
// KernelSU 계열은 OverlayFS를 먼저 설치한다. 구성 순서가 설치 순서다.
export const MODULE_PRESET: Record<"foundation" | "evasion", { id: string; name: string; role: string; kernelsuOnly?: boolean }[]> = {
  foundation: [
    { id: "overlayfs", name: "OverlayFS MetaModule", role: "KernelSU 계열의 모듈 마운트 기반", kernelsuOnly: true },
    { id: "neozygisk", name: "NeoZygisk", role: "Zygisk 구현체" },
    { id: "bootloop-protector", name: "AshReXcue", role: "부트루프 보호" },
  ],
  evasion: [
    { id: "play-integrity-fork", name: "PlayIntegrityFork", role: "Play Integrity 대응" },
    { id: "tricky-store", name: "TrickyStore", role: "키 증명(keybox) 대응" },
    { id: "tricky-addon", name: "TrickyAddon", role: "TrickyStore 대상·keybox 관리" },
    { id: "hma", name: "Hide My Applist", role: "루팅 앱 숨김 · 카페 프리셋" },
  ],
};
/** 지금 루팅 엔진의 매니저 앱 이름 */
export function managerAppName(engine: RootState["engine"]): string {
  return engine === "kernelsu-family" ? "ReSukiSU" : "Magisk";
}
/** 모듈을 설치하기 전 폰의 매니저에서 한 번 맞출 설정 — 실행 중 안내로 보여 준다(계획 단계의 체크 대신) */
export function managerSettingsInstruction(engine: RootState["engine"]): string {
  return engine === "kernelsu-family"
    ? "ReSukiSU 앱 → 설정에서 [모듈 마운트 해제 기본값]과 [Hide SELinux Modification]을 켠 뒤 계속을 눌러 주세요."
    // NeoZygisk는 Magisk에서 DenyList에 등록한 앱만 숨긴다(README, 2026-10-08 조사) — 적용(Enforce)은 끄고 목록에는 등록한다
    : "Magisk 앱 → 설정에서 [Zygisk]와 [DenyList 적용]은 끄고, [DenyList 설정]에서 은행·결제 앱(토스·은행 앱 등)과 Google Play 서비스를 체크한 뒤 계속을 눌러 주세요.";
}
/** TrickyAddon·HMA처럼 매니저 모듈 화면(WebUI)에서 직접 하는 설정 — 번호 단계로 안내한다(2026-10-09) */
export function trickyAddonInstruction(engine: RootState["engine"]): string {
  const app = managerAppName(engine);
  const open = engine === "kernelsu-family"
    ? `${app} 앱 → [모듈] 탭에서 TrickyStore 위젯(Tricky Addon)의 [열기](지구본/WebUI 아이콘)를 누릅니다.`
    : `${app} 앱 → 모듈 목록에서 TrickyStore 위젯(Tricky Addon)의 [열기](WebUI)를 누릅니다. WebUI가 없다는 안내가 나오면 그 화면에서 설치됩니다.`;
  return [
    open,
    "열린 설정 화면에서 [Target]을 눌러, 쓰시는 은행·결제 앱(토스·각 은행 앱 등)을 켭니다. Play 스토어·Play 서비스는 기본으로 켜져 있습니다.",
    "[Keybox]는 기본값이 들어가 있습니다 — 그대로 두고, Integrity가 통과되지 않을 때만 이 화면에서 다른 keybox로 바꿉니다.",
    `설정이 저장되면 ${app} 앱을 닫고 아래 [설정을 완료했습니다]를 누릅니다.`,
  ].join("\n");
}
export function hmaInstruction(engine: RootState["engine"], presetPath: string): string {
  const app = managerAppName(engine);
  return [
    `${app} 앱 → [모듈] 탭에서 Hide My Applist(HMA) 위젯의 [열기](WebUI)를 누릅니다.`,
    `[설정] → [가져오기]에서 폰의 다운로드 폴더에 있는 프리셋 파일(${presetPath.split("/").pop()})을 고릅니다.`,
    "가져온 뒤, 숨길 앱 목록에 쓰시는 은행·결제 앱이 들어 있는지 확인합니다.",
    `끝나면 ${app} 앱을 닫고 아래 [설정을 완료했습니다]를 누릅니다.`,
  ].join("\n");
}
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
  return { ...selection, sets: next, zygisk: "neozygisk", integrity: "play-integrity-fork", extras: [], settingsAck: false };
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
  // 예전 진행 기록에 남은 자유 선택값(zygisk·integrity·extras)은 쓰지 않는다 — 고정 프리셋만 설치
  const sets = selectedModuleSets(selection);
  return sets.flatMap(set => MODULE_PRESET[set].filter(m => !m.kernelsuOnly || engine === "kernelsu-family").map(m => m.id));
}
export interface ModuleSetPort {
  rootInspect(serial: string): Promise<ApiResult<RootState>>;
  rootModulesInspect(serial: string): Promise<ApiResult<RootModuleInventory>>;
  rootPackagePrepare(id: string): Promise<ApiResult<RootPackage>>;
  rootModuleInstall(serial: string, sha256: string, confirm: boolean, confirmExternal: boolean): Promise<ApiResult<RootModuleInventory>>;
  rootReboot(serial: string, target: "os"): Promise<ApiResult<unknown>>;
  rootPresetPush(serial: string): Promise<ApiResult<string>>;
  rootManagerSetup(serial: string): Promise<ApiResult<string[]>>;
  deviceWaitReady(serial: string): Promise<ApiResult<null>>;
  rootModuleRunAction(serial: string, moduleId: string): Promise<ApiResult<string>>;
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
  if (!serial) throw new Error("기기를 확인하세요");
  const root = requireResult(await port.rootInspect(serial)); hooks.check();
  if (root.access !== "granted") throw new Error("Shell 루트 권한을 허용하세요");
  const packages = moduleSetPackages(selection, root.engine);
  // 매니저 설정은 매니저가 깔린 지금 루트 셸로 맞춘다(2026-10-09 자동화) — 실패하면 폰에서 직접 하도록 안내
  if (packages.length) {
    const setup = await port.rootManagerSetup(serial); hooks.check();
    if (setup.ok) hooks.progress(`매니저 설정: ${setup.value.join(" · ")}`, 0, packages.length);
    else await hooks.instruction(`${managerSettingsInstruction(root.engine)} (자동 설정 실패: ${setup.error})`);
  }
  hooks.check();
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
    if (id === "play-integrity-fork") {
      // 매니저의 Action 버튼과 같은 스크립트(autopif)를 루트 셸로 실행한다 — 실패하면 폰에서 직접
      const action = await port.rootModuleRunAction(serial, appliedId ?? id); hooks.check();
      if (action.ok) hooks.progress(`PlayIntegrityFork Action 실행: ${action.value.split(/\r?\n/).pop() ?? ""}`, index, packages.length);
      else await hooks.instruction(`폰 매니저에서 PlayIntegrityFork Action(autopif)을 실행하고 설정을 확인하세요. (자동 실행 실패: ${action.error})`);
    }
    if (id === "tricky-addon") await hooks.instruction(trickyAddonInstruction(root.engine));
    if (id === "hma") {
      // 카페 프리셋을 폰 Download에 넣어 두고 가져오기만 안내한다(PC 저장 → 폰 이동 단계를 없앰, 2026-10-09)
      const pushed = await port.rootPresetPush(serial); hooks.check();
      await hooks.instruction(pushed.ok
        ? hmaInstruction(root.engine, pushed.value)
        : `HMA 프리셋을 폰에 넣지 못했습니다(${pushed.error}). 소니 카페 HMA JSON을 ${managerAppName(root.engine)} 앱의 HMA WebUI에서 직접 가져와 주세요.`);
    }
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
      // 새 부팅 ID만으로는 아직 부팅 중일 수 있다 — 공용 부팅 완료 대기를 거친 뒤 상태를 다시 읽는다(2026-10-09)
      requireResult(await port.deviceWaitReady(serial)); check();
      const ready = requireResult(await port.rootModulesInspect(serial)); check();
      if (ready.uncertain || ready.engine !== engine) throw new Error("재부팅 후 모듈·엔진 상태가 불확정입니다");
      return ready;
    }
    await new Promise(resolve => setTimeout(resolve, 1500));
  } while (Date.now() < until);
  throw new Error("재부팅 후 같은 폰의 루트 권한·모듈 적용을 확인하지 못했습니다. Shell 권한과 연결을 확인하세요");
}
