// 실행 계획 생성 — 단일 공급원. "실행 순서" 미리보기와 실제 실행이 모두 이 결과를 쓴다.
// 순서/내용은 원본 CLI(cliInterface.py)의 언락 → 루팅 → EFS 업로드 → VoLTE 설정 → 언루팅 → 리락 흐름을 따른다.
import type { DeviceStatus, PlanStep, VolteConfig } from "$lib/types";
import { CARRIER_LABEL } from "$lib/types";

export interface PlanOptions {
  unroot: boolean;
  relock: boolean;
  restore: boolean;
}

type Seed = Omit<PlanStep, "optional" | "enabled" | "risk" | "wipe"> &
  Partial<Pick<PlanStep, "risk" | "wipe">>;

export function buildPlan(
  device: DeviceStatus | null,
  config: VolteConfig,
  opts: PlanOptions,
  hasBackup: boolean,
): PlanStep[] {
  if (!device) return [];
  const steps: Seed[] = [];
  const needsUnlock = device.bootloader === "locked";
  // 후처리(언루팅/리락)는 부트로더 상태가 확인된 기기에서만 (plan §3-2 매트릭스)
  const bootloaderKnown = device.bootloader === "locked" || device.bootloader === "unlocked";
  const relock = bootloaderKnown && opts.relock;
  const unroot = bootloaderKnown && (opts.unroot || relock); // 리락 ⟹ 언루팅
  const wipes = needsUnlock || relock;

  if (hasBackup) {
    steps.push({ id: "backup", kind: "backup", title: "백업", desc: "선택한 항목을 PC에 저장합니다", risk: "warn", estSec: 1800, manual: ["backup-notice"] });
  }
  if (needsUnlock) {
    steps.push({ id: "dev-options", kind: "setup", title: "개발자 옵션 준비", desc: "OEM 잠금 해제와 USB 디버깅 활성화", estSec: 120, manual: ["oem-toggle"] });
    steps.push({ id: "unlock", kind: "unlock", title: "부트로더 언락", desc: "기기가 초기화됩니다", risk: "danger", wipe: true, estSec: 120, manual: ["unlock-code", "mode-wait"] });
    steps.push({ id: "setup-min", kind: "setup", title: "기본 설정", desc: "재부팅 후 초기 설정 및 USB 디버깅 활성화", estSec: 300, manual: ["usb-debug"] });
  }
  if (device.rooted !== true) {
    // 펌웨어 지정 → (Magisk 앱 설치·이미지 전송은 자동) → 폰에서 패치 → 플래시
    steps.push({ id: "root", kind: "root", title: "루팅", desc: "Magisk로 시스템 수정 권한 확보", risk: "warn", estSec: 600, manual: ["firmware-select", "magisk-patch"] });
  }

  const targets = config.sims
    .filter((s) => s.carrier !== null)
    .map((s) => `SIM${s.slot}=${CARRIER_LABEL[s.carrier!]}`)
    .join(", ");
  steps.push({ id: "efs-preflight", kind: "efs-preflight", title: "연결 안정성 검사", desc: "USB 포트·케이블 상태 확인", estSec: 60 });
  steps.push({ id: "efs", kind: "efs", title: "VoLTE 적용", desc: `${targets} 프로파일을 주입합니다`, risk: "danger", estSec: 420, manual: ["su-grant"] });
  steps.push({ id: "verify", kind: "verify", title: "적용 확인", desc: "주입된 파일의 무결성 검증", estSec: 120 });
  steps.push({ id: "volte-props", kind: "volte-props", title: "VoLTE 활성화 설정", desc: "VoLTE·영상통화·Wi-Fi 통화 활성화 설정 후 재부팅", estSec: 120 });

  if (unroot) {
    steps.push({ id: "unroot", kind: "unroot", title: "언루팅", desc: "순정 이미지로 복원합니다 — 리락 전 필수", risk: "warn", estSec: 180 });
  }
  if (relock) {
    steps.push({ id: "relock", kind: "relock", title: "부트로더 리락", desc: "기기가 초기화됩니다", risk: "danger", wipe: true, estSec: 120, manual: ["mode-wait"] });
  }
  if (bootloaderKnown) {
    steps.push({ id: "final-verify", kind: "final-verify", title: "최종 확인", desc: "재부팅 후 VoLTE 작동 여부 확인", estSec: 300, manual: ["ims-check"] });
  }
  // 복구는 초기화가 실제로 일어나는 경우에만 (초기화 없이 복원하면 기존 데이터에 덮어씀)
  if (wipes && opts.restore && hasBackup) {
    steps.push({ id: "restore", kind: "restore", title: "복구", desc: "백업한 데이터를 기기로 복원", estSec: 1500 });
  }

  return steps.map((s) => ({ ...s, optional: false, enabled: true, risk: s.risk ?? "safe", wipe: s.wipe ?? false }));
}
