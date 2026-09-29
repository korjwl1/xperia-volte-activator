# 00 — 런타임 아키키처 요약

status: implemented (프론트(mock) 기준)

## 계층

```
[src/lib/views/*]      화면 6종 — 순수 표시/인터랙션, 직접 I/O 없음
[src/lib/stores/]      wizard.svelte.ts — 위자드 상태 머신 (단계/선택/세션)
[src/lib/api/]         facade — mock↔실전 전환 지점 (USE_MOCK=true)
[src/lib/mock/]        실측 기반 mock 데이터 + 시뮬레이션 러너
[향후 src-tauri]       .plans/02-contracts 계약대로 구현 (M2+)
```

## 위자드 흐름 (plan.md §3-1)

```
① DeviceStatus (감지/상태/환경검사)
② PlanReview  (프로파일 + 선택 토글 + 의존성 규칙)
③ BackupSelect (초기화 루트 있을 때만)
④ BackupTarget (경로/용량)
⑤ RunProgress (단계 카드 + 진행률 + 로그 + 수동 개입 모달)
```

## 리스크 3층 (plan.md §10/§9 — UI 어디에 묘사되는지)

| 층 | 위치 | 프론트 표현 |
|---|---|---|
| 프리플라이트 | EFS 주입 단계 직전 | RunProgress 내 사전 검사 서브스텝 + DeviceStatus의 환경검사 패널 |
| 런타임 복구 | 전송 중 이상 | RunProgress의 오류 배너 + 재연결 안내 카드(§9-4 에스컬레이션) |
| 재검증 프로브 | 재개/완료 시 | 세션 재개 화면(향후) / 단계 카드의 검증 표시 |

## 핵심 도메인 규칙 (UI가 반드시 반영)

- 의존성: 리락 ⟹ 언루팅 (끌 수 없음, §3-3) / 파괴 단계 ⟹ 완결 백업 (게이트)
- 초기화 루트: 언락·리락 단계에 ⚠ 배지 + 횟수 표시
- QPST: 미설치 = 정상 상태, 폴백 트리거는 EFS 실패 시점뿐 (§7.5)
- persist 프롭: 리락 후 소실 → 최종검증 단계 존재 (§4)
