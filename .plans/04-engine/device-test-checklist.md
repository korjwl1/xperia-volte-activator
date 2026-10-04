# 실기기 검증 체크리스트

status: 진행 전 — 2026-10-05 기준 아래 항목은 모두 **실기기에서 확인하지 않았다**(가짜 기기·가짜 전송 단위 테스트만 통과).

이 문서가 실기기 미검증 항목의 단일 목록이다. 각 엔진 문서(backup-engine·fastboot·root·unroot-relock·integrated-review·code-review)의
"검증 대기/미검증" 표는 배경 설명이고, 실제 확인 여부는 여기서 체크한다.

## 규칙

- 단계의 `REAL_STEPS` 플래그(`src/lib/data/runMode.ts`)와 쓰기 Cargo 기능(`fastboot-write`·`root-write`·`efs-write`)은 **그 단계의 항목이 모두 체크되기 전까지 켜서 배포하지 않는다.**
- 확인할 때는 `[x]`로 바꾸고 날짜·기종(모델·펌웨어)·결과 한 줄을 적는다. 실패하면 체크하지 말고 "결과"에 증상을 적은 뒤 코드를 고친다.
- 파괴 단계(언락·기록·리락)는 백업 완료·순정 펌웨어 준비 상태에서, 복구 가능한 테스트 기기로만 확인한다.
- 네이티브 EFS도 이 목록에서 관리한다. 2026-10-05 병합·오프라인 검증 완료는 실기기 확인으로 간주하지 않는다.

## 1. 공통 기기 I/O (모든 단계의 전제)

- [ ] **셸 종료 코드 표식** (`device_io::shell_run`, `__XV_RC=$?`)
  - USB 직접 연결(adb 서버 없음)과 adb 서버 연결 각각에서 성공 명령은 종료 코드 0, `false`·없는 명령은 0이 아닌 값으로 판정되는가
  - 개행으로 이은 표식이 Sony 기기 셸(toybox sh)에서 그대로 동작하는가, 출력 끝 표식이 잘리지 않는가
  - 2026-10-03에 실측한 백업 읽기 명령(getprop·pm list·dumpsys·settings·content query)이 표식 추가 후에도 같은 결과를 내는가
- [ ] **USB 직접 연결 stderr 혼합**: USB 직접 연결에서 stderr가 stdout에 섞여도 파서(설정·연락처·저장 공간)가 오판하지 않는가
- [ ] **adb_client I/O 시간 상한** (`vendor/adb_client`, PATCHES.md): USB 읽기 300초·쓰기 30초, 서버 읽기/쓰기 300초
  - 큰 앱 `pm install-commit`(dexopt), 저장 공간 측정 `du`, boot_patch.sh가 상한 안에 끝나는가
  - 폰을 응답 없게 만든 뒤(화면 잠금 상태 USB 디버깅 해제 등) 300초 안에 오류로 끝나고 다음 호출이 다시 연결되는가
- [ ] **연결 선택**: adb 서버 공존·복수 기기·Sony 외 기기 혼입 시 Sony 1대만 고르는가, 미승인(unauthorized) 안내가 뜨는가
- [ ] **루팅 여부 3단계** (`adb.rs` `root_state`): 루팅 기기 true, 잠긴 순정 false, 언락·su 숨김(Magisk 앱 전용 권한) "unknown"

## 2. 백업 (`REAL_STEPS.backup`)

2026-10-03 읽기 전용 실측 이후 판정 로직이 바뀌었으므로 다시 확인한다.

- [ ] 전체 백업 1회 완주 — 항목별 파일 수·바이트·해시, 완결 게이트(complete) 결과가 실제 폰과 맞는가
- [ ] **빈 목록 확인** (`walker::confirm_empty`): 권한 없는 폴더(예: 다른 앱의 `Android/data/<pkg>` 일부)가 "읽을 수 없음" 오류로 잡히는가,
      진짜 빈 폴더·없는 기명 폴더(Recordings 등)는 오류가 아닌가, 빈 폴더가 많을 때 소요 시간
- [ ] 파일명 금지 문자(`\`·`:` 등)·대소문자 충돌·긴 경로가 격리(quarantine)로 가고 검증을 통과하는가
- [ ] 4GiB 이상 파일 크기 확인(stat_extended), mtime 보존
- [ ] 연락처 `content query` — 정상 조회·권한 거부·결과 없음 출력 형식, 합쳐진 연락처의 vCard(N 1개·번호 중복 제거)를 연락처 앱이 가져오는가
- [ ] 백업 중 USB 분리·저장 공간 부족·[중단]·이어서 백업(재개 기기 키 대조)
- [ ] 백업 취소 토큰: 준비 직후 중단, 실행 중 중단이 해당 실행에만 적용되는가

## 3. 문자·통화 (sms-ie)

- [ ] APK 다운로드 검증(다이제스트·인증서 핀 `c105e6d9…`)·설치·`pm grant`·기본 문자 앱 전환/원복
- [ ] **내보내기 파일명**: sms-ie가 실제로 `messages<날짜>.zip`·`calls<날짜>.json`을 만드는가(원본 소스 기준으로 고침, 실측 전) — 수집이 두 항목을 모두 인식하는가
- [ ] 선택한 항목이 모두 끝나면 기기 임시 폴더가 지워지는가
- [ ] 복원: 파일 전송·가져오기·역할 원복, 비행기 모드 안내, 강제 종료 후 재연결 시 원복 기록 유지

## 4. 복구 (`REAL_STEPS.restore`) — 기기에 쓰기

- [ ] **exec 완료 대기** (`restore::exec_checked`): 기기 명령 출력 끝까지 기다린 뒤 종료 코드를 판정하는가
- [ ] **`tar -xf -` 종료**: adb_client exec가 stdin을 닫지 않는다 — toybox tar가 아카이브 끝 블록 뒤 스스로 끝나는가(아니면 300초 시간 초과 오류)
- [ ] tar 복원 결과: 원래 경로·mtime·긴 이름(GNU longname)·격리 파일이 올바른 위치에 복원되는가
- [ ] APK 재설치: split APK 세션(install-create/write/commit), 실패 시 abandon, 이미 설치된 앱 건너뜀
- [ ] 설정 화이트리스트·배터리 최적화 예외(`user,<pkg>,<uid>` 형식) 복원 후 실제 값
- [ ] 대용량(수십 GB) 복원 시간·진행 표시·USB 쓰기 30초 상한 여유

## 5. 펌웨어·부트 이미지

- [ ] Sony 서버 부분 다운로드(Range 206)·조각 목록(연속 번호)·ZIP64
- [ ] **부트 이미지 헤더 검사** (`boot_image::validate`): 실제 init_boot(v4)·boot(v2/v3) 순정 이미지가 통과하는가, 잘린 파일이 거부되는가
- [ ] `.sin` 안 이미지 조각 구성(.000 하나인가) — 실제 펌웨어에서 오탐 없음
- [ ] update.xml·`.sin` 유일성 규칙이 실제 펌웨어 ZIP 구조에서 오탐 없이 동작하는가
- [ ] 직접 지정 펌웨어 폴더 검사·캐시 추출

## 6. 루팅·언루팅 (`REAL_STEPS.root`, `root-write`)

- [ ] Magisk APK 다이제스트·인증서 핀(`b4cb83b4…`) 검증, 오프라인 캐시 사용
- [ ] boot_patch.sh 실행(종료 코드 판정), new-boot.img stat 크기·수신 길이·헤더 검사 통과
- [ ] 패치 이미지 양 슬롯 기록 → OS 부팅 → Magisk 앱 설치 → su 승인(root_check)
- [ ] 언루팅: 순정 이미지 양 슬롯 재기록 후 정상 부팅·루팅 해제 확인

## 7. fastboot (`REAL_STEPS.fastboot`, `fastboot-write`)

- [ ] Windows 드라이버 바인딩·rusb open/claim·재연결, getvar:all 형식, is-userspace
- [ ] **응답 대기**: `oem unlock`(초기화)·`flash:` 응답이 300초 안에 오는가, 10초 이상 걸리는 실제 시간
  - INFO/TEXT가 계속 와도 종결 응답 전체 제한(일반 10초·언락/기록 300초)이 늘어나지 않는가. DATA 송신 전체 600초·개별 bulk 최대 60초가 정상 기록에 충분한가
- [ ] 언락 후 unlocked 조회 시점·자동 재부팅·USB 분리
- [ ] 기록: DATA 전송·양 슬롯 기록·이력(started/done/failed), 중간 분리 시 이력과 기기 상태
- [ ] 재부팅 OS/bootloader 전환과 wizard 다음 단계 진행
- [ ] 리락: 실제 리락은 코드에서 항상 차단 — 출처·AVB·부트 체인 증명 구현 전까지 해제하지 않음

## 8. 업데이트 확인·최종 확인 (`REAL_STEPS.verify`)

- [ ] 최종 확인: `root_reboot("os")` → 끊김(60초)·재연결(5분) 감지 → VoLTE 등록 자동 감지(imsReady)
- [ ] 업데이트 확인: 펌웨어 기록 실전 구현 후, 업데이트 직후 첫 부팅 시간(15분 대기 충분한지)·ro.build.id·지문 대조
- [ ] IMS 판정(TelephonyDebugService 필드)이 VoLTE·Wi-Fi 통화·미등록을 구분하는가

## 9. PC 환경·앱

- [ ] **DIAG 드라이버 감지** (`env_check`): qcser 미설치 PC에서 안내가 뜨고, 설치 후 [다시 확인]으로 사라지는가
- [ ] 폰을 DIAG 모드로 바꿨을 때 실제 USB ID(VID 05C6?·PID)와 qcser가 그 포트에 붙는지 — 감지 기준(qcser 패키지)이 맞는지
- [ ] 백업 파일 삭제(`backup_delete`): 실제 백업 폴더 삭제, 탐색기·백신이 파일을 잡고 있을 때 실패 후 재시도
- [ ] 작업 중 PC 보호(절전·종료 방지) 켜짐/해제, 창 닫기·Windows 로그아웃 중 진행 기록 보존
- [ ] 쓰기 기능을 켠 release 빌드(`--features fastboot-write,root-write`)로 실제 앱 실행

## 10. 네이티브 EFS/NV (`REAL_STEPS.efs`, `efs-write`)

- [ ] qcser 드라이버와 명시적 COM이 선택한 Sony ADB serial의 같은 폰을 가리키는지 확인. 포트 자동 추정 없음. 다른 폰·여러 포트·포트 점유는 명확한 오류로 중단하는가
- [ ] Magisk su 승인 → ADB DIAG 전환 → COM 38400/8N1 → hello/query/auth/suppression → EFS 초기화 순서가 실제 펌웨어에서 동작하는가. 7000ms 교환 제한이 적절한가
- [ ] 8개 balance 프리셋 각각의 파일·item PUT(원본 할당 padding·10진 flags/mode)·짧은 NV를 실제 폰에서 리드백 비교. 슬롯별 스냅샷 → 2회 업로드 → 전체 활성 대상 확인 순서 검증
- [ ] KT의 빈 NV 6789/6849 미변경 경고 및 알 수 없는 짧은 NV prefix 검증 경고가 UI·결과에 남는가. 미검증 tail이나 빈 NV를 검증 완료로 주장하지 않는가
- [ ] 서로 충돌하는 혼합 통신사 글로벌 NV는 어떤 기기 작업보다 먼저 차단되는가. 지원 가능한 조합의 동작은 별도 확인(충돌을 자동 덮어쓰지 않음)
- [ ] 스냅샷이 기존 EFS 내용·mode/type·기록된 시간·없는 대상·raw 128B NV를 정확히 보존하는가. 공간 부족·취소·USB 분리 시 incomplete로 남고 복원 입력으로 거부되는가
- [ ] 명시적 복원: EFS 파일·item·raw NV 복구, 새 대상 제거, mode/type·내용 리드백. 여러 슬롯 복원은 스냅샷 생성 역순. 시간 재적용 및 생성한 부모 폴더 제거는 미지원임을 확인
- [ ] 손상·잘린 응답·분리·취소 후 자동 재전송 없음, 소유한 descriptor 정리 실패가 오류에 포함되는가. 결과 불명 상태를 성공으로 처리하지 않는가
- [ ] 백업·복구·fastboot·Magisk·EFS 동시 실행 차단, 중단·재시도·창 닫기 및 PC 보호 유지가 실제 지연 I/O 중에도 일관적인가
- [ ] persist.dbg 4종이 모두 성공한 뒤에만 재부팅하고 끊김→재연결을 확인하는가. efs-write 단독 빌드의 OS 재부팅·실전 최종 확인, bootloader 재부팅 거부 확인
- [ ] 실제 IMS 셀룰러 등록 및 사용자 발신·수신 확인. EFS 리드백·속성 설정 성공과 통화 검증이 구분되는가
- [ ] 모든 쓰기 기능을 켠 검증용 release(`--features fastboot-write,root-write,efs-write`)에서 선택한 실전 플래그 조합을 확인. 기본 release는 모든 쓰기 기능·REAL_STEPS 꺼짐 유지
