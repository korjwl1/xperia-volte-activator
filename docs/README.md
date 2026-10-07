# 프로젝트 문서

현재 코드의 책임, 기능별 동작, 기종별 차이와 검증 범위를 설명합니다. 최근 갱신일은 **2026-10-08**입니다. 각 문서의 기준일과 구현 상태를 확인하세요. 구현 계획과 실제 기능을 구분합니다.

| 문서 | 내용 |
|---|---|
| [시스템 구조](structure/overview.md) | 전체 형상, 실행 계층, 기능 상태 |
| [작업 흐름](structure/workflow.md) | 화면·계획 생성·조건별 생략·새 분기 계획 |
| [기기 감지와 연결](structure/device-detection.md) | ADB/USB/COM, 인증, 기종·SIM·루트 판별 |
| [백업과 복구](structure/backup-restore.md) | 수집, 영수증·해시·완결 검사, 재개·복원 제한 |
| [부트로더](structure/bootloader.md) | 언락·리락, 모드, 양 슬롯 기록과 출처/이력 게이트 |
| [루팅과 언루팅](structure/root-unroot.md) | Magisk 준비·패치, 순정 이미지 복원, 모델별 파티션 |
| [수동 루트 엔진과 모듈](structure/root-tools.md) | ReSukiSU 선택·수동 패치, 엔진 전환, 모듈 조건·기록·동봉 출처 |
| [VoLTE 패치](structure/volte.md) | 프리셋·DIAG·EFS/NV·리드백·기종별 추가 조건 |
| [펌웨어](structure/firmware.md) | 부트 부분 다운로드·SIN 추출·업데이트 정책 |
| [통신 확인](structure/communication.md) | IMS/VoLTE 판정, 실제 통화 기록, SIM별 검증 |
| [기록과 복구](structure/state-recovery.md) | journal·실행권·PC 보호·중단·재개 |
| [개발과 검증](structure/development.md) | 빌드, 실행 게이트, 공유 엔진 CLI, 테스트 |
| [기기별 메모](devices.md) | 모델별 알고리즘 분기, 실기기 사례, 기록 템플릿 |

`docs/structure`는 실제 코드 설명을 유지합니다. `.plans/`는 변경 계획과 세부 검증 목록을 유지합니다. “계획”으로 표시된 항목은 사용 가능한 기능이 아닙니다. 변경 시 [AGENTS.md](../AGENTS.md)의 문서 유지 규칙을 따릅니다.

업데이트 관련 자료: [시작 경로·백업·루팅 유지 계획](../.plans/04-engine/workflow-modes-20261007.md), [Newflasher 네이티브 설계](../.plans/04-engine/newflasher-native.md), [`newflasher-add` 오프라인 구현·검증 범위](../.plans/04-engine/newflasher-native-progress.md).
