# RootToolsView

기준일: 2026-10-08. 코드 구현, 기본 쓰기 비활성. 전용 수동 도구 화면이며 자동 VoLTE 러너에 추가하지 않는다.

- 진입: 기기 첫 화면의 수동 작업 · 루트 엔진 / 모듈. 진입 시 대상 기기를 고정하고 기기 화면 폴링을 중지한다.
- 상태: RootState, 선택 엔진/릴리스 태그, 순정/패치 IMG, RootSwitch, 준비 패키지, 모듈 목록/재부팅/불확정 상태, 위험 확인·매니저 설정 확인, 오류/로그.
- 조회 버튼만 root_inspect/root_modules_inspect를 부른다. su 요청을 화면 진입/폴링 때 자동 생성하지 않는다.
- 엔진 전환: 순정 IMG 준비 → 명시적 정리 → 순정 양 슬롯 복원·재부팅 → 사람의 OS 확인 버튼 → 새 엔진 준비 → 별도 기록·재부팅 → 최종 확인. 중간 확인을 건너뛰지 않는다.
- ReSukiSU 목록/선택/설치: resukisu_releases → root_package_prepare(selected tag) → resukisu_install. 패치 결과는 폰에서 직접 생성하고 PC 경로+동일 폰 확인 후 root_external_patch_import.
- Magisk: 기존 prepare/patch/install. 기록은 기존 root_reboot/fastboot_getvar/fastboot_flash/fastboot_reboot와 동일 기기 게이트.
- 모듈 grid: 공개 최신 stable/동봉 준비 → 개별 설치, 상호배타/선행 조건 백엔드 재검사. root_module_action으로 끄기/제거 예약. 새 부팅 관찰 전 다음 설치 금지.
- OverlayFS 카드는 공식 GitHub 준비로 표시하고 최신 stable 검증 결과를 표시한다. 동봉 출처 배지는 AshReXcue KO·PlayStoreFix에만 해당한다.
- HMA 프리셋 PC 내보내기와 카페 JSON 그대로 가져오기 안내. 목록·scope를 직접 구성하라는 대체 안내는 제거한다. WebUI/MMRL 배포 안내, PIF Action·TrickyAddon 설정·su 승인은 폰에서 진행한다.
- 업데이트 루트 점검은 안내용 root plan만 표시하며 기록 실행 버튼으로 연결하지 않는다.
- 오류/불확정 성공 위장 없음. 기본 쓰기 gate와 Cargo feature 모두 필요하며 위험 확인 checkbox를 유지한다. 호출 중 버튼·뒤로·창 닫기를 막고 PC 보호를 해제하기 전에 native I/O 종료를 기다린다.
- 비주얼: desktop-ui 규칙, 내부 pane 스크롤, 좌측 기능/우측 독립 로그, 3열 모듈 그리드, warning/destructive/info 토큰·lucide 아이콘, 시스템 테마. 후속 소스 정책에 맞춰 OverlayFS/HMA 카드 설명과 설정 안내 문구를 갱신했다.

상세 API 계약은 [명령 문서](../02-contracts/tauri-commands.md), 처리/제한은 [루트 도구 구조](../../docs/structure/root-tools.md).
