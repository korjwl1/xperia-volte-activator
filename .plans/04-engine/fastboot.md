# fastboot 엔진 (M2) — 언락/리락·플래시

status: implemented / gated (실기기 미검증, 쓰기·재부팅 기본 비활성, 조건부 리락 구현)

- 정책: tasks/plan.md §3-3(리락·백업 의존성), §9-3(유한 처리), §10-2(모드 게이트).
- 검증: FakeTransport와 메모리 API를 사용하는 wizard 테스트만 실행. USB 장치 open·프로브·쓰기 테스트는 실행하지 않는다.
- 프로토콜 근거: [AOSP fastboot README](https://android.googlesource.com/platform/system/core/+/refs/heads/main/fastboot/README.md) (2026-10-04 확인).
- 명령 길이는 Sony 호환을 위해 기존 64바이트 제한 유지. 응답은 최대 256바이트, DATA는 정확히 8자리 hex 크기.
- INFO/TEXT는 최대 256개까지 허용한 다음 종결 응답을 읽는다. 응답 대기는 응답 1건 전체 상한(INFO가 와도 늘어나지 않음) — 일반 명령 10초, getvar:all 30초, 언락(초기화)·flash·본문 수신 확인 300초. 명령 쓰기 10초, DATA 본문은 청크마다 최대 60초·전체 600초.

## 모듈

- transport.rs: Sony VID 0x0FCE + FF/42/03 인터페이스 장치 정확히 한 대 선택. bulk IN/OUT 쌍과 실제 USB configuration/alternate setting을 적용한 뒤 claim한다.
- 명령은 단일 전송 길이가 정확히 일치해야 성공. DATA는 짧은 쓰기만큼 남은 버퍼를 다시 보내고 0바이트 쓰기는 실패. Drop에서 인터페이스 release.
- protocol.rs: OKAY/FAIL/INFO/TEXT/DATA 상태 머신, getvar·Sony oem unlock/lock·download·flash·reboot.
- mod.rs: Tauri 명령, 동시 작업 거부, 민감 로그/반환 오류 마스킹, 기본 꺼진 쓰기 게이트, 플래시 이력.

## 실행 게이트

1. 프론트 REAL_STEPS.fastboot=false: wizard 시뮬레이션 유지, facade의 unlock/lock/reboot 직접 호출도 거부.
2. Cargo feature fastboot-write는 기본 꺼짐: 직접 invoke해도 unlock/lock/flash/reboot를 USB open 전에 거부한다. 읽기 전용 getvar는 별도다.
3. unlock/lock/flash는 confirm=true 필수. 언락 코드·파티션명은 USB open 전에 검증한다.
4. backend unlock은 getvar:is-userspace=no와 unlocked=yes/no를 확인한다. fastbootd, 조회 실패, 미지원·빈 값은 거부. 이전 Sony에서 is-userspace 미지원이면 모드 근거를 추가하기 전까지 차단한다.
5. 리락은 현재 OS와 추출 이미지 지문 대조 → 추출 출처·이미지 종류/해시 → 해당 기기 양 슬롯의 최신 done 복원 이력 → bootloader 모드/동일 serial/슬롯 존재 확인 후 허용한다. 상세 절차는 [unroot-relock.md](unroot-relock.md). 언루팅 done/skipped·su 부재·일반 confirm만으로 허용하지 않는다. 전체 AVB 검증이나 외부 도구가 바꾼 파티션의 인증을 주장하지 않는다.
6. 백업을 선택한 계획이면 fastboot만 실전인 경우도 실제 완결 summary 또는 기존 폴더 파일/해시 재검사가 필요하다. 백업 미선택은 기존 실행 전 이중 확인 경로를 따른다.
7. 실전 fastboot를 켜면 수동 확인의 목업 건너뛰기를 숨기고 거부한다.

## 결과 확인·재부팅

- unlocked()는 명시적인 yes/no만 bool로 반환한다. 통신 실패·FAIL·빈 값·unknown을 false로 바꾸지 않는다.
- 언락/리락 명령 후 원하는 잠금 상태를 확인하지 못하면 오류. 자동 재부팅/연결 해제는 확인 불가로 처리하며 성공으로 추측하지 않는다.
- reboot는 OKAY만 성공. FAIL/DATA/타임아웃을 성공으로 처리하지 않는다.
- wizard는 상태 확인 성공 후 재부팅을 await하고 성공해야 완료한다. 재부팅 실패 시 정지. 재시도에서 이미 unlocked=yes인 경우에도 재부팅 확인 후 진행한다.
- 비동기 구독·프로브·명령·재부팅 뒤 실행 세대 확인. 이미 시작한 실전 단계를 tick의 시뮬레이션 완료 경로로 흘려보내지 않는다.
- 중단은 이후 프론트 후속 호출을 막는다. 이미 전송한 파괴 명령을 취소/원복하지는 못한다.

## 플래시·기록

- 입력 partition은 슬롯 접미사 없는 기본명. _a/_b를 이미 붙인 입력, 잘못된 문자·길이는 거부.
- 기록 이미지는 blocking 스레드에서 읽으며 boot·init_boot만 허용(부트 이미지 상한 256 MiB, `boot_image::MAX_BYTES`). 이미지 종류가 파티션과 맞아야 한다(init_boot = 헤더 v4·커널 없음, boot = 커널 있음). 빈 expectedSha256은 거부.
- bootloader 모드, current-slot=a/b, has-slot:<partition>=yes, 기기 식별값을 확인하고 양쪽 슬롯을 기록한다.
- max-download-size 조회의 통신 오류·잘못된 값은 다운로드 전에 실패. 변수 FAIL(미지원)만 호스트 상한(프로토콜 1 GiB, 실제 입력은 부트 이미지 256 MiB)으로 폴백.
- flash-history.jsonl: deviceKey(일련번호 SHA-256), partition, image 경로, bytes, sha256, at, status(started/done/failed).
- sha256은 실제 전송 버퍼에서 계산. 경로 파일을 다시 읽지 않는다.
- 이력 파일 open과 started 기록·sync가 성공해야 flash를 시작한다. 슬롯별 done/failed도 기록·sync하고 저장 실패는 반환한다.
- 이 기록만으로는 리락하지 않는다. 추출 출처/현재 펌웨어 대조와 실제 기기·모드 검사에 결합한다. 외부 플래시·vbmeta/커널 변경까지 검증하는 기능은 없다.

## 이벤트·마스킹

- fastboot:log의 실제 payload는 string (객체가 아님).
- 언락 명령 자체는 코드 대신 [마스킹] 표기. 기기가 INFO/FAIL에서 코드를 반복해도 대소문자 무관 마스킹 후 이벤트/반환 오류에 전달한다.
- getvar:all의 IMEI/MEID/serialno 로그는 마스킹. 개별 식별값 getvar의 OKAY 응답도 로그에 원문을 남기지 않는다.
- 원문 getvar 맵은 메모리 프로브 결과로만 반환하고 wizard/journal에 통째로 저장하지 않는다.

## 코드 리뷰에서 고친 결함 (2026-10-04)

| 문제 | 수정 |
|---|---|
| 리락 상태 조회 실패가 unlocked=false로 바뀌어 성공 처리 | yes/no만 허용, 확인 불가 오류 |
| 재부팅 FAIL/DATA와 타임아웃이 성공 처리 | OKAY만 성공, wizard await |
| 상태 확인 전에 재부팅 실행 | 상태 확인 성공 뒤 실행 |
| fastboot만 켜면 선택한 백업의 완결 게이트 우회 | fastboot 활성 시에도 완결 강제 |
| 목업 언루팅·skipped·미포함 단계로 리락 허용 | 현재 펌웨어·순정 출처·같은 기기·양 슬롯 완료 이력을 확인하고 리락 실행 |
| 프론트 플래그만으로 보호해 직접 invoke 가능 | 기본 꺼진 Cargo 쓰기 feature 추가 |
| USB 부분 전송 길이 무시·엔드포인트 종류/alt 미검사 | 실제 전송 길이와 bulk 설정 검증 |
| 슬롯 변수 이름의 ':'가 잘림 | 이름/값 구분 파싱 수정 |
| 플래시 후 파일을 다시 해싱·기기 없는 이력·저장 오류 무시 | 버퍼 해시·deviceKey·슬롯별 상태·저장 오류 반환 |
| 코드 대소문자 변형/반환 오류, 식별값 로그 유출 | 이벤트·오류 마스킹 및 식별값 응답 마스킹 |
| 실전 step 재진입 시 시뮬레이션 진행률 상승 | 실전 분기에서 항상 return |

## 검증

- pnpm.cmd test:fastboot — 실제 wizard를 Vite/Svelte로 컴파일, API는 메모리 가짜로 교체. 백업 의존성·재진입·unknown/fastbootd·결과 확인·재부팅 대기/실패·중단·리락 게이트 검증.
- pnpm.cmd check — Svelte/TypeScript 검사.
- cargo test --lib — 실기기/네트워크 live 테스트는 ignore 유지.
- cargo test --lib fastboot --features fastboot-write — feature가 켜진 코드의 가짜 테스트, 기기 호출 없음.
- 코드 검증 결과: Rust 전체 83개 통과(3개 ignored), fastboot-write 활성 상태의 가짜 테스트 28개 통과, wizard 가짜 API 테스트 8개 통과. Svelte/TypeScript 검사·프론트 production 빌드·git diff --check 통과.

## 실제 테스트하지 않은 부분

| 항목 | 아직 확인하지 않은 내용 |
|---|---|
| USB 연결 | Windows 드라이버 바인딩, rusb open/claim, configuration/alternate setting, release 후 재연결 |
| Sony getvar | getvar:all 응답 형식, is-userspace 지원 여부, 슬롯·다운로드 상한·일련번호 조회 |
| 언락 | 실제 OEM 명령 수락, 초기화, unlocked 상태 조회 가능 시점, 자동 재부팅·USB 연결 해제 |
| 재부팅 | OS/bootloader 재부팅 응답과 모드 전환, 초기화 후 wizard의 다음 단계 진행 |
| 플래시 | 실제 이미지 DATA 전송·부분 전송, 양쪽 슬롯 기록, 연결 해제/타임아웃 시 기기 상태와 이력 |
| 리락 | 실제 OEM 명령·초기화·잠금 결과·AVB 부팅. 순정 출처·현재 관찰 펌웨어·기기 대조 및 양 슬롯 이력 게이트는 구현했지만 전체 AVB 부트 체인의 실기기 검증은 아직 수행하지 않음 |

실기기 테스트는 이번 작업에서 실행하지 않았다. 후속 작업에서도 미실측 항목은 이 목록에 기록하고, 실측 완료 시 결과와 함께 갱신한다.

## 병합 후 통합 점검 (2026-10-04)

당시 결과는 [integrated-review.md](integrated-review.md) 참조. 이후 사용자 지시에 따라 조건부 리락을 구현했다. 최신 조건은 [unroot-relock.md](unroot-relock.md)와 [적대적 리뷰](adversarial-review-20261005.md)를 따른다. 언락·기록·재부팅은 expectedSerial과 fastboot serialno가 같아야 한다. 기록은 필수 expectedSha256과 실제 전송 버퍼 및 OS에서 확인한 기기·펌웨어 출처를 대조한다. 공통 변경 실행권으로 ADB 엔진과 동시 실행하지 않는다. 이력 누락·손상·최신 미완료는 리락을 거부한다. 손상 이력은 명시적으로 보관한 뒤 순정 양 슬롯 기록을 다시 수행해야 한다.

## 2026-10-07 실기기 사전 점검과 Windows fastboot 드라이버 (사용자 승인)

- **사전 점검 (XQ-DQ44, 폰 데이터 변경 없음):** 개발 CLI로 부트로더 재부팅 → `fastboot_getvar` → OS 재부팅을 실행했다.
  - `is-userspace=no`, `unlocked=no`, `secure=yes`, `current-slot=a`, `slot-count=2`, `max-download-size=805306368`, `version-bootloader=8550-0001_X_Boot_SM8550_LA1.1_U_110`, 변수 245개.
  - `fastboot_reboot`의 serialno 대조가 ADB 시리얼과 일치해 통과했다. 15초 뒤 Android로 다시 연결됐다.
- **드라이버 문제:** 부트로더 모드 폰은 Windows에 자동으로 붙는 드라이버가 없어(문제 코드 28) libusb 목록에 나오지 않는다. 원본 도구는 장치 관리자에서 사람이 Sony 드라이버를 직접 지정하게 했다.
- **자동화 (`src-tauri/src/usb_driver.rs`):**
  - 판매명으로 공식 드라이버를 받는다(Microsoft WHCP 서명 `sa0200adb.inf`, 이 PC에 이미 있던 파일과 SHA-256이 같음).
  - 관리자 권한으로 같은 실행 파일을 `--xva-bind-usb-driver`로 다시 띄워 `SetupCopyOEMInf` → INF 종류 지정 → `DI_ENUMSINGLEINF`/`ALLOWEXCLUDEDDRVS` 드라이버 목록 → `DiInstallDevice`를 실행한다.
  - 실기기에서 설치 후 "Sony sa0200 ADB Interface Driver"로 연결됐고 getvar가 성공했다.
- **GUI:** 언락·리락의 "부트로더 진입 확인" 대기 중 `fastboot-nodriver`를 보면 한 번 자동 설치한다(UAC). 실패하면 사유를 남기고 계속 기다린다.
- **실제 언락 (2026-10-07, XQ-DQ44):**
  - `oem unlock 0x<코드>`는 즉시 OKAY였고 자동 재부팅은 없었다. 직후 `getvar unlocked`는 `no`였다(GUI가 성공으로 처리하지 않고 멈춤).
  - 개발 CLI로 `reboot-bootloader` 후 다시 읽자 `yes`가 됐다. Sony 부트로더는 잠금 상태를 부트로더를 다시 시작해야 갱신한다.
  - `fastboot_unlock`을 고쳤다. OKAY 뒤에도 `no`면 `reboot-bootloader`를 보내고, 최대 90초 안에 다시 열어 같은 기기·부트로더 모드를 확인한 뒤 `unlocked`를 읽는다. 여전히 `no`면 실패다. 전 과정 동안 기기 작업 실행권을 쥔다.
  - getvar의 `serial:` 변수가 로그에 마스킹되지 않던 문제도 함께 고쳤다.
- **fastbootd (2026-10-07, XQ-DQ44):**
  - 부트 이미지 기록은 fastbootd(`reboot-fastboot`/`adb reboot fastboot`)에서만 된다. 부트로더에서는 거부된다.
  - USB `18D1:4EE0`, `is-userspace=yes`, `has-slot:init_boot=yes`, `max-download-size=0x10000000`, `getvar:all` 변수 347개.
  - INFO 상한은 4096이다. 장치를 열 때 이전 연결이 남긴 응답을 비운다(끊긴 getvar:all 뒤 명령 전송 시간 초과를 실측).
  - 양 슬롯 판정은 `has-slot`이 없으면 `partition-size:<p>_a/_b`로 한다(부트로더의 init_boot).
