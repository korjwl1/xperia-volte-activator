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
  { id: "backup-1", kind: "backup", title: "1차 백업", desc: "선택 항목 백업 — 완결(전수 열거+해시) 시 파괴 단계 활성", optional: true, risk: "warn", estSec: 1800 },
  { id: "unlock", kind: "unlock", title: "부트로더 언락", desc: "fastboot oem unlock 0x{코드} — ★ 초기화 #1 발생", risk: "danger", wipe: true, estSec: 120, manual: "mode-wait" },
  { id: "setup-min", kind: "setup", title: "최소 세팅", desc: "구글 계정 로그인 + 개발자 옵션 + USB 디버깅 (안내)", risk: "safe", estSec: 300, manual: "usb-debug" },
  { id: "root", kind: "root", title: "루팅 (init_boot 패치)", desc: "SIN 추출 → Magisk 패치 → fastboot flash (신규 산출물만)", risk: "warn", estSec: 600, manual: "magisk-patch" },
  { id: "efs-preflight", kind: "efs-preflight", title: "USB/DIAG 프리플라이트", desc: "토폴로지·컨트롤러·전원·드라이버·DIAG 건전성 (§10-5)", risk: "safe", estSec: 60 },
  { id: "efs", kind: "efs", title: "EFS 주입 + 전수 검증", desc: "before-image 스냅샷 → 업로드 → 리드백 해시 비교 (SKT/SIM2)", risk: "danger", estSec: 420 },
  { id: "verify", kind: "verify", title: "주입 직후 검증", desc: "전수 리드백 sha256 — 불일치 파일 자동 재전송", risk: "safe", estSec: 120 },
  { id: "unroot", kind: "unroot", title: "언루팅", desc: "검증된 순정 이미지로 플래시 — 리락의 전제", optional: true, risk: "warn", estSec: 180 },
  { id: "backup-2", kind: "backup2", title: "2차 백업", desc: "언락 이후 신규 데이터 — 리락(초기화 #2) 직전", optional: true, risk: "warn", estSec: 900 },
  { id: "relock", kind: "relock", title: "부트로더 리락", desc: "언루팅 완료 + 순정 이미지 근거 확인 후에만 허용 — ★ 초기화 #2", optional: true, risk: "danger", wipe: true, estSec: 120, manual: "mode-wait" },
  { id: "final-verify", kind: "final-verify", title: "최종 검증 (IMS)", desc: "리락 후 부팅 — persist 프롭 소실 확인·모뎀 자체 IMS 등록 확인 (§4)", risk: "safe", estSec: 300, manual: "ims-check" },
  { id: "restore", kind: "restore", title: "복구", desc: "최신 백업 소스(2차 우선)에서 앱·파일·설정 복원", optional: true, risk: "safe", estSec: 1500 },
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
