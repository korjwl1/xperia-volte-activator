# view: VolteConfig (1단계 — SIM 및 통신사 선택)

status: implemented

## 목적/진입
경고 페이지 동의 후. SIM 슬롯별로 적용할 통신사를 고른다 (원본 CLI의 SIM1/SIM2 각각 지정 계승).

## 상태 필드
- SIM 카드(슬롯별): 슬롯 번호, 물리/eSIM 배지, 현재 통신사(또는 미삽입)
  - 카드 안 선택지: [SKT] [KT] [LG U+] [패치 안 함] → `wizard.volteConfig.sims[].carrier` (null = 패치 안 함)
  - LG U+ 선택 시 1 V/5 V(XQ-DQ*/XQ-DE*)는 내부적으로 LGU_V 전용 프리셋 사용 (UI에는 LG U+로 표시)
  - 패치 대상 슬롯 카드는 primary 테두리
- 프로파일 모드 선택 없음 (사용자 결정 2026-10-02): EFS 프리셋은 원본 CLI가 실제로 사용해 온 단일 프리셋(SonyEFS, balance)만 사용.
  원본의 실내 우선(SonyEFS_perf)은 CLI에서 연결된 적 없고 LG U+ 경로 오타(XPERIA-LGU_peref)로 동작 불가였음 — 사용하지 않음

## 인터랙션 → 계약 매핑
| 요소 | 동작 | 계약 |
|---|---|---|
| 통신사 버튼 | 슬롯별 대상 갱신 | (프론트) |
| [다음] | 패치 대상 슬롯이 1개 이상일 때만 활성 → step2 | — |
| [이전] | 경고 페이지 | — |
