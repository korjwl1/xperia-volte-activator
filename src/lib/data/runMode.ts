import type { ExecutionFlags } from "$lib/domain/execution";

// 실행 단계(언락·루팅·EFS·펌웨어 기록 등 기기 쓰기)가 아직 목업인 동안 true
// — 폰이 실제로 재부팅되지 않아 모드 감지·루트 확인이 통과할 수 없으므로, 수동 확인에 "(목업) 건너뛰기"를 둔다.
// 실전 백엔드를 연결하면 false — 확인되지 않으면 진행하지 않는다.
export const SIMULATED_RUN = true;

// 단계별 실전 전환 (사용자 승인: 백업·복구·fastboot 2026-10-03, 루팅 2026-10-04 —
// 실기기 검증 전까지 실행은 시뮬레이션 유지 — 데스크톱 빌드에서도 기본 꺼짐)
// verify: 업데이트 확인(fw-verify)·최종 확인(final-verify) 실전 — 재부팅·재연결 대기·버전/지문 대조·VoLTE 등록 확인.
//   최종 확인의 재부팅은 Rust root_reboot(쓰기 기능 빌드)를 쓴다. 펌웨어 기록이 시뮬레이션인 동안 fw-verify 실전은 기록 엔진 미구현으로 즉시 실패한다.
// 실전 조합은 실행 전에 전체 계획을 검사한다. 한 단계라도 모의 기기 작업이면 시작을 거부한다.
export const REAL_STEPS: ExecutionFlags = {
  // 2026-10-08 실기기(XQ-DQ44) 검증을 마친 단계만 켠다(사용자 결정) — 백업·복원·언락·루팅·언루팅·VoLTE.
  // 리락(초기화 동반)·펌웨어 업데이트 확인은 검증 전이라 끈다.
  backup: true,
  restore: true,
  fastboot: true,
  relock: false,
  root: true,
  rootTools: false,
  verify: false,
  efs: true,
};
