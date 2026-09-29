# AGENTS.md — xperia-volte-activator 작업 규칙

Xperia VoLTE 활성화 통합 도구. Tauri 2 + SvelteKit 2 + Svelte 5 + TypeScript 프론트, Rust 백엔드.
원천 정책 문서는 상위 폴더 `../tasks/plan.md`(v4)와 `../tasks/recovery.md`이며, 실행 계획은 `./.plans/`에 유지된다.

## 현재 단계 (매우 중요)

- **프론트엔드 우선 개발 중 — `src-tauri/` 백엔드 로직 작성 금지.** 템플릿 boilerplate만 유지한다.
- 모든 데이터는 `src/lib/mock/`의 mock으로 구동된다. 백엔드 계약은 `.plans/02-contracts/tauri-commands.md`에만 명세한다.
- 프론트는 `pnpm dev`(Vite 브라우저)로 작업한다. Rust 미설치로 `pnpm tauri dev`는 불가(M2에서 구축).

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
```
PowerShell에서는 `pnpm.cmd` 사용(실행 정책이 .ps1을 차단함).
