/** A live run must never finish device actions through the simulation runner. */
export interface ExecutionFlags {
  backup: boolean;
  restore: boolean;
  fastboot: boolean;
  root: boolean;
  verify: boolean;
  efs: boolean;
}

export function executionPlanProblem(ids: readonly string[], flags: ExecutionFlags): string | null {
  if (!Object.values(flags).some(Boolean)) return null;
  if (ids.includes("fw-flash")) return "전체 펌웨어 기록이 아직 구현되지 않아 이 실전 계획을 시작할 수 없습니다";
  for (const id of ids) {
    let ready = true;
    switch (id) {
      case "backup": ready = flags.backup; break;
      case "restore": ready = flags.restore; break;
      case "unlock": ready = flags.fastboot; break;
      case "relock": ready = flags.fastboot; break;
      case "root":
      case "unroot": ready = flags.root && flags.fastboot; break;
      case "efs-input":
      case "efs-preflight":
      case "efs":
      case "verify":
      case "volte-props":
      case "comm-check": ready = flags.efs; break;
      case "fw-verify": ready = flags.verify; break;
      case "final-verify": ready = flags.verify || flags.efs; break;
    }
    if (!ready) return `실전 계획의 ${id} 단계를 시뮬레이션으로 진행할 수 없습니다 — 해당 실전 엔진을 함께 활성화해야 합니다`;
  }
  return null;
}
