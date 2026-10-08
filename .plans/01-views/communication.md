# view: Communication — 수동 통신 확인

status: implemented (2026-10-08)

수동 통신 확인에서 deviceList로 같은 모델·시리얼 한 대를 반복 확인하고 슬롯별 IMS를 표시한다. 실제 통화/음성은 사용자 확인이다. 실패·다른 기기·늦은 결과는 성공으로 처리하지 않는다.

상태는 선택 기기/checking/error/슬롯 진단/사용자 체크. 뒤로는 returnToTasks. 재부팅·쓰기·루트 권한·완료 journal 생성 없음.

비주얼: 사이드바 없는 pane 스크롤, MD3 상태·위험 톤·lucide·고정 헤더.
