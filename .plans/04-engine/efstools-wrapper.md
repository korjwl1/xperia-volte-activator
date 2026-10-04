# EfsTools 래퍼 엔진 (M5 1단계) — 구현 설계

> Historical design, superseded on 2026-10-05 by [the native Rust engine](efs-native.md).
> The subprocess runtime and vendor build script have been removed. VoLTE property/reboot and IMS verification stages are retained in the native integration. Commands and signatures below document the old implementation; use [current contracts](../02-contracts/tauri-commands.md).

status: implemented (실기기 검증 대기 — 파서·검증 단위 테스트, efs-write + REAL_STEPS.efs 이중 게이트)

- 조사 근거: `../tasks/research-efstools-integration.md` (2026-10-04 — 하이브리드 2단계 권고, 라이선스·버전 확정)
- 방식: **서브프로세스 래퍼** — EfsTools 0.14.0.14(업스트림 master a8172de, .NET 8 self-contained 재빌드)를 argv 배열 실행(newflasher M6 패턴). 2단계(선택) Rust 네이디브 포트는 같은 efs_* 계약 뒤에서 교체만.

## 핵심 제약 (조사 확정)

1. **EfsTools 종료 코드는 항상 0** — Main이 void, 예외도 catch에서 삼킴, 인자 오류도 무시. 성공 판정은 불가능
2. → 성공 판정 = ①출력 에러 패턴 파싱 + ②**독립 전수 리드백 검증**(downloadDirectory → 프리셋 SHA-256 비교)의 조합만으로
3. 기준 에러 문양(실측): `Critical error. Port not found: COM7` (영어 리소스 폴백),
   `Error on upload file '<path>'. EFS error. Code = NNN` (이슈 #7)
4. `port="auto"` COM 자동 감지(설정 파일 불필요), `ignoreUnsupportedCommands=True`(조용한 무시 → 검증이 최종 방어)

## 바이너리 공급 (vendor)

- `scripts/build-efstools.ps1` — 업스트림 고정 커밋 clone → .NET 8 SDK(dotnet-install, 사용자 로컬) → self-contained win-x64 게시 → `vendor/efstools/`
- 재현 해시: EfsTools.dll SHA-256 `99201687…D04FD` (85MB, 트림 없음 — CommandLineParser 리플렉션)
- vendor/efstools/는 .gitignore — 실행 파일 탐색 순서: `XVOLTE_EFSTOOLS_DIR` 환경변수 → 앱 실행 파일 옆 vendor → 개발 리포지토리 vendor
- 라이선스: 전 의존 MIT/X11/MS-PL(재배포 가능, 고지 동봉 — License.md 포함됨) — 조사 보고서 §9

## 계약 (02-contracts EFS 절 교체)

```ts
invoke('efs_tool_check') → { version, path, sha256 }      // `version` 실행 — 기기 무관·읽기 전용(게이트 밖)
invoke('efs_diag_open', { serial }) → void                // su -c setprop sys.usb.config diag,... (원본 efs.py 계승)
invoke('efs_preflight') → { log: string[] }               // targetInfo + efsInfo — DIAG 연결·EFS 정보 확인
invoke('efs_upload', { presetDir }) → { errors: string[] } // uploadDirectory -i <dir> -o / -v (1회 — 슬롯별 2회는 wizard가 호출)
invoke('efs_verify', { presetDir }) → VerifyReport         // 프리셋 최상위 폴더별 downloadDirectory → 전수 해시 비교
//   VerifyReport = { ok, files, matched, mismatches[], missing[] }
invoke('efs_snapshot', { dest }) → { errors: string[] }    // downloadDirectory -i / -o <dest> (before-image)
invoke('efs_cancel') → void                                // 진행 중 프로세스 kill
// 이벤트 'efs:log': { cmd, line } — 실시간 출력 스트림(전 라인)
```

- 게이트: efs_diag_open·preflight·upload·verify·snapshot·cancel은 Cargo feature `efs-write` + 프론트 REAL_STEPS.efs
- efs_verify가 성공 판정의 **유일한 최종 근거** — 업로드 후 반드시 통과해야 단계 완료 (계약 "두 번 썼다는 것만으로 성공 판정하지 않음" 계승)

## 모듈

```
src-tauri/src/efstools/
├─ mod.rs     — 명령 7종·이벤트 emit·취소 플래그·게이트(efs-write)
├─ runner.rs  — 자식 프로세스 argv 실행·stdout/stderr 동시 드레인·라인 스트림·에러 패턴 분류(순수 fn — 단위 테스트)
└─ verify.rs  — 프리셋 파일 전수 해시 비교(순수 로직 — 단위 테스트)
```

- 원본 절차 계승: DIAG 전환 setprop → (슬롯별 2회 업로드) → 전수 리드백 → USB 재연결 대기(adb 복귀)
- 검증 임시 폴더: 앱 데이터 efs-verify/<nonce>/ — 비교 후 정리

## 테스트 (실기기 없이)

1. 파서: 실측 문양(Critical error / Error on upload file / EFS error. Code) 분류, 정상 라인은 오류 아님, 러시아어 대소문자 변형 무관(ASCII 패턴)
2. verify: 일치/불일치/누락 3분기 + 빈 프리셋 + 하위 디렉터리
3. runner 실기 동작(vendor 있을 때만, #[ignore] 가드): `version` 실행 → 0.14 확인
4. 프로세스 취소: kill 플래그 → run 중단

## 리스크·검증 대기

| 항목 | 상태 | 대응 |
|---|---|---|
| 업로드 성공 라인 포맷(진행률 파싱) | 미실측 | 전 라인 이벤트 전달, 오류 패턴만 분류 — 실측 후 진행률 정밀화 |
| ru 문화권 출력 | 미실측(현 폴백=영어 확인) | 패턴은 ASCII 영어 기준, 필요시 러시아어 문양 추가 |
| 전수 리드백 소요(파일당 COM 세션) | 미실측 | 폴더별 downloadDirectory 일괄 수신으로 완화 |
| 실기기 업로드 2회·검증 완주 | 미실측 | REAL_STEPS.efs 전환 후 실측 기록 → 2단계(포트) 픽스처로 승격 |
