# view: VolteConfig — USIM 선택

status: implemented (2026-10-08)

자동 또는 수동 VoLTE 안내 뒤 슬롯별 SKT/KT/LG U+/패치 안 함을 선택한다. `volteConfig.sims[].carrier`를 갱신하며 1 V/5 V의 LG U+는 내부 LGU_V로 매핑한다. 원본 없이 정확히 되돌릴 수 없어 패치 제거는 제공하지 않는다.

펌웨어 설정·언락만/리락만 카드를 제거했다. 새 버전은 전용 UpdateConfig, 단독 부트로더는 ManualTasks에서만 선택한다. 이 화면은 서버 버전을 조회하지 않는다. 다음은 패치 대상 SIM이 있을 때 step2, 이전은 warning이다. 자동 계획은 과거 펌웨어/bootloaderAction도 무시한다.

기종별 참고는 기존 wizard.workflow/data/devices.ts를 재사용한다. II/IV/III/PRO-I의 기존 주의 조건을 유지하며 PDC·모뎀 교체·외부 앱 설치를 추가하지 않는다. SIM 유형은 물리/eSIM/미확인을 구분한다.

비주얼: pane 스크롤, MD3 카드·위험 톤·SIM outline 배지·lucide 아이콘, 중앙 최대 폭 통신사 카드. 신규 쓰기 계약 없음.
