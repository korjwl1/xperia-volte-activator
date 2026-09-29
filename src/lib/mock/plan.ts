// 계획 생성 mock — plan.md §3-2 매트릭스 / §3-3 프로파일·의존성 규칙 구현
import type { PlanStep, Profile, StepKind, ManualId } from "$lib/types";

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

// 순정 복귀(기본) 전체 시드 — 매트릭스 1행 (잠김/신규)
const FULL: StepSeed[] = [
  { id: "backup-1", kind: "backup", title: "백업", desc: "사진·앱·설정을 PC에 저장합니다", optional: true, risk: "warn", estSec: 1800 },
  { id: "unlock", kind: "unlock", title: "부트로더 언락", desc: "기기 초기화가 발생합니다", risk: "danger", wipe: true, estSec: 120, manual: "mode-wait" },
  { id: "setup-min", kind: "setup", title: "기본 설정", desc: "재부팅 후 구글 계정 로그인 및 USB 디버깅 활성화", risk: "safe", estSec: 300, manual: "usb-debug" },
  { id: "root", kind: "root", title: "루팅", desc: "Magisk로 시스템 수정 권한 확보", risk: "warn", estSec: 600, manual: "magisk-patch" },
  { id: "efs-preflight", kind: "efs-preflight", title: "연결 안정성 검사", desc: "USB 포트·케이블 상태를 확인합니다", risk: "safe", estSec: 60 },
  { id: "efs", kind: "efs", title: "VoLTE 프로파일 적용", desc: "통신사 설정을 기기에 주입하고 검증합니다", risk: "danger", estSec: 420 },
  { id: "verify", kind: "verify", title: "적용 확인", desc: "주입된 파일의 무결성을 검증합니다", risk: "safe", estSec: 120 },
  { id: "unroot", kind: "unroot", title: "루팅 해제", desc: "시스템을 원래대로 되돌립니다 — 리락 전 필수", optional: true, risk: "warn", estSec: 180 },
  { id: "backup-2", kind: "backup2", title: "2차 백업", desc: "언락 이후 생성된 데이터를 저장합니다", optional: true, risk: "warn", estSec: 900 },
  { id: "relock", kind: "relock", title: "부트로더 리락", desc: "기기 초기화가 발생합니다", optional: true, risk: "danger", wipe: true, estSec: 120, manual: "mode-wait" },
  { id: "final-verify", kind: "final-verify", title: "최종 확인", desc: "재부팅 후 VoLTE 작동 여부를 확인합니다", risk: "safe", estSec: 300, manual: "ims-check" },
  { id: "restore", kind: "restore", title: "복구", desc: "백업한 데이터를 기기로 되돌립니다", optional: true, risk: "safe", estSec: 1500 },
];

function seedToStep(s: StepSeed, enabled: boolean): PlanStep {
  return {
    id: s.id, kind: s.kind, title: s.title, desc: s.desc,
    optional: s.optional ?? false, enabled, risk: s.risk ?? "safe",
    wipe: s.wipe ?? false, estSec: s.estSec, manual: s.manual,
  };
}

export function buildPlan(profile: Profile, overrides?: Record<string, boolean>): PlanStep[] {
  let defaults: Record<string, boolean>;
  switch (profile) {
    case "keep-root": // 루팅 유지: 백업→언락→루팅→EFS+검증→복구 (초기화 1회)
      defaults = { "backup-1": true, unroot: false, "backup-2": false, relock: false, restore: true };
      break;
    case "unroot-only": // 언루팅만: …→언루팅→복구 (초기화 1회)
      defaults = { "backup-1": true, unroot: true, "backup-2": false, relock: false, restore: true };
      break;
    default: // clean-return 순정 복귀 (초기화 2회)
      defaults = { "backup-1": true, unroot: true, "backup-2": true, relock: true, restore: true };
  }
  const merged = { ...defaults, ...overrides };
  // 의존성 규칙 (§3-3): 리락 ⟹ 언루팅 / 리락 ⟹ 2차 백업 노출 / 백업 없으면 복구 불가
  if (merged.relock) merged.unroot = true;
  if (!merged["backup-1"] && !merged["backup-2"]) merged.restore = false;
  return FULL.map((s) => seedToStep(s, s.optional ? !!merged[s.id] : true));
}

export const PROFILE_INFO: Record<Profile, { label: string; wipes: number; desc: string }> = {
  "clean-return": { label: "순정 복귀 (기본)", wipes: 2, desc: "백업→언락→루팅→EFS→언루팅→2차백업→리락→최종검증→복구" },
  "keep-root": { label: "루팅 유지", wipes: 1, desc: "백업→언락→루팅→EFS+검증→복구 — 루트 유지" },
  "unroot-only": { label: "언루팅만", wipes: 1, desc: "언락 유지·루트만 제거 — EFS→언루팅→복구" },
};
