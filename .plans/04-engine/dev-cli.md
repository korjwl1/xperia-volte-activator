# 개발용 단계 실행 CLI

2026-10-05 구현. 실기기 통신은 이번 구현 과정에서 실행하지 않았다. 검토 대상은 README의 Xperia 1 V JP · XQ-DQ44이다.

## 같은 엔진을 사용한다

Cargo `dev-cli` feature를 켰을 때만 별도 `xva-dev` 실행 파일이 포함된다. `debug/release`는 최적화 프로필이고 개발 CLI 포함 여부는 feature로 정한다. CLI는 기존 라이브러리에 링크하며 Tauri 창·데몬·별도 adb/EfsTools 프로세스를 띄우지 않는다.

Tauri 명령은 기존 알고리즘을 `*_with_events`에 위임하고 CLI도 같은 함수를 호출한다. AppHandle이 없는 명령은 기존 명령 함수를 직접 호출한다. 진행 출력만 `events::Events`를 통해 앱 이벤트 또는 CLI JSON Lines로 전달된다. 이미지 검사·서명 핀·펌웨어 대조·양 슬롯 기록·EFS 리드백·리락 이력 판정 코드는 공유한다. 코드 수정 후 **재빌드한** CLI와 앱에는 같은 엔진 수정이 적용된다.

CLI는 개발자가 명시한 엔진 명령 하나를 실행하는 도구다. Svelte wizard의 전체 계획 생성·수동 확인·재연결 대기·백업 선행 게이트를 실행하지 않는다. GUI 단계 전환은 마지막에 앱에서 별도 검토한다. 미구현 전체 펌웨어 다운로드·기록 명령은 CLI에도 없다. 임의 셸 명령 실행은 제공하지 않는다.

## 실기기 세션 규칙 (먼저 읽기)

- **GUI 앱을 닫고 쓴다.** USB 직접 연결은 프로세스마다 인터페이스를 점유한다. 기기에 닿는 명령은 GUI(`xperia-volte-activator.exe`)가 실행 중이면 시작 전에 거부한다(PC 전용 명령은 허용).
- **한 흐름 안에서 CLI와 GUI를 섞지 않는다.** CLI는 `--data-dir`를 앱 데이터 폴더로 쓰므로 CLI로 한 플래시 이력·출처 기록·Magisk 캐시·진행 기록을 GUI가 보지 못한다(예: CLI로 기록한 뒤 GUI 리락 게이트는 이력이 없어 거부).
- **종료 코드 0·`done`은 그 명령이 오류 없이 끝났다는 뜻일 뿐이다.** 백업 `complete`, SMS `ready`, 검사 결과의 `ok`·`false` 같은 의미 필드를 반드시 직접 확인한다.
- **초기화 단계(`fastboot_unlock`·`fastboot_lock`)는 백업 증명이 필요하다.** `--backup-dir <완결 백업 폴더>`(앱과 같은 매니페스트 재검사로 `complete` 확인) 또는 백업 없이 진행한다는 `--ack-no-backup` 중 하나를 지정해야 하며, 결과는 기록의 `backupGate`에 남는다. 앱의 enforceBackupGate와 같은 선행 조건이다.
- **fastboot 기대값은 앱과 같은 기준으로 대조한다.** 앱은 ADB 기기의 serial을 fastboot `expectedSerial`로 넘긴다. CLI도 같은 `--data-dir`에서 마지막 `device_list`가 본 ADB serial(해시만 `dev-session.json`에 저장)과 일치해야 실행한다. 다르면 "앱에서도 거부될 단계"로 실패한다 — Android·fastboot 식별값이 다른 기종이면 앱 수정이 필요하다는 신호다.

## 빌드

저장소 루트, MSVC 개발 셸에서 실행한다. 일반 PowerShell에서는 Visual Studio Build Tools의 `vcvars64.bat` 환경을 먼저 적용한다.

```powershell
# 읽기·로컬 준비용: 쓰기 feature를 자동으로 켜지 않는다.
cargo build --manifest-path src-tauri/Cargo.toml --bin xva-dev --features dev-cli

# 실기기 검토용: 기기 쓰기 엔진까지 명시적으로 포함한다.
cargo build --manifest-path src-tauri/Cargo.toml --bin xva-dev --features dev-cli,fastboot-write,root-write,efs-write

# 같은 CLI를 release 최적화로 빌드할 수도 있다.
# 앱 라이브러리에 Tauri 정적 자산이 포함되므로 먼저 pnpm.cmd build로 build/를 만든다.
cargo build --manifest-path src-tauri/Cargo.toml --bin xva-dev --release --features dev-cli,fastboot-write,root-write,efs-write
```

debug 산출물은 `src-tauri/target/debug/xva-dev.exe`, release는 `src-tauri/target/release/xva-dev.exe`이다. `CARGO_TARGET_DIR`를 지정했다면 그 폴더 아래에 생긴다. 개발 모드 실행은 다음처럼 Cargo가 변경분을 다시 빌드하고 실행하게 하면 오래된 exe를 실행하는 실수를 줄일 수 있다.

```powershell
cargo run --manifest-path src-tauri/Cargo.toml --bin xva-dev --features dev-cli,fastboot-write,root-write,efs-write -- commands
```

일반 앱의 `default = []`, `REAL_STEPS`와 `SIMULATED_RUN`은 변경하지 않았다. `dev-cli` 자체에는 쓰기 feature 의존성이 없다. Cargo `default-run`은 기존 GUI 실행 파일이다.

## 명령과 입력

```powershell
$cli = '.\src-tauri\target\debug\xva-dev.exe'
$sessionDir = Join-Path $env:LOCALAPPDATA 'xva-device-test\xq-dq44'
& $cli commands
& $cli validate --request .\device-list.json
& $cli run --request .\device-list.json --data-dir $sessionDir --step baseline
& $cli history --data-dir $sessionDir
```

`commands`는 기기 접근 없이 명령별 `example`, `requiresDeviceWrite`, `enabledInBuild`와 빌드 feature 목록을 JSON으로 출력한다. 인자 이름은 Tauri와 같은 camelCase다. 예제를 복사해 값만 채운다. `validate`는 입력 구조만 검사하며 기기·이미지·COM을 검증했다고 주장하지 않는다.

`device-list.json`:

```json
{"command":"device_list","args":{}}
```

시리얼은 출력/기록에서 마스킹하며 각 기기의 `serialKey`(원본 시리얼 SHA-256)를 함께 제공한다. 다음 명령의 `serial` 또는 `expectedSerial`에 `sha256:<serialKey>`를 넣으면 현재 연결된 기기의 원본 식별값으로 해석해 **같은 기존 엔진에** 전달한다. 일치하는 기기가 없거나 다르면 실패하고 다른 폰을 자동 선택하지 않는다. 기존 코드에 필요한 원본 serial을 직접 제공해도 출력/기록에서는 마스킹된다.

```json
{"command":"root_check","args":{"serial":"sha256:<device_list의 serialKey>"}}
```

fastboot 명령의 `expectedSerial`은 `device_list`의 `serialKey`(ADB serial)를 그대로 쓴다. CLI는 이를 연결된 fastboot 기기의 serialno로 해석하고, 그 값이 마지막 `device_list`의 ADB serial과 같은지 확인한다(위 세션 규칙). 먼저 같은 `--data-dir`로 `device_list`를 실행해야 한다. EFS는 항상 작업자가 직접 확인한 `COM<number>`를 지정한다. CLI의 파일 경로/COM/선택한 SIM 프리셋은 작업자가 제공하며 자동 추정하지 않는다.

기기 변경 명령에는 실행마다 `--allow-device-write`가 필요하다. 폰 데이터가 초기화되는 `fastboot_unlock`·`fastboot_lock`은 추가로 `--backup-dir` 또는 `--ack-no-backup`이 필요하다(다른 명령에 주면 오류). fastboot는 입력의 기존 `confirm: true`도 그대로 요구한다. EFS의 초기화·읽기 명령도 DIAG 세션 설정을 수행하므로 변경 옵션을 요구한다. 쓰기 feature가 빠졌거나 옵션이 없으면 해시 selector 조회도 시작하지 않는다. 문자 준비·수집·복구도 기기 변경으로 분류한다. 백업은 폰에서 읽는 기존 엔진을 그대로 쓰며 PC에는 파일을 저장한다.

언락 코드가 든 요청은 공유·커밋하지 않는다. `--request -`로 stdin을 사용할 수도 있다. 입력 전체·원본 시리얼·언락 코드·IMEI는 실행 기록에 저장하지 않는다. 엔진이 반환하는 값·오류·진행 로그는 마스킹 후 저장한다. 요청 파일의 UTF-8 BOM도 지원한다.

## 실패 후 수정·재시작

한 번에 한 엔진 명령만 실행하고 다음 명령으로 자동 이동하지 않는다. 같은 `--data-dir`를 계속 지정하면 펌웨어/Magisk 캐시·패치 이미지·양 슬롯 플래시 이력·문자 앱 역할 원복 기록을 유지한다. 저장된 결과의 경로·SHA-256·펌웨어 지문을 다음 요청에 넘긴다. `--step`은 사람이 붙이는 기록용 이름이며 실행 순서나 성공 증명이 아니다.

1. `device_list`로 모델·현재 펌웨어·루트·SIM/IMS 상태를 기록한다.
2. 백업 필요 시 `backup_prepare` → `backup_run` 및 수동 문자 단계 → `backup_manifest_check`로 complete를 확인한다. `backup_run` 재개는 `resumeDir`와 같은 대상 기기를 명시한다.
3. `firmware_fetch` 또는 `firmware_dir_check` → `boot_image_check` → `magisk_prepare` → `magisk_patch`를 각각 검토한다. XQ-DQ44는 `init_boot`를 사용한다. 새 버전의 부트 이미지 다운로드는 전체 OS 업데이트가 아니다.
4. `root_reboot`의 bootloader 진입 → `fastboot_getvar` → 필요한 언락/이미지 기록 → `fastboot_reboot` → 실제 재연결 후 `magisk_install`·`root_check`를 검토한다. 언락/리락은 초기화 가능성이 있는 별도 작업이다.
5. 선택 슬롯들의 `efs_validate_presets`를 먼저 수행한다. 기기와 COM을 대조한 후 `efs_diag_open` → `efs_preflight` → **각 슬롯 snapshot 저장** → 원래 절차의 각 슬롯 2회 upload → 모든 선택 슬롯 verify → `volte_props_set` 순서로 검토한다. write 옵션을 붙였다고 스냅샷/순서가 자동 보장되는 것은 아니다.
6. `volte_props_set`는 네 가지 속성 적용 뒤 OS 재부팅까지 한다. 다시 실제 연결을 확인하고 `device_list`의 IMS 등록 및 사용자 발신·수신·음성을 확인한다. EFS 리드백 성공만으로 통화 성공을 주장하지 않는다.
7. 언루팅/리락 검토는 현재 OS에서 `boot_image_check` → bootloader → 순정 양 슬롯 기록 → `relock_gate_check` → 필요한 경우 `fastboot_lock` → 재부팅을 나눠 진행한다. stockPath/이미지 해시/실제 기기 대조 게이트는 개발 모드에서도 유지한다.

실패하면 결과와 기기 상태를 확인하고 **공유 엔진 코드를 수정 → cargo run으로 재빌드 → 필요한 명령만 재실행**한다. 이전 성공한 언락/리락/플래시/업로드를 자동 반복하지 않는다. 부트 이미지나 펌웨어를 변경했다면 사전 검사도 다시 수행한다. EFS 복원이 필요하면 저장된 complete snapshot을 `efs_rollback`에 직접 지정하며, 복수 슬롯은 생성 역순으로 복원한다.

## 기록과 종료

`<data-dir>/dev-runs/<시각>-<PID>/record.json`에는 command/step/빌드 feature/시작·종료 시각/status/마스킹된 결과가 저장된다. `events.jsonl`에는 진행 로그가 있고 stdout도 JSON Lines이다. 매 시도는 새 폴더를 만들어 이전 실패 기록을 덮어쓰지 않는다. started 기록 저장에 실패하면 기기 명령을 시작하지 않는다. 기록/로그 저장 실패는 성공 종료로 숨기지 않는다.

종료 코드: `0` 명령 성공, `1` 엔진 오류·EFS 불일치·리락 게이트 거부 등, `2` 입력/옵션/기록 실패. `done`은 이 명령의 종료 상태이며 전체 워크플로우/실기기 검증 완료를 뜻하지 않는다. 검사 명령의 `false`, 백업 `complete`, SMS `ready` 같은 의미도 함께 확인한다.

강제 종료하면 `running` 기록이 남을 수 있다. `history`는 이를 성공으로 바꾸거나 자동 재실행하지 않는다. 명령 전달 후 프로세스를 강제로 죽여도 이미 폰에 보낸 작업이 취소되거나 복원되는 것은 아니다. CLI는 기존 Windows 절전 방지를 실행 동안 적용하지만 창이 없어 GUI의 종료/로그아웃 차단은 제공하지 않는다. GUI와 CLI의 기기 작업 실행권은 공통 OS 파일 잠금으로 겹치지 않게 한다(같은 사용자 TEMP 범위, 프로세스 종료 시 잠금 해제).

## 오프라인 검증

2026-10-05 보강 후: Rust 기본 빌드 216 passed / 8 ignored, `dev-cli`·`dev-cli,fastboot-write`·모든 feature 빌드 각 226 passed / 8 ignored, CLI 바이너리 통합 테스트 각 3 passed(초기화 단계 백업 증명 거부 포함). 백업 게이트·앱 기준 serial 대조·GUI 실행 감지 단위 테스트 추가. `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --check`, `git diff --check` 통과. 프론트 101 passed, Svelte check 0 errors / 0 warnings, 정적 프로덕션 빌드 통과. 기기 질의/USB/COM/DIAG/쓰기는 실행하지 않았다.

처음 검토에 쓸 debug CLI 사본을 `src-tauri/target/dev-cli/xva-dev.exe`(쓰기 feature 포함)와 `xva-dev-readonly.exe`(dev-cli만)에 준비했다. 사본은 현재 코드의 스냅샷이므로 **코드를 수정한 뒤에는 위의 cargo run/build로 새로 컴파일한 실행 파일을 사용한다.** 일반 GUI 설치 파일의 갱신과 CLI 빌드는 별도다.

쓰기 포함 사본 SHA-256: `828a858c82b31c411018b41d2a06e004959cb41541b0dfea0076d5fdb9c76ae8`.
읽기용 사본 SHA-256: `7935d2bb3f89bdd8121c37563a2eccdaefb97f4e89f1a27211bdddd051fb72dd6`.

입력 스키마/feature·실행 옵션/비밀값 마스킹/검증 실패 판정/재시도 이력/공통 이벤트/프로세스 잠금 단위 테스트와, 실제 CLI 실행 파일의 기기 무관 명령·입력 거부·프로세스 재시작 후 기록 조회 통합 테스트를 수행한다. 실제 기기 동작은 `device-test-checklist.md`에서만 체크한다.
