# .plans — 실행 계획 문서 규칙

이 폴더는 코드 작성의 "설계도"를 유지한다. 상위 정책(`../tasks/plan.md` v4)을 실행 단위로 쪼갠 것이다.

## 구조

```
.plans/
├─ README.md              # 이 문서 (규칙)
├─ 00-architecture.md     # 런타임 아키텍처 요약 (뷰 계층/상태흐름/리스크 3층)
├─ 01-views/              # 화면별 설계 (뷰 1개 = 문서 1개)
├─ 02-contracts/          # 프론트↔백엔드 계약 (Tauri command 시그니처)
├─ 03-data/               # mock 데이터 스키마 및 실측 시드
└─ 04-engine/             # 백엔드 엔진 설계 (백업·복구 엔진 등)
```

## 규칙

1. **뷰 작성/변경 시 대응 문서를 같은 커밋에서 갱신** (AGENTS.md 규칙 1)
2. 뷰 문서에는 반드시 포함한다:
   - 목적/진입 조건
   - 표시 상태 필드 (출처 계약 명시)
   - **버튼/인터랙션 → 호출할 백엔드 계약 매핑** (`.plans/02-contracts` 참조)
   - 이탈/뒤로 가기 시 상태 보존 여부
3. 계약 문서에는 command 이름·인자·반환·이벤트 스트림을 적는다. 프론트는 이 문서 기준으로 api facade를, (M2 이후) Rust는 이 문서 기준으로 구현을 작성한다.
4. mock은 실측 데이터를 시드로 사용한다(`../tasks/recovery.md` 2-3, 현재 기기 스냅샷). 가공 데이터 금지.
5. 문서 상단에 상태 배지를 유지한다: `status: draft | implemented | stale`

최신 병합 후 검토·수정·실기기 미검증 목록: [integrated-review.md](04-engine/integrated-review.md).
