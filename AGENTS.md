# AGENTS.md — xperia-volte-activator 작업 규칙

Xperia VoLTE 활성화 통합 도구. Tauri 2 + SvelteKit 2 + Svelte 5 + TypeScript 프론트, Rust 백엔드.
원천 정책 문서는 상위 폴더 `../tasks/plan.md`(v4)와 `../tasks/recovery.md`이며, 실행 계획은 `./.plans/`에 유지된다.

## 현재 단계 (매우 중요)

- **프론트엔드 작업 위주 단계. 기기·PC에 영향을 줄 수 있는 백엔드(쓰기·설치·삭제·플래시·EFS·백업 실행 등)는 작성 금지** — 실기기 작동 시나리오는 짜지 않는다.
- **읽기 전용·무해한 명령**(기기 감지, 용량 조회 등)은 프론트와 연동해 실제로 동작하는 것을 눈으로 확인하며 작성해도 된다. 이때 `.plans/02-contracts/tauri-commands.md`에 계약을 추가한다.
- mock은 백엔드가 없어도 프론트가 동작하도록 유지한다 (연동 실패 시 mock 폴백).
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
- 예외(사용자 승인 2026-10-04): 완료 화면 [백업 파일 삭제] — 사용자가 확인 모달에서 [삭제]를 누른 경우에만, 이 실행이 만든 백업 폴더(이름 `backup-*` + 유효한 manifest.json, 심볼릭 링크·정션 아님)를 PC에서 지운다(`backup_delete`). 사용자가 고른 상위 저장 위치는 지우지 않는다.
- **실기기 미검증 항목은 `.plans/04-engine/device-test-checklist.md` 한 곳에서 관리한다.** 단계의 REAL_STEPS·쓰기 Cargo 기능은 그 단계 항목이 모두 체크되기 전까지 켜서 배포하지 않는다. 새 기기 동작을 추가하면 체크리스트에 항목을 추가한다.
- 예외(사용자 승인 2026-10-05): **전체 브랜치 병합·통합 코드 리뷰·수정·실기기 이외의 검증 허용**. 원본 파일 읽기, 공식 배포 해시 확인, Sony 서버 부분 다운로드, 빌드·패키징은 허용한다. 실기기 연결·DIAG 전환·COM/USB 동작 테스트는 하지 않으며 기존 쓰기 기능 기본 꺼짐을 유지한다. 통합 리뷰: `.plans/04-engine/full-review-20261005.md`.
- 예외(사용자 승인 2026-10-05 후속): **조건부 리락 구현·기종 인식 워크플로우 보강 허용**. 과거 리락 무조건 차단 및 II/IV 일괄 제한을 재검토한다. Hanabi 배포 소스와 실제 사용자 보고에 맞추며 PDC·모뎀 교체·외부 앱 설치는 새로 자동화하지 않는다. 실기기 테스트 금지 및 기본 쓰기 비활성은 유지한다. 최신 기준: `.plans/04-engine/model-workflow-recheck-20261005.md`.
- 기기 통신: ADB는 `adb_client` 크레이트(ADB 프로토콜 순수 Rust) — 실행 중인 adb 서버 재사용 → USB 직접 연결 폴백. EFS/NV는 명시적으로 지정한 COM의 순수 Rust DIAG 세션. adb·EfsTools 바이너리 직접 실행 및 .NET 런타임 의존 금지.
  - 레지스트리 판이 아니라 I/O 시간 상한을 넣은 사본 `src-tauri/vendor/adb_client`(3.2.3, `[patch.crates-io]`)을 쓴다. 고친 내용은 `vendor/adb_client/PATCHES.md`에 기록하고, 업그레이드할 때 다시 적용한다.

- 예외(사용자 요청 2026-10-05, 개발 CLI): **기존 Rust 엔진을 공유하는 단계별 개발 실행 파일 구현·오프라인 검증 허용**. Cargo `dev-cli`로만 CLI를 포함하고 쓰기 feature는 별도 명시한다. 일반 앱의 기본 feature·REAL_STEPS는 유지한다. 내일 실기기 검토를 위한 준비이며 이번 작업에서는 기기 통신·실기기 테스트를 실행하지 않는다. 설계·사용법: `.plans/04-engine/dev-cli.md`.

## 필수 작업 규칙

1. **.plans 문서 의무**: 뷰를 작성·변경하면 대응하는 `.plans/01-views/<view>.md`를 같은 커밋에서 갱신한다.
   - 새 뷰 → 목적/상태 필드/버튼→백엔드 계약 매핑을 문서에 기록
   - 새 백엔드 계약 → `.plans/02-contracts/tauri-commands.md`에 시그니처 추가 (프론트엔드 코드엔 mock만)
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
