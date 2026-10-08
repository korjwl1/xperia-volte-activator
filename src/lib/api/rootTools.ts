import type { ApiResult, RootState, RootUpdateRequest, RootUpdatePlan, RootPackage, RootRelease, RootModuleInventory, RootSwitch, RootImportedImage } from "$lib/types";
import { transport } from "./transport";
import { REAL_STEPS } from "$lib/data/runMode";

const rootWrite = <T>(command: string, args: Record<string, unknown>): Promise<ApiResult<T>> => {
  if (!REAL_STEPS.root) return Promise.resolve({ ok: false, error: "루팅 실전 실행이 비활성화되어 있습니다" });
  return transport.result<T>(command, args);
};
export const rootToolsApi = {
  rootInspect: (serial: string) => transport.result<RootState>("root_inspect", { serial }),
  firmwareUpdateRootPlan: (request: RootUpdateRequest) => transport.result<RootUpdatePlan>("firmware_update_root_plan", { request }),
  rootToolsCapabilities: () => transport.result<{ writeEnabled: boolean; switchEnabled: boolean }>("root_tools_capabilities", {}),
  resukisuReleases: () => transport.result<RootRelease[]>("resukisu_releases", {}),
  rootPackagePrepare: (id: string, tag?: string) => transport.result<RootPackage>("root_package_prepare", { id, tag: tag ?? null }),
  rootPresetExport: (dest: string) => transport.result<string>("root_preset_export", { dest }),
  rootModulesInspect: (serial: string) => transport.result<RootModuleInventory>("root_modules_inspect", { serial }),
  rootModuleInstall: (serial: string, sha256: string, confirm: boolean, confirmExternal: boolean) => rootWrite<RootModuleInventory>("root_module_install", { serial, sha256, confirm, confirmExternal }),
  rootModuleAction: (serial: string, moduleId: string, action: "disable" | "remove", confirm: boolean) => rootWrite<null>("root_module_action", { serial, moduleId, action, confirm }),
  rootModuleReconcile: (serial: string, confirm: boolean) => transport.result<RootModuleInventory>("root_module_reconcile", { serial, confirm }),
  rootSwitchPrepare: (serial: string, stockPath: string, target: "magisk" | "resukisu", confirm: boolean) => {
    if (!REAL_STEPS.fastboot) return Promise.resolve<ApiResult<RootSwitch>>({ ok: false, error: "fastboot 실전 실행이 비활성화되어 있습니다" });
    return rootWrite<RootSwitch>("root_switch_prepare", { serial, stockPath, target, confirm });
  },
  rootSwitchStatus: (serial: string, verifyStock = false) => transport.result<RootSwitch>("root_switch_status", { serial, verifyStock }),
  rootExternalPatchImport: (serial: string, stockPath: string, patchedPath: string, confirmSamePhone: boolean) => rootWrite<RootImportedImage>("root_external_patch_import", { serial, stockPath, patchedPath, confirmSamePhone }),
  resukisuInstall: (serial: string, sha256: string, confirm: boolean) => rootWrite<null>("resukisu_install", { serial, sha256, confirm }),
  rootSwitchFinish: (serial: string) => transport.result<RootSwitch>("root_switch_finish", { serial }),
};
export type RootToolsApi = typeof rootToolsApi;
