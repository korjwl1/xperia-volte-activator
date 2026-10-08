// 실행 계획 생성 — 단일 공급원. "실행 순서" 미리보기와 실제 실행이 모두 이 결과를 쓴다.
// 순서/내용은 원본 CLI(cliInterface.py)의 언락 → 루팅 → EFS 업로드 → VoLTE 설정 → 언루팅 → 리락 흐름과
// 수동 가이드의 "수동 업데이트"(newflasher, .ta·userdata 제외)를 따른다.
import type { DeviceStatus, ManualId, ManualTask, PlanStep, VolteConfig, WorkflowMode, RootModuleSelection } from "$lib/types";
import { CARRIER_LABEL } from "$lib/types";
import { deviceWorkflow } from "$lib/data/devices";
import { manualTaskProblem, updateProblem } from "$lib/domain/workflow";
import { automaticModulesProblem, selectedModuleSets } from "$lib/domain/moduleSets";

export interface PlanOptions {
  mode?: WorkflowMode;
  manualTask?: ManualTask;
  modules?: RootModuleSelection;
  backupOnly?: boolean;
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
  if (opts.mode === "manual") return manualPlan(device, config, opts, hasBackup);
  if (opts.mode === "update") {
    const version = updateTarget(device, config);
    if (updateProblem(device) || !version) return [];
    return finalize([
      ...(hasBackup ? [backupStep()] : []),
      { id: "fw-download", kind: "fw-download", title: "업데이트 펌웨어 준비", desc: `${version} 전체 펌웨어와 보존 정책 확인`, estSec: 900 },
      { id: "fw-flash", kind: "fw-flash", title: "펌웨어 업데이트", desc: "Newflasher 기반 기록 · 모뎀·DSP·TA·사용자 데이터 제외 · 잠금 상태 유지", risk: "danger", estSec: 900, manual: ["flash-mode"] },
      { id: "fw-verify", kind: "fw-verify", title: "업데이트 확인", desc: "목표 버전·지문·루트 정책 확인", estSec: 180 },
      { id: "final-verify", kind: "final-verify", title: "통신 확인", desc: "기존 VoLTE 등록과 실제 통화 확인", estSec: 300, manual: ["ims-check"] },
    ]);
  }
  // Firmware updates and standalone bootloader operations belong to their own routes.
  if (opts.mode === "automatic") config = { ...config, firmware: null, bootloaderAction: null };
  if (opts.backupOnly) return hasBackup ? finalize([{ id: "backup", kind: "backup", title: "백업", desc: "선택한 항목과 원본 속성을 PC에 보존합니다", risk: "warn", estSec: 1800, manual: ["backup-notice"] }]) : [];
  const only = bootloaderOnly(device, config);
  if (only) return finalize(bootloaderOnlyPlan(device, only, opts, hasBackup));
  const steps: Seed[] = [];
  const patch = config.sims.some((s) => s.carrier !== null); // VoLTE 패치 대상 슬롯이 있는지
  if (patch) {
    steps.push({ id: "efs-input", kind: "setup", title: "VoLTE 프리셋 확인", desc: "설정·프리셋 해시·슬롯 간 충돌을 기기 작업 전에 확인", estSec: 5 });
  }
  const update = updateTarget(device, config);
  const needsUnlock = patch && device.bootloader === "locked"; // 업데이트만이면 잠금 상태로도 순정 펌웨어 기록 가능
  const bootloaderKnown = device.bootloader === "locked" || device.bootloader === "unlocked";
  // 루팅: VoLTE 패치에 필요하거나, 업데이트로 풀리는 기존 루팅을 다시 살릴 때 (새 버전 이미지로)
  const needsRoot = (patch && (device.rooted !== true || update !== null)) || (update !== null && device.rooted === true);
  // 후처리(언루팅/리락)는 VoLTE 패치 흐름에서만, 부트로더 상태가 확인된 기기 (plan §3-2 매트릭스)
  const relock = patch && bootloaderKnown && opts.relock;
  const unroot = patch && bootloaderKnown && (opts.unroot || relock); // 리락 ⟹ 언루팅
  const workflow = deviceWorkflow(device.model, config.sims.flatMap(s => s.carrier ? [s.carrier] : []), relock);
  const wipes = needsUnlock || relock;

  // 사전 준비 — 백업(수 분) 전에 사용자 입력·폰 설정을 한 번에 받는다
  const prep: ManualId[] = [];
  if (needsUnlock && !prepReady(device)) prep.push("oem-toggle");
  if (needsUnlock) prep.push("unlock-code");
  // 루팅(새로 또는 다시)·언루팅 모두 순정 부트 이미지가 필요 — 업데이트 시에는 새 버전의 이미지
  if (needsRoot || unroot) prep.push("firmware-select");
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
      // 플래시 허용 목록 정책: 패치 유지 = 모뎀·DSP·TA·사용자 데이터 제외 / 재패치 = 모뎀·DSP 포함, TA·사용자 데이터 제외
      desc: patch
        ? "모뎀·DSP 포함 업데이트(TA·사용자 데이터 제외) — 이후 VoLTE를 다시 적용합니다"
        : "모뎀·DSP·TA·사용자 데이터를 제외하고 업데이트 — 기존 VoLTE 패치 유지",
      risk: "danger",
      estSec: 900,
      manual: ["flash-mode"],
    });
    steps.push({ id: "fw-verify", kind: "fw-verify", title: "업데이트 확인", desc: `기기 버전·지문이 ${update}와 일치하는지 확인`, estSec: 180 });
  }
  if (needsUnlock) {
    steps.push({ id: "unlock", kind: "unlock", title: "부트로더 언락", desc: "기기가 초기화됩니다", risk: "danger", wipe: true, estSec: 120, manual: ["mode-wait"] });
    steps.push({ id: "setup-min", kind: "setup", title: "기본 설정", desc: "초기화 후 폰 초기 설정 · 개발자 옵션 · USB 디버깅", estSec: 300, manual: ["usb-debug"] });
  }
  if (needsRoot) {
    // 자동: Magisk 최신 APK → 패치 도구·부트 이미지 전송 → 폰에서 boot_patch.sh(셸 권한) → 결과 검증 → fastboot 기록 → Magisk 앱 설치
    //   (2026-10-03 실기기 검증: XQ-DQ44 init_boot, Magisk v30.7 — 사용자 조작 없음)
    steps.push({
      id: "root",
      kind: "root",
      title: update && !patch ? "루팅 다시 적용" : "루팅",
      desc: `${workflow.partition ?? "부트"} 이미지를 Magisk로 패치해 시스템 수정 권한 확보`,
      risk: "warn",
      estSec: 600,
    });
  }

  if (patch) {
    const targets = config.sims
      .filter((s) => s.carrier !== null)
      .map((s) => `SIM${s.slot}=${CARRIER_LABEL[s.carrier!]}`)
      .join(", ");
    steps.push({ id: "efs-preflight", kind: "efs-preflight", title: "EFS 연결 확인", desc: workflow.diagEngineering ? "루트 권한 승인 · Mark IV 개발 포트 설정 · DIAG 연결 확인" : "루트 권한 승인 · DIAG 연결 및 프로토콜 응답 확인", estSec: 60, manual: ["su-grant"] });
    // 원본 beta11 계승: 슬롯별 두 번 업로드 → 전수 리드백. 두 번 썼다는 것만으로 성공 판정하지 않는다
    steps.push({ id: "efs", kind: "efs", title: "VoLTE 적용", desc: `${targets} 프로파일을 슬롯별로 두 번 주입합니다${workflow.manualPdc ? " — PDC 고정은 별도 수동 작업입니다" : ""}`, risk: "danger", estSec: 420 });
    steps.push({ id: "verify", kind: "verify", title: "적용 확인", desc: "주입한 파일 전수 리드백·해시 비교 — 누락·불일치는 실패", estSec: 120 });
    steps.push({ id: "volte-props", kind: "volte-props", title: "VoLTE 활성화 설정", desc: "VoLTE·영상통화·Wi-Fi 통화 설정을 켜고 재부팅 (통신사 서비스 검증은 아님)", estSec: 120 });
    // 언루팅·리락 전에 실제 통신 확인 — 리락 뒤 문제가 있으면 다시 고치려면 초기화가 한 번 더 필요
    if (unroot || relock) {
      steps.push({ id: "comm-check", kind: "final-verify", title: "통신 확인", desc: "언루팅·리락 전에 VoLTE 등록과 실제 발신·수신을 확인", estSec: 300, manual: ["ims-precheck"] });
    }
  }

  if (opts.mode === "automatic" && !automaticModulesProblem(device, config, opts) && selectedModuleSets(opts.modules).length) {
    steps.push({ id: "root-modules", kind: "root-modules", title: "루팅 모듈 세트 설치", desc: "의존 세트 포함 · 엔진별 설치 순서 · 재부팅과 적용 확인", risk: "warn", estSec: 900 });
  }
  if (unroot) {
    steps.push({ id: "unroot", kind: "unroot", title: "언루팅", desc: `순정 ${workflow.partition ?? "부트"} 이미지로 복원합니다 — 리락 전 필수`, risk: "warn", estSec: 180 });
  }
  if (relock) {
    steps.push({ id: "relock", kind: "relock", title: "부트로더 리락", desc: "순정 복원·현재 펌웨어 확인 후 리락 — 기기가 초기화됩니다", risk: "danger", wipe: true, estSec: 120 });
    // 초기화 후 최종 확인·복구에 adb 연결이 필요
    steps.push(setupAfterWipe("setup-relock"));
  }
  // 복구는 초기화가 실제로 일어나는 경우에만 (초기화 없이 복원하면 기존 데이터에 덮어씀)
  if (wipes && opts.restore && hasBackup) {
    steps.push({ id: "restore", kind: "restore", title: "복구", desc: "백업한 데이터를 기기로 복원", estSec: 1500 });
  }
  // 최종 확인은 리락·복구까지 끝난 뒤
  if (patch || update !== null) {
    steps.push({ id: "final-verify", kind: "final-verify", title: "최종 확인", desc: "재부팅 후 VoLTE 작동 여부 확인", estSec: 300, manual: ["ims-check"] });
  }

  return finalize(steps);
}

function finalize(steps: Seed[]): PlanStep[] {
  return steps.map((s) => ({ ...s, optional: false, enabled: true, risk: s.risk ?? "safe", wipe: s.wipe ?? false }));
}

function backupStep(): Seed {
  return { id: "backup", kind: "backup", title: "백업", desc: "선택한 데이터를 PC에 저장합니다", risk: "warn", estSec: 1800, manual: ["backup-notice"] };
}

function manualPlan(device: DeviceStatus, config: VolteConfig, opts: PlanOptions, hasBackup: boolean): PlanStep[] {
  const task = opts.manualTask;
  if (!task || manualTaskProblem(task, device)) return [];
  if (task === "backup") return hasBackup ? finalize([backupStep()]) : [];
  if (task === "restore") return hasBackup ? finalize([{ id: "restore", kind: "restore", title: "복구", desc: "선택한 기존 백업 데이터를 현재 기기에 복원", risk: "danger", estSec: 1500 }]) : [];
  if (task === "unlock" || task === "relock") return finalize(bootloaderOnlyPlan(device, task, { ...opts, restore: false }, hasBackup));
  if (task === "volte") {
    // Keep only this task's SIM choices; stale firmware/post-processing cannot add other writes.
    return buildPlan(device, { ...config, firmware: null, bootloaderAction: null }, { unroot: false, relock: false, restore: false }, hasBackup);
  }
  if (task === "verify") return [];
  if (task === "root-manager" || task === "root-modules") return [];
  const partition = deviceWorkflow(device.model, [], false).partition;
  return finalize([
    { id: "prep", kind: "setup", title: "순정 이미지 준비", desc: "현재 기기·펌웨어와 같은 순정 부트 이미지 확인", estSec: 300, manual: ["firmware-select"] },
    // 언루팅은 초기화가 없어 백업이 필요 없다(사용자 결정 2026-10-08)
    ...(hasBackup && task !== "unroot" ? [backupStep()] : []),
    { id: task, kind: task, title: task === "root" ? "루팅" : "언루팅", desc: task === "root" ? `Magisk로 ${partition} 패치·기록·매니저 설치·권한 확인` : `순정 ${partition} 양 슬롯 복원·OS 복귀·루트 확인`, risk: "warn", estSec: 600 },
  ]);
}

function setupAfterWipe(id: string): Seed {
  return { id, kind: "setup", title: "기본 설정", desc: "초기화 후 폰 초기 설정 · 개발자 옵션 · USB 디버깅", estSec: 300, manual: ["usb-debug"] };
}

/** 부트로더 언락만 / 리락만 — VoLTE 패치·펌웨어 업데이트 없음 */
function bootloaderOnlyPlan(device: DeviceStatus, only: "unlock" | "relock", opts: PlanOptions, hasBackup: boolean): Seed[] {
  const steps: Seed[] = [];
  // 리락 전 같은 펌웨어의 순정 이미지로 양 슬롯 복원. su 부재만으로 슬롯 상태를 추측하지 않는다.
  const unroot = only === "relock";
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
    steps.push({ id: "relock", kind: "relock", title: "부트로더 리락", desc: "순정 복원·현재 펌웨어 확인 후 리락 — 기기가 초기화됩니다", risk: "danger", wipe: true, estSec: 120 });
    steps.push(setupAfterWipe("setup-relock"));
  }
  if (opts.restore && hasBackup) {
    steps.push({ id: "restore", kind: "restore", title: "복구", desc: "백업한 데이터를 기기로 복원", estSec: 1500 });
  }
  return steps;
}

/** 실행 전 확인·위험 표시 대상 단계의 안내 — 초기화, 펌웨어·부트 이미지 기록, 모뎀 설정(EFS) 수정.
 *  확인 모달·실행 순서 툴팁·창 닫기 보호가 같은 기준을 쓴다 (AGENTS 규칙 7). 대상이 아니면 null */
export function stepHazard(step: Pick<PlanStep, "kind" | "wipe">): { short: string; detail: string } | null {
  if (step.wipe) {
    return { short: "데이터 초기화", detail: "부트로더 언락/리락 단계는 핸드폰 데이터가 초기화될 수 있습니다. 백업을 권장합니다." };
  }
  switch (step.kind) {
    case "fw-flash":
      return { short: "펌웨어 기록", detail: "사용자 데이터 유지를 목표로 기록하지만 손실·부팅 실패 가능성이 있습니다. 중요한 데이터는 별도로 백업하세요." };
    case "restore":
      return { short: "기존 데이터 덮어쓰기", detail: "선택한 백업 데이터를 현재 기기에 복원합니다. 현재 데이터·설정이 덮어써질 수 있습니다." };
    case "root":
      return { short: "부트 이미지 수정 기록", detail: "Magisk로 수정한 부트 이미지를 양쪽 슬롯에 기록합니다 — 기록 중 연결이 끊기면 부팅되지 않을 수 있습니다." };
    case "unroot":
      return { short: "순정 부트 이미지 재기록", detail: "순정 부트 이미지를 양쪽 슬롯에 다시 기록합니다 — 기록 중 연결이 끊기면 부팅되지 않을 수 있습니다." };
    case "efs":
      return { short: "모뎀 설정(EFS) 수정", detail: "모뎀 설정(EFS)을 직접 수정합니다 — 잘못 기록되면 통화·데이터가 안 될 수 있어, 적용 후 전수 확인합니다." };
    default:
      return null;
  }
}
