# view: UpdateConfig — 업데이트 안내

status: implemented UI / execution blocked (2026-10-08)

VoLTE 인식 기기의 update에서 같은 폰 조회·손실 위험·판올림·보존 범위를 안내한다. riskAck+목표 firmware 선택 뒤 공통 백업 선택으로 이동하며 이전은 mode-select다.

ensureFirmwareVersions/firmwareVersions는 이 화면에서만 호출한다. 현재 버전 제외·중복 제거 목록, 로딩/빈 결과/실패 안내. 연결 확인 중/IMS 불충족은 선택을 막는다. 자동/수동은 현재 설치 버전 부트 이미지를 준비한다.

루팅 기기의 목표 이미지 패치·후속 기록은 준비 중, 기존 IMG 재사용 금지 안내. 전체 취득/기록/루팅 유지 미완성으로 실행 차단. 언락/리락/재패치/자동 복구를 추가하지 않는다.

비주얼: pane 스크롤, MD3 안내·버전 목록·위험 톤·lucide, 공통 백업 화면. 신규 쓰기 계약 없음.
