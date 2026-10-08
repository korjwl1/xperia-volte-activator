# Newflasher 네이티브 구현 진행 기록

## 2026-10-08 루트 입력 문서 후속

`root-method.md`·`for-rooted-phone.md`를 읽고 사용자 답변 범위대로 수동 ReSukiSU 전환·모듈 도구를 추가했다. [구현 기록](root-tools.md), [현재 설명](../../docs/structure/root-tools.md), [동봉 자료 출처](../../src-tauri/assets/root/README.md).

Newflasher 쪽에는 PC-only `firmware_update_root_plan`을 연결했다. stock 업데이트에 루트/언락을 요구하지 않고, Magisk 유지·기존 언락·같은 폰의 목표 버전 패치 요건과 백업 후 수동 진행 정책을 반환한다. 항상 writeReady=false이며 실제 업데이트/백업 선택 화면/목표 IMG 사전 패치·하드웨어 진입 연결은 남아 있다. 이번 구현의 엔진 전환은 현재 버전 이미지이며 이 제한을 전역 완화하지 않았다.


기준일: 2026-10-08. 브랜치/워크트리: `newflasher-add`. 기준 커밋: `7bfd38d`. 전체 설계는 [native 계획](newflasher-native.md)을 따른다. 메인 작업 폴더의 미커밋 백업/ADB/EFS 변경을 가져오지 않았다.

## 구현한 범위

| 모듈 | 현재 동작 |
|---|---|
| `flasher/package.rs` | 로컬 폴더·update.xml 지문·NOERASE·모든 파일 해시·SIN 목록 검사. 원본 수정/추출/다운로드/기기 통신 없음 |
| `policy.rs` | 입력 TA, modem/DSP, userdata/metadata/persist 등 보존; 내부/외부 대상 불일치 거부; 알 수 없는 대상과 layout 후보 차단 |
| `sin.rs` | raw/gzip TAR의 CMS → `.000/.001/...` 검사·스트리밍 해시. 링크·순서/크기·checksum·gzip CRC·후행 데이터 검사 |
| `protocol.rs` | S1 DATA/OKAY/FAIL/INFO, legacy signature 또는 download→signature, partial OUT·전체 deadline·상한. 실패 자동 fallback 없음 |
| `engine.rs` | 파일/멤버 재검증 후 세션 TA flag·서명·조각 다운로드·최초 erase·매 조각 flash·세션 종료·Sync. preselected target만 받음 |
| `engine.rs::FileJournal` | intent/ACK를 기존 atomic-write로 저장. ACK 저장 실패·연결 끊김의 unmatched intent는 불확정 증거로 유지 |
| `transport/windows.rs` | SetupDi 경로 열거, GordonGate 방식 OVERLAPPED 읽기/쓰기, 취소 뒤 완료 대기. 컴파일만 확인; 장치에 연결하지 않음 |
| Tauri/facade/CLI | `firmware_package_inspect`만 등록. 프론트 타입과 `api.firmwarePackageInspect` 연결, 브라우저에서는 mock 성공 없음 |

패키지의 include는 **검증할 후보**이고 실제 기록 허용이 아니다. `writeReady=false`가 항상 반환된다. 지역·동일 기기·슬롯·layout·boot delivery가 검증되지 않았다는 blocker를 표시한다. 모델/지역을 아직 입력받거나 확인하지 않으며, targetFingerprint도 사용자가 요청한 대상과 파일 메타데이터의 일치만 확인한다.

현재 API는 준비 ID/실행 계획을 만들지 않는다. 전체 취득/스테이징, 기기 profile, 모드 간 identity 연결, 재부팅/부팅 검증, 목표 이미지 사전 패치와 기존 러너 연결은 다음 구현 범위다. 기존 `fw-flash` 실전 차단과 `default=[]`/REAL_STEPS 비활성을 유지한다. 아직 `firmware-write` feature나 하드웨어 플래시 invoke를 추가하지 않았다.

Windows 취소는 CancelIoEx 요청 뒤 GetOverlappedResult로 실제 완료를 기다린다. 드라이버가 취소 완료를 늦추면 반환도 늦어진다. deadline 도달을 I/O 종료로 표시하거나 버퍼/핸들을 먼저 해제하지 않는다. 이 동작과 GordonGate 드라이버 호환성은 실기기/드라이버 검증이 필요하다.

전송 runner는 내부 코어다. 검증된 device/profile/plan gate와 공통 `WriteOperation`/PC 보호를 결합한 하드웨어 진입점은 아직 없다. 따라서 이 코어를 임의 장치/target 목록으로 외부 호출하는 API를 추가하면 안 된다. 패키지 검사 보고서를 직접 쓰기 증명으로 사용하지 않는다.

## 원본 차등 검증

- 기준 C SHA-256과 커밋은 [NOTICE](../../src-tauri/src/flasher/NOTICE.md)에 유지한다. NOTICE는 앱 bundle resource로 포함한다.
- `scripts/newflasher-reference.mjs`는 해시가 같은 원본 파일에서 `process_sins`, `get_reply`, TAR helpers 등을 추출한다. USB 연결 함수·main·드라이버 코드는 가져오지 않는다.
- 테스트 C transport만 생성하고 합성 USTAR(CMS 내용 `TEST-CMS`, image 내용 `first/second`)를 전달한다. C harness를 MSVC로 컴파일·실행했고 `fixtures/upstream.json`에 modern/legacy OUT bytes를 저장했다.
- Rust runner의 서명·payload·erase·매 chunk flash 바이트를 이 fixture와 비교한다. C의 `getvar:has-slot:boot` 한 번은 slot 선정 위치의 의도적 차이라 비교에서 별도로 확인/제외한다. Rust target은 실행 이전에 고정해야 한다.
- 이는 이미지 전송의 작은 사례에 대한 차등 검사다. boot delivery·전체 패키지 순서·양 슬롯 특수 처리·partition delivery·내부 TA 읽기/다른 명령의 동등성을 증명하지 않는다.
- 합성 서명은 Sony 서명 검증 테스트가 아니다. Sony 펌웨어/드라이버는 저장소에 넣지 않았다.

재생성 순서:

1. 고정 원본 `newflasher.c`를 저장소 밖에 준비한다.
2. `node scripts/newflasher-reference.mjs <source> <새 절대 임시 폴더>`를 실행한다.
3. MSVC 개발 셸에서 생성 `reference.c`를 `cl /D_CRT_SECURE_NO_WARNINGS`로 컴파일한다.
4. 생성 폴더를 cwd로 두고 `reference.exe boot_X-FLASH-ALL-test.sin modern` / `legacy`를 실행한다.
5. `TRACE `로 시작하는 hex OUT 목록을 fixture와 대조한다. C 출력은 합성 데이터만 사용한다.

## 오프라인 검증

- Rust 기본 feature 전체 테스트 312개, 기존 쓰기 feature + dev-cli 전체 테스트 328개가 통과했다(각 10개 ignored). 이후 자원 상한/INFO 폭주 사례를 추가한 flasher 전용 테스트 22개도 통과했다. 실기기/네트워크 opt-in 테스트는 실행하지 않았다.
- 새 Rust 테스트는 다중 조각/gzip, CMS/순서/길이/checksum/CRC/링크/자원 상한, 보존/경로/지문/XML, partial OUT, 서명 방식과 원본 비교, malformed DATA/INFO 폭주, 변경된 파일/전송 상한, journal 실패/disconnect/cancel/내구성을 확인한다.
- 프론트 기존 회귀 테스트와 새 검사 facade 테스트를 포함한 `pnpm.cmd test` 142개, `pnpm.cmd check` 오류/경고 0건을 통과했다. 새 테스트는 브라우저 성공 위장 방지, blocker 전달, native 오류 보존을 확인한다.
- `cargo fmt --check`에는 기존 파일의 형식 차이가 있어 전체 저장소를 재포맷하지 않았다. 새 flasher Rust 파일만 rustfmt를 적용/검사한다.

PC-only CLI를 쓰기 feature 없는 `dev-cli` 빌드로 만들고 합성 패키지/임시 기록 폴더에 실제 호출했다. 종료 코드 0, 검사 결과 `writeReady=false`, CLI 실행 기록을 확인했다. `done`/종료 코드 0은 검사 호출 완료일 뿐 `writeReady`가 true라는 뜻이 아니다.

## 남은 통합 순서

1. 실제 패키지 XML/SIN 변형과 기종별 boot delivery·storage/slot selection을 읽기 자료로 확인하고 strict parser/profile/plan을 작성한다. 현재 parser가 거부하는 형식을 묵시적으로 허용하지 않는다.
2. 전체 펌웨어 취득·immutable staging·준비 ID/계획 해시·같은 기기 연결을 구현한다. 현재 검사 API는 개발용으로 유지한다.
3. 공통 실행권·PC 보호·쓰기 feature/capability·기기/profile gate를 결합한 하드웨어 진입점과 안전한 취소/상태 계약을 작성한다. 차등 fixture를 전체 명령/실패 사례로 늘린다.
4. 기존 업데이트 러너·백업 뒤 대기·목표 이미지 준비·순정 기록 증명·fastboot 후속 기록·OS/root/IMS 확인을 연결한다.
5. [실기기 체크리스트](device-test-checklist.md)의 profile별 항목을 별도 세션에서 검증한다. 미검증 상태로 쓰기를 켜지 않는다.
