# view: ManualTasks — 수동 그리드

status: implemented (2026-10-08)

백업·복구·언락·리락·루팅·언루팅·VoLTE 패치·통신 확인·루팅 매니저 변경·루팅 모듈 설치를 3열 그리드로 표시한다. MANUAL_TASKS/manualTaskProblem 조건을 계획과 공유하고 최신 deviceList 조회 성공 후 chooseManualTask한다.

백업/복구는 Android 연결, 언락은 지원 파티션+locked, 리락은 지원 파티션+unlocked, 루팅은 unlocked+비루팅, 언루팅/패치/매니저 변경은 지원 unlocked+rooted, 모듈은 unlocked+rooted, 통신 확인은 인식된 SIM이 필요하다. 불충족은 비활성 이유를 표시한다.

백업→step2+기록 확인, 통신 확인→communication, 매니저/모듈→root-tools의 해당 section, 나머지→작업별 warning. 패치는 USIM을 거친다. 이전→mode-select. 완료 뒤 입력을 정리하고 목록으로 돌아간다.

비주얼: lucide 아이콘/설명의 MD3 카드·비활성 사유·pane 스크롤. 직접 쓰기 호출 없음.
- 2026-10-08 (사용자 결정): 수동 언루팅은 초기화가 없으므로 백업 단계를 넣지 않는다(준비 → 언루팅). 루팅·리락 계획은 그대로다.
