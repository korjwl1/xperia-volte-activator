// 실행 단계(언락·루팅·EFS·펌웨어 기록 등 기기 쓰기)가 아직 목업인 동안 true
// — 폰이 실제로 재부팅되지 않아 모드 감지·루트 확인이 통과할 수 없으므로, 수동 확인에 "(목업) 건너뛰기"를 둔다.
// 실전 백엔드를 연결하면 false — 확인되지 않으면 진행하지 않는다.
export const SIMULATED_RUN = true;

// 단계별 실전 전환 (사용자 승인 2026-10-03: 백업·복구 엔진 코드는 실전으로 작성하되
// 실기기 검증 전까지 실행은 시뮬레이션 유지 — 데스크톱 빌드에서도 기본 꺼짐)
export const REAL_STEPS: { backup: boolean; restore: boolean; efs: boolean } = {
  backup: false,
  restore: false,
  efs: false,
};
