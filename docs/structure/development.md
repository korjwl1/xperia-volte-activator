# 개발과 검증

기준일: 2026-10-07. Windows/MSVC, Tauri 2·SvelteKit 2·Svelte 5·TypeScript·Rust 구성입니다. 실제 명령·의존성 버전은 `package.json`, `src-tauri/Cargo.toml`과 lockfile이 기준입니다.

## 준비와 화면 실행

Rust 최소 버전은 현재 Cargo 설정의 1.88입니다. Windows Rust 빌드에는 Visual Studio Build Tools의 C++ 워크로드/MSVC 환경과 WebView2가 필요합니다. 프론트는 pnpm을 사용합니다. PowerShell에서는 실행 정책 문제를 피하려고 `pnpm.cmd`를 호출합니다.

```powershell
pnpm.cmd install --frozen-lockfile
pnpm.cmd dev
pnpm.cmd check
pnpm.cmd test
pnpm.cmd build
```

`vite.config.js`의 `optimizeDeps.include`에 facade가 동적 import하는 Tauri 모듈(core·event·window·plugin-dialog)을 미리 넣어 둡니다. 개발 중 처음 import될 때 Vite가 의존성을 다시 묶으면 열린 창의 모듈 해시가 낡아 폴더 선택 창 등이 조용히 실패했습니다(2026-10-08). release 빌드에는 영향이 없습니다.

브라우저 개발은 명시적 mock 환경입니다. Tauri 앱은 `pnpm.cmd tauri dev`로 실행하며 이미 같은 포트에서 개발 서버가 실행 중이면 먼저 정리합니다. Rust 빌드는 MSVC 개발 셸에서 실행합니다.

## 실행 게이트

| 항목 | 현재 기본 | 역할 |
|---|---|---|
| `SIMULATED_RUN` | true | 화면의 모의 기기 절차/수동 확인 개발 |
| `REAL_STEPS` | backup·restore·fastboot·root·efs true, relock·rootTools·verify false | GUI 단계별 실전 엔진 연결. 리락(`relock`)과 루팅 도구·ReSukiSU 루팅(`rootTools`)은 별도 스위치 |
| Cargo default features | `fastboot-write`, `root-write`, `efs-write` | 실기기 검증을 마친 부트 기록·Magisk 루팅/언루팅·EFS 쓰기. `root-tools-write`는 제외 |
| `fastboot-write` / `root-write` / `efs-write` | 명시적 선택 | 각각 부트 기록, Magisk 폰 작업, DIAG/EFS 작업 허용 |
| `dev-cli` | 명시적 선택 | 공유 엔진 개발 실행 파일 포함 |

2026-10-08 사용자 결정으로 XQ-DQ44에서 실기기 검증을 마친 백업·복원·언락·루팅·언루팅·VoLTE만 기본 실전으로 켰습니다. 리락·루팅 도구(엔진 전환·모듈)·ReSukiSU 처음 루팅·업데이트 확인은 검증 전이라 꺼져 있습니다.

일부 백업 준비/문자 작업·복구 등은 위 세 쓰기 feature만으로 일괄 통제되지 않습니다. API/CLI의 명령별 쓰기 정책을 함께 확인해야 합니다. 실제 테스트를 하려는 이유만으로 일반 빌드의 기본 플래그를 바꾸지 않습니다.

## 공유 엔진 CLI

```powershell
cargo run --manifest-path src-tauri/Cargo.toml --bin xva-dev --features dev-cli -- commands
```

위 명령은 명령 목록을 확인하는 진입점입니다. 개발 CLI는 Tauri 화면을 띄우지 않고 동일한 Rust 명령/`*_with_events` 함수를 호출합니다. 결과·진행 이벤트는 JSON Lines로 출력합니다. CLI에서 GUI 계획·기종 선택·수동 대기 전체를 대신하지는 않습니다.

명령별 JSON 요청, 승인·백업 증명, 식별자 마스킹, 단계별 기록·재실행은 [개발 CLI 상세 사용법](../../.plans/04-engine/dev-cli.md)을 따릅니다. GUI와 CLI 기기 작업을 동시에 실행하지 않습니다. 엔진 수정은 재빌드한 앱/CLI 양쪽에 반영됩니다.

## 오프라인 검증과 실기기 검증

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

도메인·위자드 tests는 가짜 IPC로 순서/입력/오류를 확인하고 Rust tests는 가짜 ADB/전송/파일로 엔진을 검사합니다. 필요하면 변경한 feature 조합의 tests·Clippy를 실행합니다. ignored 실기기 tests를 사용자 요청 없이 실행하지 않습니다.

오프라인 통과는 실기기 호환성 완료가 아닙니다. [실기기 체크리스트](../../.plans/04-engine/device-test-checklist.md)와 [기기별 메모](../devices.md)에 모델·펌웨어·통신사·GUI/CLI 경로·실제 결과를 적습니다. 과거 대용량 백업 성공으로 이후 메타데이터·재연결 수정까지 검증됐다고 하지 않습니다.

## 코드 변경 시 문서

[AGENTS.md](../../AGENTS.md)에 따라 기능 문서·Mermaid·기기 표·해당 `.plans`를 같은 변경에서 갱신합니다. README의 검증/제한도 결과와 맞춥니다. 새 코드 경로는 담당 모듈·분기 조건·검증 근거·미구현 범위를 적습니다. 문서 링크, Mermaid 문법, Git 추적 여부를 검사하고 원본 개인정보/백업/키를 포함하지 않습니다.

## 시작 스플래시

앱을 처음 띄우면 첫 기기 조회가 끝날 때까지(최대 15초) 시작 스플래시가 빈 화면을 가립니다. 그림은 `src/lib/splash/draw.ts`, 이미지는 `static/splash/xperia-phone.png`, 화면 설명은 `.plans/01-views/splash.md`입니다. 움직임 줄이기 설정을 따르며, 창 배경색(`tauri.conf.json` `backgroundColor`)도 같은 색으로 맞췄습니다.
