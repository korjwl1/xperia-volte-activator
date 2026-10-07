# ReSukiSU·수동 엔진 전환·모듈 구현 기록

상태: 코드 구현 / 오프라인 검증, 실기기 미검증. 기준일 2026-10-08, `newflasher-add`.

입력은 작업 폴더의 `root-method.md`, `for-rooted-phone.md` 전체입니다. 사용자 후속 답변으로 ReSukiSU 전환·모듈 기능도 구현하도록 범위를 확정했고, 카페 전용 파일은 원본·출처와 함께 저장소에 포함하도록 변경했습니다. 상위 입력 문서 자체는 이동/수정하지 않았습니다. 이 기록과 [현재 구조 설명](../../docs/structure/root-tools.md)이 저장소 안의 유지 대상입니다.

## 반영한 결정

- Magisk 최신 stable, ReSukiSU 태그 선택 및 arm64 release APK digest/서명 인증서 핀 검증.
- ReSukiSU 매니저 패치 1회는 수동. 외부 IMG 검증·순정 부모 해시·동일 기기/현재 지문·후속 fastboot 기록 게이트.
- 수동 화면에서 옛 루트가 살아 있을 때 정리 → 양 슬롯 순정 기록 → OS 재부팅/지문/새 이력 확인 → 새 엔진 → 최종 루트 재감지.
- su 거부를 비루팅/순정으로 단정하지 않는 `root_inspect`; 활성 버전과 마커 충돌 검사. KernelSU 세부 포크는 미확정.
- 공통 su 후보 확장. `/data/adb/ksud`를 su처럼 실행하지 않고 모듈 설치 CLI로만 사용.
- 모듈별 ZIP 검증, 선행 설치·상호배타 조건, intent/ACK, 재부팅 뒤 활성 관찰, 끄기·제거 예약·수동 오류 재확인.
- AshReXcue KO·PlayStoreFix 두 ZIP·HMA 카페 프리셋 동봉. HMA JSON은 수정 없이 그대로 내보내기/가져오기 안내. OverlayFS를 포함한 공개 모듈은 런타임 GitHub 최신 stable. 파일 출처/해시는 [manifest 설명](../../src-tauri/assets/root/README.md).
- 별도 `root-tools-write`와 기존 fastboot/REAL_STEPS 게이트. GUI/CLI 공유 엔진, PC 보호·호출 중 닫기/이탈 제한.
- 업데이트 루트 정책 API는 항상 쓰기 불가 안내이며 미래 버전 사전 패치/실제 플래시에는 연결하지 않음.

## 입력 문서에서 보정한 점

1. 데이터 무손실·금융앱 충분 호환을 보장하지 않는다. boot 파티션만 기록해도 부팅 실패·암호화/복구 문제로 데이터 초기화가 필요할 수 있다. 모듈 설정은 의도적으로 삭제된다.
2. `/data/adb` 마커는 잔여물일 수 있다. 버전과 권한을 함께 보고 충돌/불명을 그대로 남긴다. 루트 없는 상태에서 마커가 없다고 증명하지 않는다.
3. ReSukiSU 저장소 주소와 Zygisk Next 배포 위치가 입력 문서와 달랐다. 확인한 현재 공개 위치만 허용하며 임의 미러로 전환하지 않는다.
4. 동일 기기 외부 패치 확인은 사용자 진술이다. 헤더/해시 검증을 Sony AVB 인증이나 암호학적 생성 증명으로 표시하지 않는다.
5. ReSukiSU boot 기종/LKM·커널·판올림 호환은 확인 전이며 현재 외부 IMG 경로는 init_boot로 제한한다.
6. 관리자 앱 제거·확장 엔진 설정 정리, WebUI/MMRL 설치, PIF Action·TrickyAddon·HMA 설정은 수동 안내다. 알 수 없는 패키지 삭제와 keybox 배급을 자동화하지 않는다.

## 검증 및 재개 제한

최초 구현 때 실제 PC 준비 검증: ReSukiSU v4.2.0-rc3(자산 digest·APK 인증서 핀·ksud), 당시 공개 모듈 10개·동봉 모듈 3개 모두 검증/캐시 준비 성공. 후속 정책에서 OverlayFS가 공개 모듈로 이동해 현재는 공개 11개·동봉 2개다. NeoZygisk와 Next, PIF와 Integrity Box의 ID 공유를 발견해 영수증 소유권 검사·기록 없는 충돌 차단을 추가했다. Zygisk Assistant의 기존 공식 릴리스는 digest 필드가 없어 해당 저장소/태그/파일명만 고정 SHA-256으로 bootstrap했다. 다른 무해시 자산 거부는 유지한다. Shamiko의 XZ ZIP 지원을 추가하고 Cargo.lock도 갱신했다. 원본 바이트를 바꾸지 않았다.

2026-10-08 소스 정책 후속: OverlayFS의 include_bytes/고정 핀과 저장소 사본을 제거하고 RipperHybrid/Meta-Overlayfsx 최신 stable 조회로 전환했다. HMA JSON은 원본 해시를 유지하고 수동 scope 재구성 안내를 제거했다. PlayStoreFix는 ZIP 스크립트와 공개 BKI v1.4/v1.6.1을 비교했고 공개 v1.6.1 ZIP의 ABX 변환기 10개·LICENSE가 카페판과 동일함을 확인했다. 직접 부모 통합본/수정 이력은 확보하지 못했으므로 작성자별 변경을 단정하지 않는다. [상세 조사](../../docs/structure/play-store-fix.md)를 유지한다.

후속 변경 검증: 기본 쓰기 비활성 Rust root_tools 회귀 12 passed(공식 OverlayFS 출처·digest 누락 거부 포함), 프론트 148 passed, Svelte check 0 errors / 0 warnings. 쓰기 feature 없이 개발 CLI를 재빌드하여 새 PC 캐시에 공식 OverlayFS v1.3.4 다운로드·API 해시/크기·ZIP 검증 성공(external=false, moduleId=meta-overlayfsx)을 확인했다. 같은 CLI의 HMA 내보내기는 카페 원본 SHA-256과 일치했다. 변경 문서 9개·로컬 링크 41개·Mermaid 2개·UTF-8 검사 통과. 실제 폰 작업과 기본 게이트 변경은 없다.

기본 Rust 331 passed / 10 ignored, 쓰기 feature 포함 Rust 346 passed / 10 ignored 및 CLI 통합 3 passed. ignored 실기기/네트워크 검사를 실행하지 않았다. 전체 프론트·타입 검사·프로덕션 빌드와 문서 검증 결과는 최종 검증 기록을 따른다. 브라우저에서 새 수동 화면·그리드·별도 로그 pane·기본 쓰기 비활성을 확인했다.

최종 프론트 148 passed, Svelte check 0 errors / 0 warnings, 프로덕션 정적 빌드 성공. HMA PC 내보내기 원본 해시 일치와 기존 파일 덮어쓰기 거부도 개발 CLI로 확인했다. 변경 문서 18개의 로컬 링크 90개·Mermaid 6개와 UTF-8을 검사했고, 동봉 프리셋에 식별정보 필드가 없는 것을 확인했다. 후속 전환 기록의 잘못된 단계/대상/해시 차단은 root_tools 회귀로 확인한다. 신규 Rust 파일에만 rustfmt를 적용했으며 기존 미포맷 파일 전체를 다시 쓰지 않았다.

FakeADBDevice 테스트는 권한/버전/마커, 모듈 선행·상호배타, 설치 실패 intent 보존, 재부팅 전 다음 설치 거부, 새 부팅 뒤 활성 확인, 현재 지문 불일치 정리 거부, 정리 실패 재시도 거부, 과거 양 슬롯 이력 차단, 다른 기기·원본 동일 이미지 가져오기 거부를 다룬다. facade 테스트는 브라우저 성공 위장 방지·기본 쓰기 차단·선택 태그 보존·fastboot 실패 뒤 자동 재기록/재부팅 없음이다.

정리 실패의 cleanup-intent는 자동 재개하지 않는다. 새 패치 기록 실패도 기존 fastboot 이력의 미완료/실패로 남는다. 사용자 데이터 복원·초기화·재언락을 복구 수단으로 자동 실행하지 않는다. 실기기 항목은 [단일 체크리스트](device-test-checklist.md)에 남긴다.
