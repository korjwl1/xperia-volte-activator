# AGENTS.md — xperia-volte-activator 작업 규칙

Xperia VoLTE 활성화 통합 도구. Tauri 2 + SvelteKit 2 + Svelte 5 + TypeScript 프론트, Rust 백엔드.
원천 정책 문서는 상위 폴더 `../tasks/plan.md`(v4)와 `../tasks/recovery.md`이며, 실행 계획은 `./.plans/`에 유지된다.
GitHub에서 읽는 현재 시스템 설명은 `docs/structure/overview.md`, 기기별 동작·검증 기록은 `docs/devices.md`에 유지한다. 상위 로컬 자료 없이도 이 저장소의 코드와 문서로 현재 동작을 이해할 수 있어야 한다.

## 문서 유지와 GitHub 포함 (필수)

- **프로그램을 수정하면 영향을 받는 문서도 반드시 같은 변경/커밋에서 갱신한다.** 기능, 화면 흐름, API, 파티션/모드 선택, 기종 조건, 실패·재개·검증 정책, 기본 실행 설정이 바뀌면 해당 `docs/structure/*.md` 설명과 Mermaid/표도 함께 수정한다. 문서에 영향이 없는 변경은 작업 결과에 그 이유를 짧게 적는다.
- 기기별 분기 또는 실기기 결과가 바뀌면 `docs/devices.md`를 갱신한다. 정확한 모델·지역·펌웨어·통신사·SIM·사용한 경로(GUI/CLI)·검증 범위를 적고, 미검증·실패·추정과 성공을 구분한다. 구현된 모델 표만으로 호환성 테스트 완료라고 표시하지 않는다.
- README는 짧은 프로젝트 소개, 실기기 검증 범위, 주요 경고, 문서 링크를 유지한다. 검증 범위나 사용자에게 보이는 제한이 바뀌면 README도 갱신한다. 상세 구현·설치 설명은 docs에 둔다.
- `.plans/`는 구현 계획·내부 검증 체크리스트이며 `docs/`는 현재 동작 설명이다. 새 계획은 계획이라고 명시하고 구현 완료로 설명하지 않는다. 기존 `.plans` 갱신 의무도 유지한다.
- **루트 `AGENTS.md`, `README.md`, `docs/`는 Git으로 추적하고 GitHub에 포함한다.** 작업 규칙을 로컬 전용 파일로 제외하지 않는다. GitHub에 게시하는 변경에는 관련 문서를 포함하고 로컬 링크·그림 문법을 검사한다.
- 공개 문서/예시에 실제 IMEI·언락 코드·기기 시리얼·전화번호·토큰·개인 PC 경로를 남기지 않는다. 프로젝트 내부 상대 경로와 마스킹된 예시를 사용한다.

## 현재 단계 (매우 중요)

- **공유 Rust 엔진·개발 CLI·화면 구현 및 오프라인 검증 단계**. 아래 사용자 승인에 따라 백엔드 수정·빌드·병합을 진행한다. 이번 작업에서 실제 폰의 ADB·USB·COM·DIAG 질의나 쓰기를 실행하지 않는다.
- 2026-10-08 사용자 결정: 실기기(XQ-DQ44) 검증을 마친 단계만 기본 실전이다 — `REAL_STEPS` backup·restore·fastboot·root·efs와 Cargo 기본 `fastboot-write`·`root-write`·`efs-write`. 리락(`REAL_STEPS.relock`)·루팅 도구/ReSukiSU 루팅(`REAL_STEPS.rootTools`, `root-tools-write`)·업데이트 확인(`verify`)은 체크리스트 항목이 모두 확인되기 전까지 끈다. 실기기 검증은 사용자 세션에서 진행한다.
- mock은 브라우저 개발에만 사용한다. 데스크톱 연결/기록 오류는 실패로 표시하고 mock 성공이나 기록 없음으로 바꾸지 않는다.
- Rust 설치 완료 — `pnpm.cmd tauri dev`로 데스크톱 윈도우 테스트 가능하다.
- 예외(사용자 승인 2026-10-02): USB 직접 연결용 ADB 인증 키를 앱 데이터 폴더에 1회 생성·저장한다 (표준 ~/.android/adbkey가 있으면 그것을 사용).
- 예외(사용자 승인 2026-10-03): 폰에 설정 화면(개발자 옵션/휴대전화 정보)을 띄우는 것 — 설정 값은 바꾸지 않음.
- 예외(사용자 승인 2026-10-03): 작업 중 PC 보호(src-tauri/src/guard.rs) — 실행 중에만 절전 방지(PowerRequestSystemRequired)와 Windows 종료 방지(ShutdownBlockReasonCreate + WM_QUERYENDSESSION 보류)를 켜고, 완료·중단·오류·처음으로 시 즉시 해제. 프로세스 강제 종료 방지는 하지 않음.
- 예외(사용자 승인 2026-10-03): 작업 진행 기록(journal) — 실행 상태를 앱 데이터 폴더 journal/<기기 해시>.json에 저장해 끊긴 작업을 이어서 진행 (src-tauri/src/journal.rs). 언락 코드·IMEI는 기록하지 않음.
- 예외(사용자 승인 2026-10-03): 순정 펌웨어 부트 이미지(init_boot/boot)를 Sony 서버에서 부분 다운로드해 앱 데이터 폴더 firmware/ 캐시에 저장한다 — 저장 공간이 부족하면 사용자가 고른 다른 폴더에 저장 (src-tauri/src/firmware.rs, 재배포 금지).
- 예외(사용자 승인 2026-10-03, feat/backup-engine 워크트리): **백업·복구 엔진 실전 코드 작성 허용** — 백업(폰에서 읽기만)은 사용자 요청으로 실기기 검증 완료(2026-10-03). 복구(폰에 쓰기)는 실기기 테스트 금지 유지 — 모의 실행(live_restore_dryrun)과 FakeADBDevice 단위 테스트로 검증. 실행은 `REAL_STEPS` 플래그가 꺼져 있는 동안 시뮬레이션 유지. 설계는 `.plans/04-engine/backup-engine.md`.
- 예외(사용자 승인 2026-10-04, efs-native-rust 격리 워크트리): **네이티브 Rust EFS/NV 구현·오프라인 검증·커밋 허용**. `src-tauri/src/efs/`의 COM/HDLC/DIAG/EFS/NV, `adb_client`를 통한 DIAG 전환, 스냅샷·업로드·리드백·롤백을 작성한다. 실기기 작업·DIAG 전환 테스트는 금지. Cargo `efs-write`와 `REAL_STEPS.efs`는 기본 꺼짐 유지. 공유 원본 프리셋·상위 tasks·다른 워크트리는 수정하지 않는다. 설계: `.plans/04-engine/efs-native.md`.
- 예외(사용자 승인 2026-10-03, feat/fastboot-unlock 워크트리): **fastboot 엔진(언락/리락/플래시) 실전 코드 작성 허용** — 단 실기기 테스트 금지(FakeTransport 단위 테스트). 실행은 `REAL_STEPS.fastboot` 꺼져 있는 동안 시뮬레이션 유지. 설계는 `.plans/04-engine/fastboot.md`.
- 예외(사용자 승인 2026-10-04, feat/root-engine 워크트리): **루팅 엔진(Magisk 자동 패치·기록·설치) 실전 코드 작성 허용** — 절차는 2026-10-03 실기기 검증 분량. 단 이 구현의 실기기 테스트는 금지(FakeADBDevice 단위 테스트). 실행은 `REAL_STEPS.root` + Cargo feature `root-write` 이중 게이트 뒤(기록은 기존 fastboot-flash 게이트 재사용). 설계는 `.plans/04-engine/root.md`.
- 예외(사용자 승인 2026-10-04, feat/unroot-relockgate 워크트리): **언루팅 절차 연결 + 리락 게이트(§3-3) 실전 구현 허용** — 기존 명령 조합(root_reboot·fastboot_flash·fastboot_reboot)과 게이트 판정 로직. 실기기 테스트 금지(이력 픽스처 단위 테스트). 리락 게이트는 fastboot-write feature 뒤. 설계는 `.plans/04-engine/unroot-relock.md`.
- 예외(사용자 승인 2026-10-04): 완료 화면 [백업 파일 삭제] — 사용자가 확인 모달에서 [삭제]를 누른 경우에만, 이 앱이 만든 백업 폴더(이름 `xva-<모델>-backup` 또는 이전 이름 `backup-*` + 유효한 manifest.json, 심볼릭 링크·정션 아님)를 PC에서 지운다(`backup_delete`). 사용자가 고른 상위 저장 위치는 지우지 않는다.
- **실기기 미검증 항목은 `.plans/04-engine/device-test-checklist.md` 한 곳에서 관리한다.** 단계의 REAL_STEPS·쓰기 Cargo 기능은 그 단계 항목이 모두 체크되기 전까지 켜서 배포하지 않는다. 새 기기 동작을 추가하면 체크리스트에 항목을 추가한다.
- 예외(사용자 승인 2026-10-05): **전체 브랜치 병합·통합 코드 리뷰·수정·실기기 이외의 검증 허용**. 원본 파일 읽기, 공식 배포 해시 확인, Sony 서버 부분 다운로드, 빌드·패키징은 허용한다. 실기기 연결·DIAG 전환·COM/USB 동작 테스트는 하지 않으며 기존 쓰기 기능 기본 꺼짐을 유지한다. 통합 리뷰: `.plans/04-engine/full-review-20261005.md`.
- 예외(사용자 승인 2026-10-05 후속): **조건부 리락 구현·기종 인식 워크플로우 보강 허용**. 과거 리락 무조건 차단 및 II/IV 일괄 제한을 재검토한다. Hanabi 배포 소스와 실제 사용자 보고에 맞추며 PDC·모뎀 교체·외부 앱 설치는 새로 자동화하지 않는다. 실기기 테스트 금지 및 기본 쓰기 비활성은 유지한다. 최신 기준: `.plans/04-engine/model-workflow-recheck-20261005.md`.
- 기기 통신: ADB는 `adb_client` 크레이트(ADB 프로토콜 순수 Rust) — 실행 중인 adb 서버 재사용 → USB 직접 연결 폴백. EFS/NV는 명시적으로 지정한 COM의 순수 Rust DIAG 세션. adb·EfsTools 바이너리 직접 실행 및 .NET 런타임 의존 금지.
  - 레지스트리 판이 아니라 I/O 시간 상한을 넣은 사본 `src-tauri/vendor/adb_client`(3.2.3, `[patch.crates-io]`)을 쓴다. 고친 내용은 `vendor/adb_client/PATCHES.md`에 기록하고, 업그레이드할 때 다시 적용한다.

- 예외(사용자 요청 2026-10-05, 개발 CLI): **기존 Rust 엔진을 공유하는 단계별 개발 실행 파일 구현·오프라인 검증 허용**. Cargo `dev-cli`로만 CLI를 포함하고 쓰기 feature는 별도 명시한다. 일반 앱의 기본 feature·REAL_STEPS는 유지한다. 실기기 단계별 검토를 위한 준비이며 이번 작업에서는 기기 통신·실기기 테스트를 실행하지 않는다. 설계·사용법: `.plans/04-engine/dev-cli.md`.

- 예외(사용자 승인 2026-10-07): **언락 실기기 진행 허용**. 실제 `oem unlock` 실행은 실행 직전 사용자 확인을 받는다. 사전 점검(부트로더 재부팅·getvar·OS 재부팅)은 완료했다. Windows fastboot 드라이버가 없으면 Sony 공식 드라이버를 받아 관리자 권한(UAC)으로 부트로더 모드 장치에 지정한다(`src-tauri/src/usb_driver.rs`). PC 드라이버 설정을 바꾸는 유일한 경로이며, 사용자가 UAC에서 허용한 경우에만 실행된다.

## 필수 작업 규칙

예외(사용자 요청 2026-10-08 후속, `newflasher-add`): `../root-method.md`와 `../for-rooted-phone.md`를 읽고 **ReSukiSU 버전 선택·반수동 패치·수동 엔진 전환·모듈 준비/설치/재부팅 확인 기능의 코드와 화면 구현·오프라인 검증을 허용**한다. 카페 전용 ZIP·HMA 프리셋은 사용자 지시에 따라 `src-tauri/assets/root/`에 원본·해시·출처를 함께 포함한다. 새로운 기기 변경 기능은 `root-tools-write`(기본 꺼짐), 기존 root/fastboot 게이트와 확인 절차를 유지한다. 실제 폰 연결·조작은 실행하지 않는다. 입력 문서의 무손실·금융앱 호환 보장이나 미검증 기종/버전 범위를 구현 설명에 그대로 옮기지 않는다. 현재 설명은 `docs/structure/root-tools.md`, 계획·검증 기록은 `.plans/04-engine/root-tools.md`.

후속 소스 정책(사용자 요청 2026-10-08): **HMA 카페 JSON 프리셋은 원본 그대로 동봉·내보내기·가져오기 안내를 유지**한다. 직접 목록/범위를 구성하는 절차로 대체하지 않는다. HMA 프로그램은 기존 공식 GitHub 배포를 사용한다. **OverlayFS는 `RipperHybrid/Meta-Overlayfsx`의 GitHub 최신 stable ZIP을 실행 시 조회·다운로드·검증**하며 카페 사본을 동봉하지 않는다. AshReXcue KO와 PlayStoreFix v3.4는 원본 동봉을 유지한다. PlayStoreFix의 공개 기반과 카페판 차이·조사 한계는 `docs/structure/play-store-fix.md`에 기록한다.

예외(사용자 요청 2026-10-08, `newflasher-add` 워크트리): **Newflasher 고정 소스 기반 Rust 네이티브 엔진 구현·오프라인 검증·커밋 허용**. 계획은 `.plans/04-engine/newflasher-native.md`. 메인 폴더의 미커밋 변경·다른 작업 폴더를 수정하지 않는다. 실제 ADB/USB/COM/Flash mode 질의·쓰기·드라이버 설치를 실행하지 않는다. 기기 식별/profile/boot delivery 검증 전에는 하드웨어 플래시 API를 등록하지 않고 기존 기본 쓰기 feature/REAL_STEPS를 유지한다. 공개 문서에는 브랜치 구현과 실기기 미검증을 구분한다.

1. **.plans 문서 의무**: 뷰를 작성·변경하면 대응하는 `.plans/01-views/<view>.md`를 같은 커밋에서 갱신한다.
   - 새 뷰 → 목적/상태 필드/버튼→백엔드 계약 매핑을 문서에 기록
   - 새 백엔드 계약 → `.plans/02-contracts/tauri-commands.md`에 시그니처 추가하고 facade와 타입을 함께 갱신
   - mock 스키마 변경 → `.plans/03-data/mock-schema.md` 갱신
2. **모든 백엔드 호출은 `src/lib/api/` facade 경유** — mock/실전 전환이 한 곳에서 되도록. 컴포넌트에서 `@tauri-apps/api`를 직접 import 금지.
3. UI는 shadcn-svelte(`$lib/components/ui`) 우선. 새 컴포넌트는 `$lib/components/`에.
4. 스타일: Tailwind v4. **테마는 시스템 설정 따름**(`prefers-color-scheme` 자동 감지, 다크 강제 금지). 앱 셸은 데스크탑 마법사 레이아웃(타이틀 바 + 좌측 단계 사이드바 + 콘텐츠 + 하단 액션 바). **UI/UX 전반은 `.opencode/skills/desktop-ui` 스킬 규칙 필수 준수** (pane 스크롤, MD3 톤 토큰, lucide 아이콘, 도구류 UI 관례).
5. **민감정보 마스킹 원칙**(상위 plan §12.5): mock/로그/UI에서 IMEI·언락 코드는 마스킹해 표기한다. 일련번호는 부분 마스킹(예: `AB1234****`).
6. **경로 하드코딩 금지**(한글/공백 경로 호환, §12.5) — 표시 경로는 설정/상태에서 온다.
7. 파괴적 단계(언락/리락/플래시) UI에는 항상 위험 배지 + 확인 게이트가 있다(§3-3 의존성 규칙 준수).

## 코딩 컨벤션

- Svelte 5 runes(`$state`/`$derived`/`$props`) 사용 — `export let` 구식 문법 금지
- 상태 저장소는 `*.svelte.ts` 파일로(`src/lib/stores/`)
- 타입은 `src/lib/types.ts`에 집중, mock은 실측 데이터(plan.md/recovery.md 기반)를 사용한다
- 커밋 메시지: `feat(front): ...`, `docs(plans): ...`, `chore: ...` 형식

## 빌드/실행

```powershell
pnpm.cmd install
pnpm.cmd dev        # 브라우저 개발 (http://localhost:1420)
pnpm.cmd check      # svelte-check 타입 검사
pnpm.cmd tauri dev  # 데스크톱 윈도우 (Rust 백엔드 연동 — dev 서버가 실행 중이면 먼저 종료)
```
PowerShell에서는 `pnpm.cmd` 사용(실행 정책이 .ps1을 차단함).
Rust 빌드에는 MSVC 필요 — `src-tauri` 빌드 시 vcvars 환경(또는 Visual Studio Build Tools + C++ 워크로드) 필요.


## 2026-10-08 main GUI 후속 요청

사용자가 확인한 main에 작업 종류 분기·수동 매니저 변경/모듈 세트 설치·자동 루팅 유지 시 세트 선택을 반영한다. 세트의 의존성을 코드/문서에서 함께 유지하고 초기화/언루팅/리락으로 쓸 수 없는 선택은 해제·비활성화한다. 새 펌웨어 버전 선택은 VoLTE 인식 기기의 전용 Newflasher 업데이트 경로에만 두고 자동/수동 순정 이미지는 현재 설치 버전 기준으로 준비한다. 앞의 newflasher-add 작업 제한은 당시 격리 작업 범위 기록이며 현재 main 수정 요청을 막지 않는다. 기본 쓰기 게이트와 이번 작업의 실기기 테스트 금지는 유지한다.
