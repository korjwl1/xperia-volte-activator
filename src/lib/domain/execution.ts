/** A live run must never finish device actions through the simulation runner. */
export interface ExecutionFlags {
  backup: boolean;
  restore: boolean;
  fastboot: boolean;
  /** 리락(초기화 동반)은 언락과 따로 켠다 — 실기기 검증 전에는 끈다(2026-10-08 사용자 결정) */
  relock: boolean;
  root: boolean;
  /** 루팅 도구(엔진 전환·모듈 설치) — 실기기 검증 전이라 루팅과 따로 끈다(2026-10-08) */
  rootTools: boolean;
  verify: boolean;
  efs: boolean;
}

export const hasLiveActions = (flags: ExecutionFlags): boolean => Object.values(flags).some(Boolean);

export function canReboot(flags: ExecutionFlags, target: "os" | "bootloader" | "fastboot"): boolean {
  return flags.root || flags.fastboot || (target === "os" && (flags.verify || flags.efs));
}

export function liveStepEnabled(id: string, flags: ExecutionFlags): boolean {
  switch (id) {
    case "backup": return flags.backup;
    case "restore": return flags.restore;
    case "unlock": return flags.fastboot;
    case "relock": return flags.fastboot && flags.relock;
    case "root": case "unroot": return flags.root && flags.fastboot;
    case "root-modules": return flags.rootTools;
    case "efs-input": case "efs-preflight": case "efs": case "verify":
    case "volte-props": case "comm-check": return flags.efs;
    case "fw-verify": return flags.verify;
    case "final-verify": return flags.verify || flags.efs;
    case "fw-flash": return false;
    case "prep": case "setup": case "setup-unlock": case "setup-relock": case "setup-min": return true;
    default: return false;
  }
}

export interface EngineCapabilities { fastbootWrite: boolean; rootWrite: boolean; efsWrite: boolean }
export function buildFeatureProblem(ids: readonly string[], features: EngineCapabilities): string | null {
  if (ids.some(id => ["unlock", "relock", "root", "unroot"].includes(id)) && !features.fastbootWrite) return "이 빌드에는 fastboot-write 기능이 없습니다";
  if (ids.some(id => ["root", "unroot"].includes(id)) && !features.rootWrite) return "이 빌드에는 root-write 기능이 없습니다";
  if (ids.some(id => ["efs-input", "efs-preflight", "efs", "verify", "volte-props", "comm-check"].includes(id)) && !features.efsWrite) return "이 빌드에는 efs-write 기능이 없습니다";
  if (ids.includes("final-verify") && !Object.values(features).some(Boolean)) return "이 빌드에는 최종 OS 재부팅 기능이 없습니다";
  return null;
}

export function executionPlanProblem(ids: readonly string[], flags: ExecutionFlags): string | null {
  if (ids.includes("root-modules") && !flags.rootTools) return "루팅 모듈 설치 실전 기능이 비활성화되어 있습니다";
  if (!hasLiveActions(flags)) return null;
  if (ids.includes("fw-flash")) return "전체 펌웨어 기록이 아직 구현되지 않아 이 실전 계획을 시작할 수 없습니다";
  for (const id of ids) {
    const ready = liveStepEnabled(id, flags);
    if (!ready) return `실전 계획의 ${id} 단계를 시뮬레이션으로 진행할 수 없습니다 — 해당 실전 엔진을 함께 활성화해야 합니다`;
  }
  return null;
}
