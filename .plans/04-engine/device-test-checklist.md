# 실기기 검증 체크리스트

- [ ] 2026-10-06 직접 USB SYNC 세션 재사용: 앱 데이터 완료 후 DCIM의 동일 샘플 ABBA 전송 비교, 매회 PC 크기·해시 및 회차간 내용 일치. 실제 세션 재사용 여부와 측정 시간 기록.
- [ ] 2026-10-06 수정한 세션 재사용·버퍼 저장 엔진으로 DCIM/fs-rest 완료 및 전체 선택 항목 PC 해시 검증. 샘플 비교만으로 전체 백업 완료 주장 금지.

통신 진단 추가 검증(2026-10-05, 미실행):

- [ ] 지원 기기·펌웨어의 구형/신형 IMS 덤프와 전화 앱 IMS 화면 결과를 대조한다. Wi-Fi/NR/다른 SIM 경유 및 기술 미확인 표기를 확인한다.
- [ ] SIM 없음·PIN 잠김·등록 중·미등록·권한 거부/서비스 미지원에서 사유가 맞고 이전 정상 배지가 남지 않는지 확인한다.
- [ ] 작업 전/완료 후 읽기 전용 진단과 SIM 변경 재확인이 설정·발신·재부팅·EFS 기록을 수행하지 않는지 확인한다.
- [ ] 슬롯별 사용자 발신·수신·양방향 음성과 재부팅·대기 후 유지 체크 및 done 기록 갱신을 확인한다.

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
- [ ] 재개 중 같은 파일을 다시 받다가 끊겨도 기존 검증된 사본이 유지되는가. 원본 파일이 폰에서 사라진 경우에도 이전 성공 사본을 유지하고, 선택 항목 축소·재선택의 완결 판정이 맞는가
- [ ] 백업 취소 토큰: 준비 직후 중단, 실행 중 중단이 해당 실행에만 적용되는가

## 3. 문자·통화 (sms-ie)

- [ ] APK 다운로드 검증(다이제스트·인증서 핀 `c105e6d9…`)·설치·`pm grant`·기본 문자 앱 전환/원복
- [ ] **내보내기 파일명**: sms-ie가 실제로 `messages<날짜>.zip`·`calls<날짜>.json`을 만드는가(원본 소스 기준으로 고침, 실측 전) — 수집이 두 항목을 모두 인식하는가
- [ ] 선택한 항목이 모두 끝나면 기기 임시 폴더가 지워지는가
- [ ] 저장 위치 선택 직후 0바이트·작성 중인 ZIP/JSON은 완료로 받지 않는가. 폴링만으로 완료/삭제되지 않고, 선택 항목 모두의 폰 앱 내보내기 성공을 확인한 뒤 매니페스트 저장 → 폰 사본 삭제 순서를 따르는가
- [ ] 언락 초기화로 sms-ie가 없는 폰에서 복원 준비가 검증된 APK 설치·권한 부여·임시 폴더 준비를 수행하는가
- [ ] 복원: 파일 전송·가져오기·역할 원복, 비행기 모드 안내, 강제 종료 후 재연결 시 원복 기록 유지

## 4. 복구 (`REAL_STEPS.restore`) — 기기에 쓰기

- [ ] **exec 완료 대기** (`restore::exec_checked`): 기기 명령 출력 끝까지 기다린 뒤 종료 코드를 판정하는가
- [ ] **`tar -xf -` 종료**: adb_client exec가 stdin을 닫지 않는다 — toybox tar가 아카이브 끝 블록 뒤 스스로 끝나는가(아니면 300초 시간 초과 오류)
- [ ] tar 복원 결과: 원래 경로·mtime·긴 이름(GNU longname)·격리 파일이 올바른 위치에 복원되는가
- [ ] APK 재설치: split APK 세션(install-create/write/commit), 실패 시 abandon, 이미 설치된 앱 건너뜀
- [ ] 설정 화이트리스트·배터리 최적화 예외(`user,<pkg>,<uid>` 형식) 복원 후 실제 값
- [ ] 대용량(수십 GB) 복원 시간·진행 표시·USB 쓰기 30초 상한 여유
- [ ] **USB 직접 연결 재사용**(2026-10-05): 재부팅·케이블 재연결 뒤 첫 명령이 살아 있는지 확인(`true`) 후 한 번 다시 연결되는가, 이때 쓰기 명령이 두 번 실행되지 않는가
- [ ] **USB 디버깅 허용 대기**: adb 서버 없이 미승인 폰을 연결하면 오류 대신 "unauthorized" 자리표시·허용 안내가 뜨는가(vendor 패치 UNAUTHORIZED_MARKER)
- [ ] adb 서버가 떠 있지만 오류를 낼 때 USB로 넘어가지 않고 서버 오류가 표시되는가

## 5. 펌웨어·부트 이미지

- [ ] Sony 서버 부분 다운로드(Range 206)·조각 목록(연속 번호)·ZIP64
- [ ] **부트 이미지 헤더 검사** (`boot_image::validate`): 실제 init_boot(v4)·boot(v2/v3) 순정 이미지가 통과하는가, 잘린 파일이 거부되는가
- [ ] `.sin` 안 이미지 조각 구성(.000 하나인가) — 실제 펌웨어에서 오탐 없음
- [ ] update.xml·`.sin` 유일성 규칙이 실제 펌웨어 ZIP 구조에서 오탐 없이 동작하는가
- [ ] 직접 지정 펌웨어 폴더 검사·캐시 추출
- [ ] **출처 기록**(2026-10-05): 받은·추출한 이미지 옆 `<이미지>.json`이 생기고, 기록 없는 예전 캐시는 "펌웨어를 다시 받아 주세요"로 안내되는가
- [ ] **이미지 종류 검사**: 실제 init_boot(헤더 v4·커널 없음)·boot(커널 있음) 순정 이미지가 `check_fits_partition`을 통과하는가

## 6. 루팅·언루팅 (`REAL_STEPS.root`, `root-write`)

- [ ] Magisk APK 다이제스트·인증서 핀(`b4cb83b4…`) 검증, 오프라인 캐시 사용
- [ ] boot_patch.sh 실행(종료 코드 판정), new-boot.img stat 크기·수신 길이·헤더 검사 통과
- [x] 2026-10-07 XQ-DQ44(67.2.A.3.178, CLI 단계별): 순정 init_boot 부분 다운로드(1.9MB) → 기기 대조 → Magisk v30.7 패치(8 MiB, 해시 상이) → **fastbootd**에서 init_boot_a/_b 기록 OKAY → OS 부팅 25초 → Magisk 앱 설치 → `magiskd` 실행·`30.7:MAGISK:R` → su uid=0(`u:r:magisk:s0`).
  - 부트로더 fastboot는 `flash:init_boot_a`를 `Flashing is not allowed for partition`으로 거부한다 → 기록은 fastbootd(원본 도구와 같음).
  - fastbootd USB ID는 `18D1:4EE0`(드라이버 없음) → Sony INF를 같은 방식으로 자동 지정했다(UAC).
  - 부트로더는 `has-slot:init_boot`에 "Variable Not found" → `partition-size:init_boot_a/_b`로 판정. fastbootd는 `has-slot:init_boot=yes`.
  - fastbootd `getvar:all`은 347개 변수로 INFO 256 상한을 넘었다 → 상한 4096. 끊긴 응답이 남아 fastbootd가 멈췄다 → 장치를 열 때 남은 응답을 비운다.
  - Magisk 30.7은 `/system/bin/su`가 없고 `/debug_ramdisk/su`만 있다 → su 명령은 PATH에 없으면 그 경로를 쓴다.
- [ ] GUI 루팅 단계 전체(fastbootd 진입·드라이버·기록·복귀·su 승인 화면 깨우기/잠금 대기/거부 안내) 재실행 검증 — 다음 루팅 기기에서
- [ ] 언루팅: 순정 이미지 양 슬롯 재기록(fastbootd) 후 정상 부팅·루팅 해제 확인
- [ ] Magisk APK 신뢰 범위: `magisk_prepare`가 받은 캐시(magisk/ 안, 다이제스트 기록 일치)로만 패치·설치되는가
- [ ] 패치 결과의 출처 기록에 순정 부모 이미지 해시·파티션·펌웨어 지문이 남고, 동일 기기의 검사 기록으로 이후 플래시가 통과하는가

## 7. fastboot (`REAL_STEPS.fastboot`, `fastboot-write`)

- [ ] Windows 드라이버 바인딩·rusb open/claim·재연결, getvar:all 형식, is-userspace
- [ ] **응답 대기**: `oem unlock`(초기화)·`flash:` 응답이 300초 안에 오는가, 10초 이상 걸리는 실제 시간
  - INFO/TEXT가 계속 와도 종결 응답 전체 제한(일반 10초·언락/기록 300초)이 늘어나지 않는가. DATA 송신 전체 600초·개별 bulk 최대 60초가 정상 기록에 충분한가
- [ ] 언락 후 unlocked 조회 시점·자동 재부팅·USB 분리
- [ ] 기록: DATA 전송·양 슬롯 기록·이력(started/done/failed), 중간 분리 시 이력과 기기 상태
- [ ] 재부팅 OS/bootloader 전환과 wizard 다음 단계 진행
- [ ] 조건부 리락: 현재 OS 지문/추출 이미지 대조 및 양 슬롯 최신 복원 이력이 통과한 뒤 동일 fastboot serial·bootloader·슬롯 확인 → oem lock → 명시적인 unlocked=no → OS 재부팅/초기화 후 정상 부팅을 단계별 확인한다. 응답/상태 조회 중 재열거·자동 재부팅의 실제 동작 확인
- [ ] 리락 거부 조건: 추출 출처 없음/변조·펌웨어 불일치·슬롯 누락/미완료 이력·다른 기기·fastbootd에서 잠금 명령을 보내지 않는가. 외부 모뎀·커널·vbmeta 변경은 앱의 검증 범위 밖이다
- [ ] OS 기기 조회가 펌웨어 변경을 관찰하면 이전 이미지 대조 기록을 거부하는가. 손상 플래시 이력 보관 후 자동 잠금 없이 순정 양 슬롯 복원 단계로 돌아가는가
- [ ] 인식 기종별 워크플로우: boot/init_boot 자동 선택, IV 개발 포트 보완, II/IV 일반 EFS 진행 및 조건에 맞는 외부 작업 안내. 사용자가 고른 SIM·통신사·업데이트 여부가 보존되는가

## 8. 업데이트 확인·최종 확인 (`REAL_STEPS.verify`)

- [ ] 최종 확인: `root_reboot("os")` → 끊김(60초)·재연결(5분) 감지 → VoLTE 등록 자동 감지(imsReady)
- [ ] 업데이트 확인: 펌웨어 기록 실전 구현 후, 업데이트 직후 첫 부팅 시간(15분 대기 충분한지)·ro.build.id·지문 대조
- [ ] IMS 판정(TelephonyDebugService 필드)이 VoLTE·Wi-Fi 통화·미등록을 구분하는가
  - 등록 상태=2 + 음성 기능=true + 셀룰러 방식일 때만 on. 음성 기능만 있거나 필드가 누락되면 unknown으로 남는가
  - 최종 IMS 감지가 모달을 자동으로 닫지 않으며 통신 미확인과 파일 검증 결과가 구분되는가

## 9. PC 환경·앱

- [ ] 개발 CLI: 동일 엔진의 단계별 실행 → 오류 기록 → 수정·재빌드 → 필요한 명령만 재실행, 캐시/플래시 이력 보존. Android(ADB)/fastboot의 serial이 같은가 — CLI가 같지 않으면 거부하므로, 거부되면 앱 수정이 필요한 기종이다. 명시한 COM과 SIM 프리셋이 보존되는가
- [ ] CLI에서 다른 폰의 완결 백업·임의 이미지 해시가 거부되고 OS에서 대조한 이미지 또는 공통 Magisk 패치 결과만 플래시되는가. fastboot 중 빈 `device_list`는 이전 ADB 세션을 지우지 않는가
- [ ] 진행 기록 읽기·보관 실패 시 기존 기록이 덮어써지지 않고, 저장 실패 경고가 표시되는가. 실전 작업 화면에 시뮬레이션 스위치가 없는가
- [ ] GUI와 CLI 기기 작업 중복 시 공통 OS 잠금으로 중단되는가. CLI 실행 중 절전 방지·종료 후 해제, 강제 종료 시 running 기록과 재검사 후 수동 재시작을 확인한다

- [ ] **DIAG 드라이버 감지** (`env_check`): qcser 미설치 PC에서 안내가 뜨고, 설치 후 [다시 확인]으로 사라지는가
- [ ] 폰을 DIAG 모드로 바꿨을 때 실제 USB ID(VID 05C6?·PID)와 qcser가 그 포트에 붙는지 — 감지 기준(qcser 패키지)이 맞는지
- [ ] 백업 파일 삭제(`backup_delete`): 실제 백업 폴더 삭제, 탐색기·백신이 파일을 잡고 있을 때 실패 후 재시도
- [ ] 작업 중 PC 보호(절전·종료 방지) 켜짐/해제, 창 닫기·Windows 로그아웃 중 진행 기록 보존
- [ ] 쓰기 기능을 켠 release 빌드(`--features fastboot-write,root-write`)로 실제 앱 실행

## 10. 네이티브 EFS/NV (`REAL_STEPS.efs`, `efs-write`)

- [ ] 실전 계획에 미구현 전체 펌웨어 기록 또는 모의 기기 단계가 섞이면 첫 작업 전에 차단. 재개·완료된 EFS 입력 체크포인트로 우회할 수 없는가
- [ ] 부트 파티션 표에 없는 모델은 이미 루팅돼 있거나 SIM이 없어도 언락·DIAG 전 자동 패치 거부. 사용자 선택 통신사/슬롯은 그대로 유지되는가

- [ ] qcser 드라이버와 명시적 COM이 선택한 Sony ADB serial의 같은 폰을 가리키는지 확인. 포트 자동 추정 없음. 다른 폰·여러 포트·포트 점유는 명확한 오류로 중단하는가
- [ ] Magisk su 승인 → ADB DIAG 전환 → COM 38400/8N1 → hello/query/auth/suppression → EFS 초기화 순서가 실제 펌웨어에서 동작하는가. 7000ms 교환 제한이 적절한가
  - Mark IV XQ-CT*/XQ-CQ*: persist.usb.eng=1 설정·리드백 후 DIAG. USB 재열거로 셸 종료 상태가 잘리면 오류·재시도 안내로 남는가
  - SIM 미삽입·감지 통신망 불일치에도 사용자 선택대로 기록·리드백하는가
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

- [x] 2026-10-06 앱 준비: /sdcard/volte_sms_backup 생성 및 MainActivity ADB 실행. 실제 폴더·화면 확인 후 체크.
- [x] 2026-10-06 문자·통화: 사용자 성공 확인 후 날짜 파일 실기기 감지·ZIP/JSON·mtime·PC 해시 검증 및 수집 완료. 연락처 JSON 추가 보관은 후속 사용자 요청으로 제거했고 VCF는 유지한다. 중복 번호는 오프라인 테스트 통과. 성공 확인 전 완료/정리 불가 테스트 통과.
- [ ] 2026-10-06 복원: 같은 기기·해시·ZIP/JSON 검증 후 고정 폴더 전송·앱 실행, 실제 파일명과 Import Messages / Import Call Log 안내. 복원 실기기 실행은 승인되지 않았으므로 오프라인 테스트만.
- [ ] 2026-10-06 USB 대용량 재개: bulk 64 KiB, 정상 ZLP 허용·무진행 상한, WRTE/CLSE ACK·push 상태·트랜잭션 종료를 실제 백업 전송에서 확인.
- [ ] 2026-10-06 USB RECV 경계 독립 파서 수정 후 전체 선택 항목 파일 복사·PC 무결성 검사(21,933개 앱 데이터 폴더 조회는 성공, 이전 파서의 짧은 페이로드 패닉으로 미완료).
- [x] 2026-10-06 RECV 수정 후 실제 Documents/Music/Movies/Download 수신: 71개 파일 565,557,635바이트 별도 PC SHA256·크기 대조 일치. 전체 14개 완결 검증과는 별도이며 남은 항목 진행 중.
- [x] 2026-10-06 SIM 슬롯 가정 제거 후 같은 폰 읽기 조회: slot1 LOADED=physical, slot2 NOT_READY=unknown. 판별 불가 유형은 화면·기록에서 추측하지 않음. 기종을 바꾼 실기기 검증은 하지 않았음.
- [x] 2026-10-06 사진 샘플 세션 재사용: 동일 8개·66,710,218바이트씩 ABBA 4회 PC 크기·해시 및 회차 간 해시 일치. 동일 버퍼/동기화에서 per-file 30.10/30.63 MiB/s, batch 34.77/34.22 MiB/s. 전체 전송/USB 최대 대역폭 판정은 하지 않음.
- [ ] 2026-10-06 재연결: CNXN 뒤 늦은 이전 CLSE/WRTE/OKAY를 처리하는 수정 코드의 실기기 확인. 기존 코드의 전환 직후 CLSE 오류와 재시도 성공은 관찰했고, 수정 코드는 오프라인 회귀 5개 포함 vendor USB 단위 테스트 33개 통과. 현재 백업 중 강제 중단으로 오류를 재현하지 않음.
- [x] 2026-10-06 권한 거부 종료: 같은 Sony 폰의 기존 권한 거부 경로를 수정한 실행 파일로 읽기 전용 샘플 조회. SYNC RECV Permission denied는 유지되고 SYNC_BATCH_BROKEN/종료 순서 오류 없이 끝났다. 분할 FAIL 이후 같은 연결의 다음 파일 정상 수신·누락/다른 스트림 종료 응답 차단은 오프라인 회귀 2개 포함 vendor USB 35개 통과. 원래 권한 거부 파일을 성공으로 바꾸거나 권한 변경을 하지 않음. 전체 재개 백업과 최종 PC 검사는 별도 확인한다.
- [x] 2026-10-06 PC 병렬 해시: 기존 백업의 일반 파일 11개·1,073,722,564바이트를 같은 SSD 하드링크로 검사. release 순차/최대 4개 작업자, 캐시 예열 후 ABBA 4회 모두 complete·크기 일치. 평균 순차 0.681초, 병렬 0.217초(약 3.14배). Windows 캐시를 포함하는 표본이며 전체 전수 검사나 HDD에서 같은 배수를 보장하지 않는다.
- [x] 2026-10-06 전체 PC 병렬 해시 검사 완료. USB 복사 종료 뒤 PC 검사만 전환했고 194,630,644,037바이트 사본을 171.502초에 검사했다. 크기/해시 불일치 없음, 파일 2개·폴더 1개 원본 Permission denied는 app-data partial/전체 complete=false로 유지한다.
- [ ] 2026-10-06 파일 단위 재개 실기기: 정상 영수증의 재전송 생략, 크기/mtime 변경 및 손상 파일만 재전송, 기존 사본 보존·취소·진행 이벤트를 작은 표본으로 확인. 오프라인 회귀에서 정상 일반/tar pull 생략과 변경·실패·손상·시각 미확인 파일 재전송 및 최종 검증을 확인했으며, 이미 진행 중인 대용량 복사를 다시 시작하지 않는다.
- [x] 2026-10-06 기존 거부 경로 읽기 진단: 대상 기기 해시 대조 후 shell UID/그룹, 대상·부모 mode/UID/GID, head/ls로 원본·정규 경로를 확인했다. 파일 2개 mode 700, 폴더 mode 770 앱 소유로 shell 읽기 거부 조건 확인. 기기 권한/내용 변경 없음. 잠금/USB/PC 저장 공간 문제로 안내하지 않는다.

- [ ] 2026-10-06 단계별 대기 GUI: 실전 백업·후속 엔진 사이에 [다음] 전 실행 없음, 앱 재시작 후 제외 안내/대기 복원, 확인·Esc 후 자동 진행 없음. mock/실전 facade 오프라인 회귀는 통과했으며 전체 GUI 실기기 전환 테스트는 별도다.
- [x] 2026-10-06 기존 PC 백업의 권한 오류 앱 데이터 전체 정리: 원본 권한 오류 3개 패키지의 PC 앱 데이터 삭제 완료. 정상 영수증 243개·373,569,106바이트 및 이전 중단의 임시 파일 1개·30바이트 제거. 나머지 72,471개 파일의 영수증·크기·mtime·ctime·파일 id와 APK 12개 보존 확인. manifest omittedApps cleanupPending=false, 보관 범위 complete=true·194,257,074,931바이트. 기존 전수 해시 검사 증거를 유지하고 PC만 정리했으며 폰 복원/쓰기는 실행하지 않았다.
- [x] 2026-10-06 후속 연락처 JSON 제외: PC smsie/contacts 날짜 JSON 1개·3,391,180바이트 삭제 및 연락처 영수증/총량 갱신. VCF SHA-256·다른 항목 영수증·제외 앱 감사 기록 보존 확인. 현재 72,470개·194,253,683,751바이트. 손상된 연락처 JSON이 있어도 SMS 완결/VCF에 영향 없음 및 날짜·중복 번호·대소문자 파일 읽기 생략은 FakeADBDevice 회귀로 검증했다. 폰 데이터는 변경하지 않았다.

2026-10-06 review follow-up: new source attribute capture/GUI backupOnly/probe serialization/cancellable PC verify/updated APK/corrupt tar repairs have offline regression tests. Prior 194GB copy does not validate the new GUI entry or metadata restoration. No destructive live restore/unlock test is implied. Live metadata enrichment, when performed, must be marked after-copy-enrichment; unavailable birth time and unrecoverable past attributes remain explicit.

2026-10-06 최종 수정 검증 범위: 원본 속성 sidecar, GUI 백업만 실행, SMS probe/준비 충돌, tar 복구 및 제외 진단 정리, ADB 오류 후 정리는 오프라인 회귀 검증이다. 기존 D: 백업은 사용자 지시로 삭제됐다. 새 전체 실기기 백업, 재연결·강제 종료 재개, 실제 메타데이터 조회 지원값, 초기화 뒤 전체 복원은 아직 새 코드로 검증하지 않았다.

- [x] 2026-10-07 fastboot 사전 점검(XQ-DQ44): 부트로더 재부팅·getvar·OS 재부팅. `is-userspace=no`, `unlocked=no`, serialno=ADB 시리얼 확인. 폰 데이터 변경 없음.
- [x] 2026-10-07 Windows fastboot 드라이버 자동 설치: 판매명 "Xperia 1 V" → 공식 `xperia-1-v-driver` 다운로드·서명 드라이버 지정(UAC) → libusb로 getvar 성공.
- [x] 2026-10-07 실제 `oem unlock`(XQ-DQ44, GUI): 코드 수락 OKAY(즉시), 자동 재부팅 없음. **직후 `getvar unlocked`는 no**이고, `reboot-bootloader` 후 다시 읽으면 yes(6초 후 재연결). 엔진이 OKAY+no일 때 부트로더를 재시작해 재확인하도록 수정했다. 이후 OS 재부팅으로 초기화 진행.
- [ ] 수정한 엔진의 언락 경로(OKAY → reboot-bootloader → yes 확인) GUI 재실행 검증 — 다음 언락 기기에서 확인
