// 계획 생성 — 기기 상태 + VoLTE 설정 기반 자동 생성 (프리셋 폐지)
import type { PlanStep, StepKind, ManualId, DeviceStatus, VolteConfig } from "$lib/types";

interface StepSeed {
  id: string;
  kind: StepKind;
  title: string;
  desc: string;
  optional?: boolean;
  risk?: PlanStep["risk"];
  wipe?: boolean;
  estSec: number;
  manual?: ManualId;
}

function seedToStep(s: StepSeed, enabled: boolean): PlanStep {
  return {
    id: s.id, kind: s.kind, title: s.title, desc: s.desc,
    optional: s.optional ?? false, enabled, risk: s.risk ?? "safe",
    wipe: s.wipe ?? false, estSec: s.estSec, manual: s.manual,
  };
}

export function buildPlan(device: DeviceStatus | null, config: VolteConfig, overrides?: Record<string, boolean>): PlanStep[] {
  if (!device) return [];

  const steps: StepSeed[] = [];
  const needsUnlock = device.bootloader === "locked";

  // 1. 언락이 필요한 경우 백업 먼저
  if (needsUnlock) {
    steps.push({ id: "backup-1", kind: "backup", title: "백업", desc: "사진·앱·설정을 PC에 저장합니다", optional: true, risk: "warn", estSec: 1800 });
    steps.push({ id: "dev-options", kind: "setup", title: "개발자 옵션 준비", desc: "OEM 잠금 해제와 USB 디버깅 활성화", risk: "safe", estSec: 120, manual: "oem-toggle" });
    steps.push({ id: "unlock", kind: "unlock", title: "부트로더 언락", desc: "기기가 초기화됩니다", risk: "danger", wipe: true, estSec: 120, manual: "mode-wait" });
    steps.push({ id: "setup-min", kind: "setup", title: "기본 설정", desc: "재부팅 후 구글 계정 로그인 및 USB 디버깅 활성화", risk: "safe", estSec: 300, manual: "usb-debug" });
  }

  // 2. 루팅 (이미 루팅돼 있으면 생략)
  if (device.rooted !== true) {
    steps.push({ id: "root", kind: "root", title: "루팅", desc: "Magisk로 시스템 수정 권한 확보", risk: "warn", estSec: 600, manual: "magisk-patch" });
  }

  // 3. VoLTE 적용 (공통)
  steps.push({ id: "efs-preflight", kind: "efs-preflight", title: "연결 안정성 검사", desc: "USB 포트·케이블 상태 확인", risk: "safe", estSec: 60 });
  steps.push({ id: "efs", kind: "efs", title: "VoLTE 적용", desc: `${config.carrier} 프로파일을 SIM${config.simSlot}에 주입합니다`, risk: "danger", estSec: 420 });
  steps.push({ id: "verify", kind: "verify", title: "적용 확인", desc: "주입된 파일의 무결성 검증", risk: "safe", estSec: 120 });

  // 4. 후처리 — 사용자 선택
  if (needsUnlock) {
    steps.push({ id: "unroot", kind: "unroot", title: "루팅 해제", desc: "시스템을 원래대로 되돌립니다 — 리락 전 필수", optional: true, risk: "warn", estSec: 180 });
    steps.push({ id: "relock", kind: "relock", title: "부트로더 리락", desc: "기기가 초기화됩니다", optional: true, risk: "danger", wipe: true, estSec: 120, manual: "mode-wait" });
    steps.push({ id: "final-verify", kind: "final-verify", title: "최종 확인", desc: "재부팅 후 VoLTE 작동 여부 확인", risk: "safe", estSec: 300, manual: "ims-check" });
    steps.push({ id: "restore", kind: "restore", title: "복구", desc: "백업한 데이터를 기기로 복원", optional: true, risk: "safe", estSec: 1500 });
  }

  // 기본값: 선택 단계는 전부 ON
  const defaults: Record<string, boolean> = {};
  for (const s of steps) if (s.optional) defaults[s.id] = true;

  const merged = { ...defaults, ...overrides };
  // 의존성: 리락 ⟹ 언루팅
  if (merged.relock) merged.unroot = true;
  // 언락 후속 단계가 있으면 복구 가능
  if (needsUnlock && !merged["backup-1"]) merged.restore = false;

  return steps.map((s) => seedToStep(s, s.optional ? !!merged[s.id] : true));
}
