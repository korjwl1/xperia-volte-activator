# 실기기 테스트 준비 상태 — 2026-10-05 최종 리뷰

후속 정정: 아래의 리락 무조건 차단/II·IV 일괄 제한 판단은 [기종별 워크플로우 재검토](model-workflow-recheck-20261005.md)로 대체했다. 조건부 리락 및 알려진 기종의 일반 EFS는 실기기 검증 대상으로 구현했다. 전체 OS 업데이트 엔진은 여전히 없다. 아래 검증 수치·패키지는 후속 수정 전 기록이다.

검토 기준은 `d5a5e5f` 이후의 소스이며 실제 폰·USB·COM·DIAG에 연결하지 않았다. 실행 계획, 엔진 경계·취소·재개, 부트 이미지·APK 검증, EFS/NV 전송·스냅샷·리드백, IMS 판정·통화 기록, 백업·복구·저널·패키징을 재검토했다. 원본 `../src/cliInterface.py`, `efs.py`, `adb.py`의 순서도 다시 대조했다. 10 시리즈 전용 연구·기능 추가는 범위에서 제외했다.

## 판단

지원되는 기종·통신사 조합의 **현 펌웨어 유지 → 필요 시 백업·언락·루팅 → EFS/NV 패치 → 리드백 → IMS·수동 통화 확인 → 선택적 언루팅** 경로는 단계별 실기기 검증에 들어갈 수 있다. 오프라인 검증은 전송·캐리어 호환성을 증명하지 않는다. **모든 기종·업데이트·리락까지 실기기 테스트만 남은 상태는 아니다.**

현재 자동 부트 이미지 표의 1 V/5 V/1 VI, 1 III/5 III/PRO-I, 1 IV/5 IV가 주 검토 대상이다. Mark IV는 SKT 경로만 지원하며 KT/LGU 모뎀 선행 작업 검증이 없다. Mark II PDC 절차도 없다. 표에 없는 새 세대·일본 통신사 모델은 자동 지원하지 않는다. 카페의 1 VIII 성공 후기는 원본 툴·별도 수작업/AI 실행 사례이고 우리 앱의 검증 결과가 아니다.

## 이번에 수정한 결함

1. 실전 계획에 리락이 있으면 언락·패치를 끝낸 뒤에야 리락 거부를 만날 수 있었다. 업데이트만 선택한 경로에도 일부 실전 단계와 모의 펌웨어 기록이 섞일 수 있었다. `domain/execution.ts`의 전체 계획 검사로 실행·재개·재시도 전에 미구현 리락/전체 flasher 및 모의 기기 단계 혼합을 거부한다. 타이머·PC 보호·수동 안내·기기 엔진을 시작하지 않는다. 기본 전체 모의 실행은 그대로 허용한다.
2. 부트 파티션 표에 없는 기종은 언락 후 루팅에서 실패하거나, 이미 루팅되어 있으면 모델 절차 검증을 우회하고 Sony 프리셋을 쓸 수 있었다. `patchProcedureProblem()`이 첫 EFS 입력 단계에서 확인되지 않은 모델을 거부한다. SIM 삽입 여부·감지 통신사는 사용하지 않으며 선택한 통신사/슬롯은 유지한다.

회귀 테스트는 시작 전 단계 상태·안내·엔진·타이머 미실행, 모의 실행 유지, 실전 기능 조합, 완료된 입력 체크포인트를 가진 재개, 루팅 여부와 무관한 미확인 모델 거부를 검증한다.

## 구현이 더 필요한 범위

| 범위 | 현재 동작과 남은 작업 |
| --- | --- |
| 전체 펌웨어 업데이트 | flasher 미구현. 읽기·부트 이미지 부분 다운로드를 전체 펌웨어 기록으로 간주하지 않는다. 실전 계획 시작 거부 |
| 부트로더 리락 | 항상 거부. 전송 이력·입력 이미지 해시는 순정 출처/AVB/전체 부트 체인의 증명이 아님. 해당 검증을 구현한 뒤 별도 하드웨어 검증 필요 |
| 모델별 절차 | Mark II PDC, Mark IV KT/LGU 모뎀 준비/검증, 표 밖 모델 지원 미구현 |
| 혼합 통신사 | 서로 다른 값의 글로벌 NV/EFS가 충돌하면 사전 거부. 충돌 없는 조합도 실제 모뎀 검증 필요 |
| 원상 복원 범위 | 대상 before-image만 보관. 전체 QCN/모뎀 백업·자동 롤백·시간 재적용·생성 부모 제거 미구현. 명시적 복원은 생성 역순 |
| 기기와 COM 대응 | 명시적 COM 선택 사용. COM과 ADB 폰의 일치 증명은 자동화하지 않음 |

## 자료 대조

- [Hanabi beta11](https://cafe.naver.com/x1smart/605919): 슬롯별 2회 업로드·원본 설정 4종과 Mark IV DIAG 예외를 유지했다. 중간 실패를 성공으로 처리하지 않고 리드백으로 확인한다.
- [1 VI KT 업로드 오류 사례](https://cafe.naver.com/x1smart/617265): OMD/5G 데이터가 정상이어도 EFS 업로드 실패로 IMS가 안 될 수 있었고 작성자가 오류 확인·재패치 해결을 보고했다. 파일 검증과 네트워크 판정을 분리한다.
- [리락 실패 사례](https://cafe.naver.com/x1smart/616414), [Mark IV 모뎀 사례](https://cafe.naver.com/x1smart/608340): 언루팅 또는 명령 성공만으로 안전한 리락을 증명할 수 없다. 기존 리락 기본 차단을 유지하고 이번에는 계획 시작 전으로 앞당겼다.
- [OMD 안내와 엘렌나의 KT 조건 설명](https://cafe.naver.com/x1smart/617140): 모든 회선에 사전 OMD 등록을 강제하지 않는다. EFS 기록과 OMD/APN·서비스 인증은 별도다.
- [1 VIII 최신 성공 보고](https://cafe.naver.com/x1smart/617368), [작업 후기](https://cafe.naver.com/x1smart/617135): 원본 툴과 별도 부트 이미지·언락 작업을 이용한 성공 보고. 앱의 새로운 기종 지원/리락 근거로 일반화하지 않는다.
- [최근 1 V SKT 네트워크 불가](https://cafe.naver.com/x1smart/617172): 패치하지 않은 폰의 사례. Hanabi가 슬롯 이동 시 유심보호 서비스와 SIM 자체 점검을 제안했지만 작성자의 최종 해결은 확인되지 않았다. 비슷한 증상을 EFS 오류나 SIM 손상 확정으로 취급하지 않는다.
- [XDA Xperia 패치 후 통화 성공 보고](https://xdaforums.com/t/enable-volte.4458067/): 통신사 표시·설정 메뉴는 기대와 달라도 발신/수신이 가능했다고 보고. 표시 이름/스위치를 성공 판정으로 쓰지 않는다. 국가·모델별 개인 사례다.
- [XDA 1 III 데이터/IMS 문제](https://xdaforums.com/t/solutions-for-the-issue-of-being-unable-to-transmit-data-on-a-specific-carrier.4655220/): 초기화 후 문제와 미해결 MBN 시도가 보고됐다. APN·데이터·IMS·통화를 별도로 검사하며 외국 통신사 MBN을 한국 프리셋에 추가하지 않는다.
- [Android RegistrationManager](https://developer.android.com/reference/android/telephony/ims/RegistrationManager)와 [AOSP ImsPhone](https://android.googlesource.com/platform/frameworks/opt/telephony/+/master/src/java/com/android/internal/telephony/imsphone/ImsPhone.java): 등록 상태·음성 capability·전송 방식의 구분을 유지한다. 덤프는 Sony 펌웨어별 출력 확인이 필요하며 필드/권한 부족을 실패·미확인으로 표현한다.

Orca 읽기는 최초 `runtime_unavailable` 뒤 기존 카페 탭에 재시도하여 정상 연결됐다. 새 세션·탭·프로젝트를 만들거나 다른 작업 탭을 변경하지 않았다. 최신 검색과 추가 본문/댓글을 읽었으며 보고되지 않은 해결은 추정하지 않았다.

## 오프라인 검증과 다음 단계

- 프런트 회귀 97/97 통과. Svelte/TypeScript 오류 0·경고 0. 최종 소스의 정적 프로덕션 빌드 통과.
- Rust 기본/전체 기능 각각 211 통과·8 opt-in 제외. `cargo clippy --all-targets --all-features -- -D warnings` 통과. 네이티브 EFS는 가짜/재생 COM을 사용했다.
- 최종 소스의 기본 쓰기 비활성 release 및 NSIS 0.1.0 x64 생성 통과. 설치 파일 3,432,090 B. 실행·설치하지 않았다. MSVC 라이브러리 생성 안내 stdout이 `linker_messages` 경고 1개로 표시됐으며 링크/패키징은 성공했다.
- 기존 APK·프리셋·Sony 서버 부분 다운로드에 대한 검증은 `full-review-20261005.md`의 같은 날 결과를 참조한다. 해당 기능 코드는 이번에 변경하지 않았다.
- 실제 폰·USB·COM 테스트 없음. 모든 쓰기 기능과 실전 플래그의 기본값은 그대로 꺼져 있다.

실기기 항목의 유일한 기록은 [device-test-checklist.md](device-test-checklist.md)다. 먼저 현 펌웨어 유지·리락 없음·한 슬롯의 알려진 프리셋 경로에서 읽기, DIAG, before-image, 두 번 기록·리드백, 재부팅 후 IMS, 사용자 발신/수신·양방향 음성·대기 후 수신을 단계별로 확인한다. 필요하면 백업·언락·루팅·언루팅·복구를 별도로 검증한다. 이 계획은 실행한 테스트의 기록이 아니다.

실전 검증 빌드는 계획에 필요한 모든 `REAL_STEPS`와 Cargo 쓰기 기능이 일치해야 한다. 기본 빌드는 `SIMULATED_RUN=true`, 모든 `REAL_STEPS=false`, Cargo `default=[]`를 유지하며 설치 파일을 실전 패치 완료판으로 배포하지 않는다. EFS 프리셋은 앱 내부 실행 코드와 별개로 기존 고정 해시 번들 위치를 지정해야 한다.
