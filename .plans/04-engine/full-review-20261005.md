# 전체 병합 후 코드 리뷰 — 2026-10-05

후속 정책: 리락 무조건 차단 및 II/IV 일괄 거부는 [기종별 워크플로우 재검토](model-workflow-recheck-20261005.md)에서 대체했다. 현재 리락은 복원 조건 검사 뒤 실행 가능하며, 일반 EFS는 외부 작업 안내와 함께 진행한다. 아래는 이 후속 수정 전의 리뷰 기록이다.

대상: 기존 통합 브랜치 `fix/full-review`의 `371d063`와 네이티브 EFS 브랜치 `feat/efs-native`의 `d90acdb`. 기존 driver-check·verify·backup·root·fastboot 수정 이력을 보존하고 EFS를 병합한다. 실행 파일 래퍼 대신 앱 내장 Rust COM/HDLC/DIAG/EFS/NV 모듈을 사용한다.

사용자 범위: 코드 전체의 일관성·오류·함수 분리·원본 자동화 순서·최적화를 재검토하고 발견 사항을 수정한다. 실제 폰/COM/USB 테스트는 제외한다. 기본 `REAL_STEPS`와 Cargo 쓰기 기능은 모두 꺼짐을 유지한다.

## 수정한 발견 사항

| 문제 | 수정 및 근거 |
| --- | --- |
| EFS 자체 잠금만 있어 다른 엔진의 기기 작업과 동시에 실행될 수 있음 | 기존 `device_io::WriteOperation`을 EFS의 RAII 소유권에 포함. 취소·descriptor 정리·포트 닫힘까지 유지. 상호 배제 양방향 테스트 |
| 전체 프리셋 충돌 검사가 언락·루팅 뒤에 있음 | 패치 계획 맨 앞에 기기 접근 없는 `efs-input` 추가. 설정·고정 해시·공유 NV/EFS 충돌·실전 엔진 조합 검사. 충돌·모의 선행 엔진·필요한 Rust 쓰기 feature 누락은 언락 전에 실패 |
| 루트 권한 안내가 DIAG 전환보다 뒤에 있음 | `su-grant`를 `efs-preflight`의 사전 수동 단계로 이동 |
| 정상 프리셋 내용을 다른 통신사/슬롯 이름으로 바꾸어도 허용될 수 있음 | 허용 목록을 폴더 이름과 해당 SHA-256 쌍으로 검사. 원본 SKT 내용을 KT 이름에 복사한 임시 폴더를 거부하는 실제 파일 테스트 |
| PC 설정을 실행 중 다시 읽어 COM·프리셋 루트가 바뀔 수 있음 | 실행별 설정 고정, facade에 선택적 설정 전달, native resolver에도 명시적 설정 검증. 단계별 프리셋 내용 재검사는 유지 |
| 설정·스냅샷 저장 구현이 기존 공통 저장 방식과 불일치 | 공통 bounded read/atomic write 사용. 설정 16 KiB·manifest 4 MiB 제한, snapshot capture/restore 모두 128 MiB 제한. 완료 manifest 교체 실패 시 incomplete 보존 |
| 로컬 manifest 파싱·해시가 async 명령 실행 스레드를 점유 | 공통 `offline()` blocking worker로 옮기고 native 구조화 오류 유지 |
| VoLTE 속성 명령 실패·기존 연결을 정상 재부팅으로 오인할 수 있음 | 4개 속성 각각 종료 코드 확인 후 재부팅. 연결 끊김(60초)→재연결(5분)을 모두 확인. 기존 최종 IMS 확인 로직과 공통 재연결 함수 재사용 |
| EFS가 실전이어도 마지막 확인이 모의 엔진으로 끝날 수 있음 | `final-verify`는 verify 또는 EFS 플래그 활성 시 실전 경로. efs-write 단독 빌드에는 OS 재부팅만 허용 |
| 중단 직후 재시도·재패치·USB 재개가 이전 I/O 완료 전에 상태를 초기화 | 공통 재개 검사 함수를 사용해 busy 동안 상태 유지. 늦은 실행 결과와 이중 실행 회귀 테스트 |
| fastboot INFO/TEXT마다 응답 제한 시간이 다시 시작됨 | 종결 응답 전체 deadline 적용(일반 10초·언락/기록 300초). 부분 DATA 전송에도 전체 600초 제한. libusb의 timeout=0 무제한 호출 방지 |
| 실제 EFS 설정을 입력하는 화면 부재 및 가짜 연결 검사 설명 | 플래그가 켜진 패치 계획에서 COM·번들·스냅샷 설정 표시, 미저장 시 실행 차단. 세부 작업을 실제 DIAG/프로토콜/응답 검사와 일치시킴 |

## 구조·원본 자동화 비교

Rust는 transport → HDLC → wire → session/device → manifest/engine → command 계층을 유지한다. 기기 쓰기/종료 코드/전역 잠금, 원자 파일 저장, APK 검증, 부트 이미지 검증은 기존 공통 구현을 재사용한다. 프런트는 API facade → 실행 스토어, 계획·journal·verify 순수 도메인 함수, 설정 UI로 나눈다. 재연결·재개·EFS 입력 검사 중복은 함수로 합쳤다. 무관한 모듈 재작성은 하지 않았다.

원본 `src/efs.py`·`src/adb.py`의 언락→루팅→DIAG→SIM1 2회→SIM2 2회→4개 persist.dbg 설정→재부팅→언루팅/리락 순서를 대조했다. 선택 슬롯을 오름차순으로 처리하고 2회 업로드를 유지한다. 입력 검사를 앞당기고 슬롯별 쓰기 전에 scoped before-image, 이후 전수 리드백을 추가했다. 통신 확인은 언루팅/리락보다 앞에서, 최종 확인은 복구 뒤에 한다. 실패하면 다음 쓰기 단계로 넘어가지 않는다.

원본 `-v`는 numeric NV 처리이고 verbose가 아니다. decimal flags/mode, PUT 할당 padding, 짧은 NV 요청 바이트를 보존한다. 실제 동봉 C# DLL과 비교한 13개 요청·45개 NV 크기·4개 상수 골든 증거는 [EFS 구현 문서](efs-native.md)에 유지한다. `.NET`은 개발용 골든 재생성에만 사용한다. 앱에서 adb/EfsTools 실행 파일을 호출하지 않는다.

최적화는 실행 스레드에서 파일 파싱·해시 제거, 전체 NV 스캔 대신 확정 대상만 읽기, 크기·시간 제한, 부분 전송 처리, 파일 진행 이벤트 중복 로깅 제거에 적용했다. 실패 판단·원본 2회 업로드·리드백을 성능 이유로 생략하지 않았다.

## 검증 결과

- 프런트 회귀 테스트: 74/74 통과.
- Svelte/TypeScript 검사: 오류 0·경고 0. 프로덕션 정적 빌드 통과.
- Rust `cargo clippy --all-targets --all-features -- -D warnings` 통과.
- Rust 기본 기능 / 모든 기능 각각 198 통과, 8 opt-in 테스트 제외. 새 회귀는 실제 전송 대신 가짜 COM/ADB/fastboot 사용.
- 원본 balance 8개 실제 폴더·해시·723개 파일 검사 통과. 이름 교체 거부·혼합 통신사 충돌도 확인. 원본 변경 없음.
- 공식 APK 실제 파일 인증서 핀 2개 검사 통과. GitHub 공식 최신 릴리스 API digest/size와 기존 캐시의 바이트를 대조해 Magisk v30.7 및 sms-ie v2.11.1 일치 확인.
- Sony 서버 실제 네트워크 테스트 통과: XQ-DQ44 67.2.A.3.178, 31개 ZIP 항목, 1,887,394 B 부분 다운로드, 8 MiB init_boot 추출·지문 일치. 기기 접근·이미지 저장 없음.
- Windows 드라이버 저장소 읽기 및 일시적 절전 방지 등록/해제 opt-in 테스트 통과. 다른 앱의 절전 방지 요청이 있어 전후 시스템 비트는 모두 0x1였으며, 본 테스트 요청 해제는 성공했다. 종료 방지 창 메시지 및 실제 기기 동작 검증을 대신하지 않는다.
- 모든 기능 Rust 라이브러리 빌드 통과. 기존 실행 중인 debug exe가 파일을 점유해 전체 debug 바이너리 덮어쓰기는 하지 않았다. 기본 기능 release 및 NSIS 패키지 생성 통과(약 3.24 MiB). 실행·설치는 하지 않았다.

## 남은 구현 범위와 실기기 검증

리뷰로 확인된 결함을 고쳤지만 실기기 호환성을 보증하지 않는다. 원래 미구현이던 전체 펌웨어 flasher는 여전히 모의 구현이며, 그 단계를 포함한 실전 EFS 계획은 첫 검사에서 차단된다. 리락은 순정 출처·AVB·전체 부트 체인 증명 구현 전까지 항상 거부한다.

서로 다른 값의 글로벌 NV를 포함하는 혼합 통신사는 지원하지 않는다. KT 빈 NV는 미변경 경고, 알려지지 않은 짧은 NV는 명시한 prefix만 검증한다. 스냅샷은 적용 대상 before-image로 전체 모뎀 파일시스템 백업이 아니다. 롤백은 명시적으로 요청해야 하며 여러 슬롯은 생성 역순으로 복원한다. 시간 값 재적용·새 부모 폴더 제거는 미지원이다. COM과 실제 폰의 대응 관계·드라이버·IMS·통화는 [단일 실기기 체크리스트](device-test-checklist.md)에서 확인해야 한다.

## 공식 자료

- [AOSP fastboot 프로토콜](https://android.googlesource.com/platform/system/core/+/refs/heads/main/fastboot/README.md): INFO/TEXT는 중간 프레임, 종결 응답 및 DATA 길이 검사 근거. 우리 deadline은 앱의 유한 대기 정책이며 표준이 지정한 값이라고 주장하지 않는다.
- [Rust std::fs::rename](https://doc.rust-lang.org/std/fs/fn.rename.html): 대상 파일 교체 동작. 기존 공통 atomic writer를 재사용하고 Windows 실제 임시 폴더 교체 테스트로 확인했다.
- [Magisk 공식 릴리스 API](https://api.github.com/repos/topjohnwu/Magisk/releases/latest), [sms-ie 공식 릴리스 API](https://api.github.com/repos/tmo1/sms-ie/releases/latest): 자산 이름·출처·digest·size 확인.
