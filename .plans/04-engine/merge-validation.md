# 의존 브랜치 병합 검증 — 2026-10-04

status: implemented (main 통합, 실기기 테스트 없음)

사용자가 지정한 커밋을 아래 순서대로 `--no-ff` 병합했다. 각 병합 결과를 검사한 뒤 병합 커밋을 확정했으며 충돌은 없었다.

| 순서 | 브랜치 | 원본 커밋 | `cargo test --lib` | `pnpm.cmd check` |
|---|---|---|---|---|
| 1 | feat/fastboot-unlock | b1f0290 | 113 통과, 5 ignored | 오류 0, 경고 0 |
| 2 | feat/root-engine | 7a7f0b6 | 123 통과, 5 ignored | 오류 0, 경고 0 |
| 3 | feat/unroot-relockgate | e54825b | 133 통과, 5 ignored | 오류 0, 경고 0 |

최종 통합 상태에서 `pnpm.cmd test` 23개도 통과했다. ignored/live_* 테스트는 실행하지 않았다. 병합 과정에서 Sony 장치 연결·루팅·언루팅·리락·플래시·실제 다운로드를 실행하지 않았다. 각 엔진 문서의 실기기 미검증 항목은 계속 적용한다.

추가 검증: `cargo test --lib --all-features` 133개 통과(5 ignored), `pnpm.cmd build` 정적 프로덕션 빌드 통과. 기능 게이트를 포함한 코드는 빌드·가짜 I/O 테스트로만 확인했다.

EFS 작업은 이 병합 범위에 포함하지 않는다. `xperia-volte-activator-efs` 워크트리와 `feat/efs-wrapper` 브랜치는 유지하며, [EfsTools 통합 조사 보고서](../../../tasks/research-efstools-integration.md)는 다음 단계의 참고 자료다.
