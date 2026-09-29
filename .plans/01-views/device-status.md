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
