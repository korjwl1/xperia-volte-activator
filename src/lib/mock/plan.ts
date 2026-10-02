// 실행 계획 생성 — 단일 공급원. "실행 순서" 미리보기와 실제 실행이 모두 이 결과를 쓴다.
// 순서/내용은 원본 CLI(cliInterface.py)의 언락 → 루팅 → EFS 업로드 → VoLTE 설정 → 언루팅 → 리락 흐름과
// 수동 가이드의 "수동 업데이트"(newflasher, .ta·userdata 제외)를 따른다.
import type { DeviceStatus, ManualId, PlanStep, VolteConfig } from "$lib/types";
import { CARRIER_LABEL } from "$lib/types";

export interface PlanOptions {
  unroot: boolean;
  relock: boolean;
  restore: boolean;
}

type Seed = Omit<PlanStep, "optional" | "enabled" | "risk" | "wipe"> &
  Partial<Pick<PlanStep, "risk" | "wipe">>;

/** 언락 사전 조건이 모두 켜져 있는지 (판별 불가는 미충족으로 보고 확인 단계를 둔다) */
export function prepReady(d: DeviceStatus): boolean {
  return d.prep.developerOptions === true && d.prep.usbDebugging === true && d.prep.oemUnlockAllowed === true;
}

/** 부트로더만 작업 — VoLTE 패치·업데이트가 없고 기기 상태가 맞을 때만 유효 */
export function bootloaderOnly(device: DeviceStatus | null, config: VolteConfig): "unlock" | "relock" | null {
  if (!device || config.sims.some((s) => s.carrier !== null) || updateTarget(device, config)) return null;
  if (config.bootloaderAction === "unlock" && device.bootloader === "locked") return "unlock";
  if (config.bootloaderAction === "relock" && device.bootloader === "unlocked") return "relock";
  return null;
}

/** 펌웨어 업데이트 대상 버전 (설치된 버전과 다를 때만) */
export function updateTarget(device: DeviceStatus | null, config: VolteConfig): string | null {
  return device && config.firmware && config.firmware !== device.firmware ? config.firmware : null;
}

export function buildPlan(
  device: DeviceStatus | null,
  config: VolteConfig,
  opts: PlanOptions,
  hasBackup: boolean,
): PlanStep[] {
  if (!device) return [];
  const only = bootloaderOnly(device, config);
  if (only) return finalize(bootloaderOnlyPlan(device, only, opts, hasBackup));
  const steps: Seed[] = [];
  const patch = config.sims.some((s) => s.carrier !== null); // VoLTE 패치 대상 슬롯이 있는지
  const update = updateTarget(device, config);
  const needsUnlock = patch && device.bootloader === "locked"; // 업데이트만이면 잠금 상태로도 순정 펌웨어 기록 가능
  const bootloaderKnown = device.bootloader === "locked" || device.bootloader === "unlocked";
  // 루팅: VoLTE 패치에 필요하거나, 업데이트로 풀리는 기존 루팅을 다시 살릴 때 (새 버전 이미지로)
  const needsRoot = (patch && (device.rooted !== true || update !== null)) || (update !== null && device.rooted === true);
  // 후처리(언루팅/리락)는 VoLTE 패치 흐름에서만, 부트로더 상태가 확인된 기기 (plan §3-2 매트릭스)
  const relock = patch && bootloaderKnown && opts.relock;
  const unroot = patch && bootloaderKnown && (opts.unroot || relock); // 리락 ⟹ 언루팅
  const wipes = needsUnlock || relock;

  // 사전 준비 — 백업(수 분) 전에 사용자 입력·폰 설정을 한 번에 받는다
  const prep: ManualId[] = [];
  if (needsUnlock && !prepReady(device)) prep.push("oem-toggle");
  if (needsUnlock) prep.push("unlock-code");
  if (needsRoot) prep.push("firmware-select"); // 업데이트 시에는 새 버전의 부트 이미지
  if (prep.length > 0) {
    steps.push({ id: "prep", kind: "setup", title: "사전 준비", desc: "언락 조건 확인 · 언락 코드 · 펌웨어 준비", estSec: 300, manual: prep });
  }
  if (update) {
    steps.push({ id: "fw-download", kind: "fw-download", title: "펌웨어 다운로드", desc: `${update} 전체 펌웨어를 받습니다`, estSec: 900 });
  }
  if (hasBackup) {
    steps.push({ id: "backup", kind: "backup", title: "백업", desc: "선택한 항목을 PC에 저장합니다", risk: "warn", estSec: 1800, manual: ["backup-notice"] });
  }
  if (update) {
    steps.push({
      id: "fw-flash",
      kind: "fw-flash",
      title: "펌웨어 업데이트",
      desc: patch
        ? "모뎀 포함 업데이트(사용자 데이터 유지) — 이후 VoLTE를 다시 적용합니다"
        : "모뎀·사용자 데이터를 제외하고 업데이트 — 기존 VoLTE 패치 유지",
      risk: "danger",
      estSec: 900,
      manual: ["flash-mode"],
    });
    steps.push({ id: "fw-verify", kind: "fw-verify", title: "업데이트 확인", desc: `기기 버전·지문이 ${update}와 일치하는지 확인`, estSec: 180 });
  }
  if (needsUnlock) {
    steps.push({ id: "unlock", kind: "unlock", title: "부트로더 언락", desc: "기기가 초기화됩니다", risk: "danger", wipe: true, estSec: 120, manual: ["mode-wait"] });
    steps.push({ id: "setup-min", kind: "setup", title: "기본 설정", desc: "재부팅 후 초기 설정 및 USB 디버깅 활성화", estSec: 300, manual: ["usb-debug"] });
  }
  if (needsRoot) {
    // 자동: Magisk 최신 APK → 패치 도구·부트 이미지 전송 → 폰에서 boot_patch.sh(셸 권한) → 결과 검증 → fastboot 기록 → Magisk 앱 설치
    //   (2026-10-03 실기기 검증: XQ-DQ44 init_boot, Magisk v30.7 — 사용자 조작 없음)
    steps.push({
      id: "root",
      kind: "root",
      title: update && !patch ? "루팅 다시 적용" : "루팅",
      desc: "Magisk로 시스템 수정 권한 확보",
      risk: "warn",
      estSec: 600,
    });
  }

  if (patch) {
    const targets = config.sims
      .filter((s) => s.carrier !== null)
      .map((s) => `SIM${s.slot}=${CARRIER_LABEL[s.carrier!]}`)
      .join(", ");
    steps.push({ id: "efs-preflight", kind: "efs-preflight", title: "연결 안정성 검사", desc: "USB 포트·케이블 상태 확인", estSec: 60 });
    steps.push({ id: "efs", kind: "efs", title: "VoLTE 적용", desc: `${targets} 프로파일을 주입합니다`, risk: "danger", estSec: 420, manual: ["su-grant"] });
    steps.push({ id: "verify", kind: "verify", title: "적용 확인", desc: "주입된 파일의 무결성 검증", estSec: 120 });
    steps.push({ id: "volte-props", kind: "volte-props", title: "VoLTE 활성화 설정", desc: "VoLTE·영상통화·Wi-Fi 통화 활성화 설정 후 재부팅", estSec: 120 });
  }

  if (unroot) {
    steps.push({ id: "unroot", kind: "unroot", title: "언루팅", desc: "순정 이미지로 복원합니다 — 리락 전 필수", risk: "warn", estSec: 180 });
  }
  if (relock) {
    steps.push({ id: "relock", kind: "relock", title: "부트로더 리락", desc: "기기가 초기화됩니다", risk: "danger", wipe: true, estSec: 120, manual: ["mode-wait"] });
    // 초기화 후 최종 확인·복구에 adb 연결이 필요
    steps.push(setupAfterWipe("setup-relock"));
  }
  if (patch ? bootloaderKnown : update !== null) {
    steps.push({ id: "final-verify", kind: "final-verify", title: "최종 확인", desc: "재부팅 후 VoLTE 작동 여부 확인", estSec: 300, manual: ["ims-check"] });
  }
  // 복구는 초기화가 실제로 일어나는 경우에만 (초기화 없이 복원하면 기존 데이터에 덮어씀)
  if (wipes && opts.restore && hasBackup) {
    steps.push({ id: "restore", kind: "restore", title: "복구", desc: "백업한 데이터를 기기로 복원", estSec: 1500 });
  }

  return finalize(steps);
}

function finalize(steps: Seed[]): PlanStep[] {
  return steps.map((s) => ({ ...s, optional: false, enabled: true, risk: s.risk ?? "safe", wipe: s.wipe ?? false }));
}

function setupAfterWipe(id: string): Seed {
  return { id, kind: "setup", title: "기본 설정", desc: "재부팅 후 초기 설정 및 USB 디버깅 활성화", estSec: 300, manual: ["usb-debug"] };
}

/** 부트로더 언락만 / 리락만 — VoLTE 패치·펌웨어 업데이트 없음 */
function bootloaderOnlyPlan(device: DeviceStatus, only: "unlock" | "relock", opts: PlanOptions, hasBackup: boolean): Seed[] {
  const steps: Seed[] = [];
  // 리락 전 순정 이미지 복원 — 루팅 여부를 모르면 안전하게 포함 (수정된 부트 이미지로 리락하면 부팅 불가)
  const unroot = only === "relock" && device.rooted !== false;
  const prep: ManualId[] = [];
  if (only === "unlock" && !prepReady(device)) prep.push("oem-toggle");
  if (only === "unlock") prep.push("unlock-code");
  if (unroot) prep.push("firmware-select");
  if (prep.length > 0) {
    steps.push({
      id: "prep",
      kind: "setup",
      title: "사전 준비",
      desc: only === "unlock" ? "언락 조건 확인 · 언락 코드" : "언루팅용 순정 펌웨어 준비",
      estSec: 300,
      manual: prep,
    });
  }
  if (hasBackup) {
    steps.push({ id: "backup", kind: "backup", title: "백업", desc: "선택한 항목을 PC에 저장합니다", risk: "warn", estSec: 1800, manual: ["backup-notice"] });
  }
  if (only === "unlock") {
    steps.push({ id: "unlock", kind: "unlock", title: "부트로더 언락", desc: "기기가 초기화됩니다", risk: "danger", wipe: true, estSec: 120, manual: ["mode-wait"] });
    steps.push(setupAfterWipe("setup-min"));
  } else {
    if (unroot) {
      steps.push({ id: "unroot", kind: "unroot", title: "언루팅", desc: "순정 이미지로 복원합니다 — 리락 전 필수", risk: "warn", estSec: 180 });
    }
    steps.push({ id: "relock", kind: "relock", title: "부트로더 리락", desc: "기기가 초기화됩니다 — VoLTE 패치는 유지됩니다", risk: "danger", wipe: true, estSec: 120, manual: ["mode-wait"] });
    steps.push(setupAfterWipe("setup-relock"));
  }
  if (opts.restore && hasBackup) {
    steps.push({ id: "restore", kind: "restore", title: "복구", desc: "백업한 데이터를 기기로 복원", estSec: 1500 });
  }
  return steps;
}
