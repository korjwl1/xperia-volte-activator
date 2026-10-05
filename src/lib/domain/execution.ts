/** A live run must never finish device actions through the simulation runner. */
export interface ExecutionFlags {
  backup: boolean;
  restore: boolean;
  fastboot: boolean;
  root: boolean;
  verify: boolean;
  efs: boolean;
}

export const hasLiveActions = (flags: ExecutionFlags): boolean => Object.values(flags).some(Boolean);

export function canReboot(flags: ExecutionFlags, target: "os" | "bootloader"): boolean {
  return flags.root || flags.fastboot || (target === "os" && (flags.verify || flags.efs));
}

export function liveStepEnabled(id: string, flags: ExecutionFlags): boolean {
  switch (id) {
    case "backup": return flags.backup;
    case "restore": return flags.restore;
    case "unlock": case "relock": return flags.fastboot;
    case "root": case "unroot": return flags.root && flags.fastboot;
    case "efs-input": case "efs-preflight": case "efs": case "verify":
    case "volte-props": case "comm-check": return flags.efs;
    case "fw-verify": return flags.verify;
    case "final-verify": return flags.verify || flags.efs;
    case "fw-flash": return false;
    default: return true;
  }
}

export function executionPlanProblem(ids: readonly string[], flags: ExecutionFlags): string | null {
  if (!hasLiveActions(flags)) return null;
  if (ids.includes("fw-flash")) return "전체 펌웨어 기록이 아직 구현되지 않아 이 실전 계획을 시작할 수 없습니다";
  for (const id of ids) {
    const ready = liveStepEnabled(id, flags);
    if (!ready) return `실전 계획의 ${id} 단계를 시뮬레이션으로 진행할 수 없습니다 — 해당 실전 엔진을 함께 활성화해야 합니다`;
  }
  return null;
}
