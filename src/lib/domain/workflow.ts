import type { DeviceStatus, ManualTask } from "$lib/types";
import { bootPartition } from "$lib/data/devices";
import { cellularReady } from "$lib/domain/communication";

export const MANUAL_TASKS: { id: ManualTask; title: string; detail: string }[] = [
  { id: "backup", title: "백업", detail: "선택한 데이터를 PC에 저장" },
  { id: "restore", title: "복구", detail: "기존 백업에서 선택한 데이터 복원" },
  { id: "unlock", title: "언락", detail: "부트로더 잠금 해제 · 데이터 초기화" },
  { id: "relock", title: "리락", detail: "순정 부트 복원 후 잠금 · 데이터 초기화" },
  { id: "root", title: "루팅", detail: "현재 펌웨어 부트 이미지 패치·적용" },
  { id: "unroot", title: "언루팅", detail: "현재 펌웨어 순정 부트 이미지 복원" },
  { id: "volte", title: "VoLTE 패치", detail: "선택한 SIM의 통신사 설정 적용" },
  { id: "verify", title: "통신 확인", detail: "SIM별 IMS 등록 상태·실제 통화 확인" },
];
export function updateProblem(device: DeviceStatus | null): string | null {
  if (!device || device.state !== "device") return "Android에서 연결되고 USB 디버깅이 승인된 기기가 필요합니다";
  return device.sims.some(cellularReady) ? null : "셀룰러 VoLTE가 인식되는 SIM이 있어야 합니다";
}
export function manualTaskProblem(task: ManualTask, device: DeviceStatus | null): string | null {
  if (!device || device.state !== "device") return "연결된 Android 기기를 확인하세요";
  if (task === "backup" || task === "restore") return null;
  if (task === "verify") return device.sims.some(s => s.carrier !== null && s.state !== "ABSENT") ? null : "인식된 SIM이 필요합니다";
  if (!bootPartition(device.model)) return "이 기종의 부트 파티션·작업 절차가 확인되지 않았습니다";
  if (device.bootloader === "unknown") return "부트로더 상태를 다시 확인하세요";
  if (task === "unlock") return device.bootloader === "locked" ? null : "이미 언락된 기기입니다";
  if (device.bootloader !== "unlocked") return "먼저 부트로더를 언락해야 합니다";
  if (task === "relock") return null;
  if (device.rooted === null) return "루팅 상태를 다시 확인하세요";
  if (task === "root") return device.rooted === false ? null : "이미 루팅된 기기입니다";
  return device.rooted === true ? null : "루팅된 기기가 필요합니다";
}
