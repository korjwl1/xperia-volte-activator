# view: RunProgress (⑤ 실행)

status: implemented (mock 시뮬레이션)

## 목적/진입
확정된 계획 실행. 단계 카드 + 진행률 + 실시간 로그 + 수동 개입 대기(모달) + USB 복구 UX.

## 상태 필드
- 단계 카드 리스트: RunStep[] — 상태(pending/running/done/failed/skipped/manual-wait), 단계별 진행률, 전체 진행률(가중)
- 로그 스트림: 스크롤 영역, 타임스탬프 라인 (마스킹 규칙 적용 표시)
- 수동 개입 모달(ManualStep): 어떤 폰 조작이 필요한지 안내 + [폰에서 완료] 버튼
- USB 복구 배너(§9-4): 1회차 재연결 안내 / 2회차+ 케이블·허브 권고 + 포트 이력
- 위험 단계 확인 게이트: 언락/리락 실행 전 모달 확인

- 단계별 수동 개입은 배열로 순차 진행 (`PlanStep.manual: ManualId[]`, `RunStep.manualDone`)
  - 사전 준비: [oem-toggle(필요 시) → unlock-code(잠김) → firmware-select(루팅 필요 시 — 업데이트면 새 버전 이미지)] / 부트로더 언락: [mode-wait] / 루팅: [magisk-patch]
  - 자동 감지(감지되면 바로 진행, 수동 [완료]도 가능 — 모달에 "자동 감지 중" 표시):
    - usb-debug: 단계 도달 시 이미 연결이면 모달 없이 진행, 아니면 2초 폴링
    - mode-wait: usb_modes에 fastboot 감지(1.5초) / flash-mode: flashmode 감지(1.5초)
    - ims-check: device_list의 패치 대상 슬롯이 모두 VoLTE 활성화(on)면 진행(5초), 모달에 슬롯별 상태 표시
  - oem-toggle: 모달이 열리면 폰에 개발자 옵션(꺼져 있으면 휴대전화 정보) 화면 자동 오픈 + [폰에서 설정 화면 열기]
  - 루팅: 수동 Magisk 패치 없음 — 자동 패치(02-contracts "root" 절차), 로그로 진행 표시
  - 펌웨어 업데이트: [flash-mode] (전원 끄고 볼륨 아래 + USB, 초록 LED) → newflasher 기록(mock) → 업데이트 확인(지문)
  - oem-toggle 모달: 세 항목 상태(켜짐/꺼짐/확인 불가) + [다시 확인](기기 재조회) — 꺼진 항목이 없어야 완료 가능(확인 불가는 막지 않음)
  - VoLTE 적용: [su-grant] (원본 CLI의 DIAG 포트 개방 시 루트 권한 승인)
- 원본 CLI 계승 단계: VoLTE 적용 후 "VoLTE 활성화 설정"(persist.dbg ims/volte/vt/wfc 4종 + 재부팅) — mock
- [중단]: 진행 중·수동 대기 단계를 대기로 되돌리고 해당 단계의 수동 개입은 처음부터 / USB 오류 배너가 떠 있는 동안 [이어서] 비활성
- 펌웨어 모달은 기종별 대상 파티션 안내(`data/devices.ts` — 원본 root() 기준, 미등록 기종은 "확인되지 않음")
- 언락 코드는 앞의 0x 접두어를 제거해 정규화 후 사용(로그도 정규화 값 마스킹)
- 백업 단계 시작 시 **백업 직전 안내**(`components/BackupNotice.svelte`, manual id `backup-notice`) — 전용 큰 화면
  - 탭: [백업 및 복구 불가능](기본) | [설정] | [앱]
    - 불가능: ① 미리 직접 옮기지 않으면 데이터가 사라지는 앱(lost) ② 다시 로그인이 필요한 앱(relogin — 앱만 재설치, 앱 데이터 백업 미선택 시 restored도 여기로) ③ 그 외 복구되지 않는 항목(인증서·OTP, 결제·기기 등록, 생체·화면 잠금, 알림 설정, 기본 앱 지정, 기본 앱 내부 설정, 블루투스, SIM PIN)
    - 설정: 자동 복원 대상 설정의 현재 값(`settings_overview`) + 배터리 최적화 예외 앱 수 + 보관용 전체 설정 키 개수. 설정 백업 미선택 시 경고
    - 앱: **실제로 백업·복구되는 앱만**(restored — 외부 데이터 존재 + 앱 데이터 백업 선택) + 태그(APK, 외부 데이터). 분류 규칙은 `data/appRules.ts`
  - 하단 고지(완전 복구 비보장, 사전 백업 권고, 책임은 사용자) + "필요한 사전 백업을 마쳤으며…" 체크 → [백업 시작] 활성 / [작업 중단]
- 입력형 모달 (mock — 값은 스토어에만 보관)
  - unlock-code: 모달이 열리면 공식 발급 페이지(opendevices.sony.net)를 시스템 브라우저로 자동 오픈 + IMEI 1 자동 읽기(read_imei1)
    → IMEI 마스킹 표시 + [IMEI 복사] + [발급 페이지 다시 열기] + 코드 입력(비밀번호형, 16자리 hex 검증). 로그에는 `0x` + 앞 3자 외 마스킹
  - firmware-select(사용자 지시 2026-10-03): 팝업 없이 firmware_fetch 자동 실행(로그에 진행 표시) → 성공하면 바로 다음 단계.
    실패했을 때만 팝업 — 원인별:
    - 저장 공간 부족(Rust 오류 접두어 `NO_SPACE|`, 받기 전에 .sin 크기 + 16 MiB로 확인): [위치 선택]으로 다른 저장 위치 → [이 위치로 다시 받기](성공 시 팝업 닫고 진행)
    - 다운로드 실패(서버·지문 불일치·미지원 기종 등): 사유 + [다시 시도] + XperiFirm으로 받는 방법(기종·버전) + 받은 폴더 지정
  - 언루팅용 순정 이미지도 같은 펌웨어에서 추출(별도 입력 없음)
  - 입력 전 [입력 완료] 비활성

## 인터랙션 → 계약 매핑
| 요소 | 동작 | 계약 |
|---|---|---|
| [실행 시작/일시정지] | 러너 제어 | `plan_run` (이벤트 `StepEvent`) — mock: 시뮬레이션 러너 |
| [폰에서 완료] (모달) | 수동 단계 통과 | 이벤트 수동 ack |
| [중단] | 세션 보존 후 정지 | `session_save` |
| USB 배너 [재시도] | 재감지 대기 | 이벤트 `device:changed` 재구독 |
| 완료 화면 [복구 승인] | 백업 보존 해제 | `backup_ack` (M3) |

## 시뮬레이션(mock) 요구사항
- 단계 순차 진행, 진행률 상승, 로그 라인append
- 루팅 단계에서 manual-wait 1회 발생
- (데모 토글) 전송 중 오류 1회 유발 → USB 복구 배너 시연

## 비주얼 (desktop-ui 스킬)
**좌우 분할 pane**(좌=단계 타임라인·자체 스크롤 / 우=다크 콘솔·모노·라인 번호·색상 로그·자동 스크롤) / 상단 전체 진행 gradient 바 + 컨트롤 / 모달 = backdrop blur + 번호 스텝 카드.

- 단계 실패(wizard.failStep): 단계 상태 failed 유지, 실행 멈춤, 상단 경고 [이 단계 다시 시도] / [중단] — 다음 단계·리락으로 넘어가지 않음.
  개발용 "EFS 실패 시뮬" 스위치(VoLTE 적용 중간에 실패)
- EFS 단계 시작 시 사용할 프리셋 기록: 슬롯·통신사·버전·파일 수·SHA-256 앞자리
- 세부 작업: VoLTE 적용 = 슬롯별 1차·2차 업로드, 적용 확인 = 슬롯별 전수 리드백·해시 비교
- 통신 확인(ims-precheck): 슬롯별 IMS 상태 표시 + "실제로 걸고 받아 통화되는 것을 확인했습니다" 체크 후 [확인하고 진행].
  IMS 미확인 시 [다시 패치] → VoLTE 적용(연결 안정성 검사)부터 다시
- 완료 화면: 통신 확인을 생략했으면 "작업 종료 · 통신 미검증"(성공 표시와 구분). 패치했으면 직접 확인 목록
  (실제 발신·수신, 문자·MMS·5G 데이터, 해외 로밍은 별개, SIM 교체·망 변경·모뎀 포함 업데이트 후 재확인)
## Native EFS integration (2026-10-04)

- REAL_STEPS.efs는 기본 false. 기존 화면·위험 확인 게이트·시뮬레이션을 유지한다.
- 실전 러너는 api facade의 설정된 COM·bundle root·snapshot root를 사용한다. 설정 미지정·전체 선택의 공유 EFS/NV 충돌은 DIAG 전환 전에 failed로 중단한다.
- efs-preflight: efsValidatePresets → efsDiagOpen(선택한 ADB serial) → efsPreflight(설정된 COM).
- efs: 슬롯별 scoped before-image efsSnapshot → efsUpload 1차·2차. verify: 슬롯별 efsVerify, 누락·불일치·읽기 오류는 failed 유지.
- native 구조화 오류와 setup/emptyNvSkipped/nvPrefixVerification 경고를 로그에 표시한다. skipped 항목은 written/verified 수에 포함하지 않는다. 리드백은 IMS·실제 통화 성공의 증거와 별개다.
- [중단]/[처음으로]는 api.efsCancel 호출·세대 가드로 후속 작업을 멈춘다. 이벤트 구독은 finally 해제. 슬롯별 before-image 폴더를 로그에 남기며 자동 rollback은 하지 않는다.
