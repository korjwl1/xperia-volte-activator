# view: RootTools — 수동 매니저 변경 / 모듈 세트

status: implemented / live default disabled (2026-10-08)

수동 그리드의 root-manager/root-modules로 진입하며 section=manager/modules를 각각 표시한다. 대상 모델·시리얼을 고정하고 호출 중 뒤로/창 닫기를 막는다. 조회 버튼으로 rootInspect/rootModulesInspect를 요청하며 폰 su 승인은 사람이 한다.

매니저 전환은 기존 순정 준비 → 모듈/엔진 정리 → 양 슬롯 순정 복원 → 순정 부팅 확인 → 새 매니저/패치 → 적용 → 최종 확인이다. Magisk는 기존 prepare/patch/install, ReSukiSU는 태그 선택·서명 검증 APK·같은 폰의 수동 패치 결과 검증을 사용한다. 미완료 전환 중 모듈 설치는 native에서도 차단한다.

모듈 section은 ModuleSetSelector의 A/B 세트, B→A 자동 의존, Zygisk/Integrity 택일, 선택 추가 모듈을 표시한다. 설치 버튼은 위험·매니저 설정 확인, REAL_STEPS.root + root-tools-write, 정상 inventory와 재부팅 완료가 필요하다. 공통 installModuleSets를 순차 실행하고 설치별 OS 재부팅/활성 확인 및 ModuleInstruction 폰 설정 확인을 거친다. 실패/중단 뒤 다음 모듈을 설치하지 않는다.

상태: root/inventory/caps/선택 엔진·태그/순정·패치 IMG/전환 기록/세트 선택/설정 안내·resolve/취소/위험 동의/로그·오류. installed 영수증으로 중복 설치만 생략하며 폰 설정은 재확인한다. HMA 원본 JSON 내보내기, WebUI/MMRL 설치 안내와 모듈 비활성/제거/오류 재확인을 유지한다.

비주얼: MD3 톤·lucide·시스템 테마·pane 스크롤, 좌 기능/우 로그. 모듈은 세트 카드·의존 배지·선택 드롭다운, 매니저 변경은 별도 section. 첫 화면의 별도 루트 도구 버튼은 제거했다.

[계약](../02-contracts/tauri-commands.md) · [현재 동작](../../docs/structure/root-tools.md).
- 2026-10-09 (사용자 지적): 모듈 화면의 [동봉 HMA 프리셋 PC 저장]·[WebUI 설치 안내]·[MMRL 설치 안내]를 없앴다. HMA 단계에서 카페 프리셋을 폰 Download에 자동으로 넣고(`root_preset_push`) 가져오기만 안내한다. WebUI는 TrickyAddon이 스스로 설치한다. [재부팅]은 끄기·제거 예약 뒤 재부팅이 남았을 때만 보인다.
- 2026-10-09 (사용자 요청): 매니저 설정 자동화(`root_manager_setup`) — KernelSU 계열은 `ksud feature set selinux_hide 1`+`feature save`(모듈 마운트 해제 기본값은 커널 기본값 켜짐), Magisk는 Zygisk·DenyList 적용 끄기+카페 HMA 프리셋 대상 중 설치된 앱 DenyList 등록. PIF Action은 `root_module_run_action`(KSU: `ksud module action`, Magisk: busybox sh action.sh). 실패하면 기존 폰 안내로 대체. 화면을 열면 권한·엔진·모듈을 자동 조회하고, [전환 기록 불러오기]·[업데이트 루트 유지 점검] 버튼은 없앴다. 폰에서 직접 할 것은 TrickyAddon(keybox)·HMA 가져오기 두 가지.
