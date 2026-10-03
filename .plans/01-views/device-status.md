# view: DeviceStatus (① 디바이스 감지/상태)

status: implemented (데스크톱 실측 + 브라우저 mock)

## 목적/진입
앱 시작 첫 화면. 연결된 폰 감지 → 기종/상태 안내 → 환경 검사(§12.6) → [작업 시작]로 계획 생성.
연결 없으면 대기 화면(폴링 안내 + "폰을 연결해주세요").
연결 수단 점검(`adb_status`) 실패 시 재시도 팝업(닫기/다시 시도) 표시.

## 상태 필드 (출처: `device_list`/`device_status`/`env_check`)
- 기기 카드: 모델명(XQ-DQ44), 제품명(Xperia 1 V), 펌웨어(67.2.A.3.178), Android(15), 모드(Android/bootloader-fastboot/fastbootd/flashmode), serial(부분 마스킹)
- 상태 배지: 부트로더(잠김/언락/unknown), 루팅(yes/no/unknown — 미승인 구분), VoLTE(on/off/unknown + 사유), SIM 슬롯별(450/05 SKT 등)
- USB 정보 행: 토폴로지(루트허브 직결/허브 경유), 컨트롤러, 링크 스피드
- 환경검사 패널: EnvCheckItem[] (pass/warn/fail/info + 자동수리 버튼)

SIM 줄의 VoLTE 표기: on → "VoLTE 활성화" / off → "VoLTE 비활성화" / unknown → "VoLTE 상태 확인 불가" (추측 표기 금지).
  판정 출처는 IMS 음성 등록 상태(전화 앱 히든 메뉴 *#*#4636#*#*와 동일) — 패치 프롭이 아님.
폴링: 3초 간격, 이전 조회 진행 중이면 건너뜀, 조회 실패는 연속 2회일 때만 "연결된 기기 없음"으로 전환.

## 인터랙션 → 계약 매핑
| 요소 | 동작 | 계약 |
|---|---|---|
| 자동 감지 | 화면 진입 시 폴링/이벤트 | `device_list`, 이벤트 `device:changed` |
| 연결 점검 | 3초 폴링, 실패 시 재시도 팝업 | `adb_status` (`available=false` → 팝업) |
| [환경 항목 자동 수리] | 개별 fix 실행 | `env_fix({id})` |
| [작업 시작] | 상태 기반 계획 생성 → ②로 | `plan_generate` (mock: wizard.goPlan()) |
| 새로고침 아이콘 | 재스캔 | `device_status({serial})` |

## 이탈
뒤로 갈 곳 없음(첫 화면). 상태는 스토어에 유지.

## 비주얼 (desktop-ui 스킬)
DeviceHero(gradient 히어로 + 폰 SVG 일러스트 + 반투명 상태 칩) / USB 정보 바(Usb 아이콘·info 컬러) / 환경검사 리스트(의미색 아이콘 배지 + tonal 컨테이너, sticky 헤더).

- [작업 시작](wizard.startSession): 기기를 고정. 이전과 다른 기기면 SIM·펌웨어 선택, 동의, 입력값(언락 코드·펌웨어)을 새로 시작
- 화면을 떠난 뒤 도착한 기기 조회 결과는 버림 — 작업 중 선택 기기가 바뀌지 않음
- 모드 감지(부트로더·플래시)는 Sony USB 장치가 정확히 1대일 때만 통과 — 여러 대면 분리 안내
