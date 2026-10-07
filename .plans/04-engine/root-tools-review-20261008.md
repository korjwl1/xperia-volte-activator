# Newflasher·루트 도구 후속 코드 리뷰 — 2026-10-08

상태: 확인된 문제 5건 수정 / 오프라인 검증 완료 / 새 기능 실기기 미검증.

`newflasher-add`에서 작성한 네이티브 Newflasher 코어, 루트 감지·엔진 전환·모듈 도구, 소스 정책 변경과 화면 연결을 검토했다. 공유 부트 이미지 검증·Magisk·작업 실행·fastboot 연결도 새 코드가 사용하는 범위에서 확인했다. 전체 프로그램이나 모든 Xperia의 실기기 호환성을 검증한 기록은 아니다.

## 발견한 문제와 수정

| 우선순위 | 기존 문제 | 수정과 근거 |
| --- | --- | --- |
| P1 | ReSukiSU 전환의 init_boot 제한이 화면에만 있었다. 백엔드를 직접 호출하면 boot 기종에서도 기존 모듈을 정리한 뒤 외부 패치 가져오기 단계에서 거부될 수 있었다. | [전환 준비](../../src-tauri/src/root_tools/switch.rs)에서 원본 파티션을 검사한 뒤에만 기기 연결 근거·전환 intent를 저장하고 모듈을 정리한다. boot 기종 거부 시 정리 명령과 두 기록 폴더가 생성되지 않는 회귀 테스트를 추가했다. 같은 boot 기종의 KernelSU 계열→Magisk 준비는 유지한다. |
| P1 | 플래시 자격이나 전환 완료를 PC에 저장하는 일부 호출이 작업 실행 잠금을 쓰지 않았다. 60초 guarded 제한이 먼저 반환돼도 내부 작업은 계속될 수 있어, 화면이 작업을 종료한 뒤 기록이 갱신되거나 다음 변경과 겹칠 수 있었다. | [boot_image_check](../../src-tauri/src/boot_image.rs), root_external_patch_import, root_switch_status의 verify_stock=true, root_switch_finish, [root_module_reconcile](../../src-tauri/src/root_tools/modules.rs)을 WriteOperation+blocking으로 처리한다. 워커가 실제 종료할 때까지 호출을 기다리고 잠금을 유지한다. root_switch_status의 단순 조회는 기존 시간 제한을 유지한다. PC 근거 저장 호출에 새 기기 쓰기 feature를 요구하지 않는다. |
| P2 | 수동 Magisk 준비에서 패치 성공 직후 적용 가능한 이미지를 공개했다. 뒤이은 매니저 설치가 실패해도 이미지 적용 버튼이 남을 수 있었다. | [prepareMagiskImage](../../src/lib/domain/rootTools.ts)는 패치와 매니저 설치가 모두 성공한 뒤에만 결과를 반환한다. ReSukiSU 매니저도 설치 성공 후 준비 결과를 공개한다. [회귀 테스트](../../tests/root-tools.test.mjs)에서 각 단계 실패와 설치 응답 대기를 확인한다. |
| P2 | 권한 조회·패키지 준비·이미지 가져오기 실패 후 이전 성공 결과가 화면에 남았다. 변경 작업 실패나 부분 플래시 뒤에도 이전 결과를 즉시 다시 사용할 수 있었다. | [RootToolsView](../../src/lib/views/RootToolsView.svelte)에서 해당 작업 시작 시 이전 결과를 지우고, 권한·모듈 조회는 함께 성공한 뒤 공개한다. 이미지 적용 시 준비 결과를 소비하고, 변경 실패 후 권한·모듈·패키지·패치·업데이트 계획을 무효화한다. 재조회·재준비가 필요하다. |
| P2 | 모듈 설치 백엔드는 루트 권한과 의존성은 확인했지만 미완료 엔진 전환 기록을 차단하지 않았다. 최종 엔진 확인 전에 모듈을 설치할 수 있었다. | 모듈 설치에서 진행 중이거나 손상된 전환 기록을 ZIP 전송·설치 intent 전에 거부한다. 화면에서도 미완료 전환의 모듈 준비·설치를 잠근다. 완료 기록 또는 전환 기록이 없는 경우에는 기존 조건을 적용한다. 오류 복구용 조회·비활성화·제거는 유지한다. |

## 검증 결과

- 기본 Rust: `cargo test --manifest-path src-tauri/Cargo.toml --lib --no-default-features` — **334 passed / 10 ignored**.
- 쓰기 기능을 명시한 Rust: `cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features dev-cli,root-tools-write,fastboot-write,efs-write` — **349 passed / 10 ignored**, 개발 CLI 통합 **3 passed**. feature를 테스트 빌드에 명시했으며 기본값은 바꾸지 않았다.
- 프론트: `pnpm.cmd test` — **150 passed**. `pnpm.cmd check` — **0 errors / 0 warnings**. `pnpm.cmd build` — 정적 프로덕션 빌드 성공.
- 실제 RootToolsView를 브라우저에서 띄워 API 실패를 주입했다. 이전 패키지 제거, 권한 조회 실패 후 모듈 잠금, 성공 재조회 후 회복, 미완료 전환의 모듈 잠금 **4개 시나리오를 확인**했다. 쓰기 기능은 꺼진 상태였으며 가로 넘침은 없었다. 임시 화면과 테스트 탭은 제거했다.
- 신규 백엔드 회귀 테스트는 boot→ReSukiSU 거부 전에 정리·기록이 없는지, 미완료·손상된 전환에서 모듈 ZIP이 전송되지 않는지 확인한다. 프론트 회귀 테스트는 Magisk 단계 실패 시 후속 호출 중단과 매니저 설치 대기·실패 동안 이미지가 공개되지 않는지 확인한다.
- 관련 구조·기기·화면·계약 문서를 같은 변경에 갱신했다. 변경 문서 **9개**, 로컬 링크 **41개**, Mermaid **2개**와 UTF-8 검사를 통과했다. 수정한 root_tools 파일의 rustfmt와 git diff 공백 검사도 통과했다.

## 남은 범위

실제 폰의 ADB/USB/COM/DIAG/Flash mode 질의·쓰기와 드라이버 설치는 실행하지 않았다. ignored 실기기·네트워크 검사는 실행하지 않았으며, 위 통과 수는 새 실기기 검증 결과가 아니다. 느린 기기 응답 중 실행 잠금·화면 busy 유지 등 실제 장치 확인 항목은 [단일 체크리스트](device-test-checklist.md)에 남긴다.

네이티브 Newflasher의 패키지·SIN·프로토콜·정책·전송 계층과 테스트도 읽었으나 이번 리뷰에서 추가로 확정한 결함은 없다. 기기 식별/profile/boot delivery 검증과 전체 업데이트 연결은 여전히 미완료이며 하드웨어 플래시 API는 등록하지 않았다. [구현 진척](newflasher-native-progress.md)과 [설계](newflasher-native.md)의 한계를 유지한다.

ReSukiSU 외부 패치 경로는 init_boot만 허용한다. KernelSU 계열 감지가 특정 포크나 패치 방식의 호환성을 증명하지 않는다. 새 엔진 전환·모듈 동작은 기기·펌웨어별 검증이 필요하다. 기본 Cargo 쓰기 feature `[]`와 REAL_STEPS=false는 유지한다.

HMA 카페 JSON, AshReXcue KO와 PlayStoreFix 동봉 원본은 이번 수정에서 변경하지 않았다. OverlayFS는 기존 공식 GitHub 소스를 유지한다. [현재 루트 도구 설명](../../docs/structure/root-tools.md), [기기별 기록](../../docs/devices.md), [PlayStoreFix 조사](../../docs/structure/play-store-fix.md)를 참조한다.
