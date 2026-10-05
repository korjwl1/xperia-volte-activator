# 기종별 워크플로우·조건부 리락 재검토 — 2026-10-05

사용자 지시: 실기기 미검증과 항상 차단을 구분하고 조건부 리락을 구현한다. 삭제된 상세 가이드 대신 내려받은 Hanabi 도구의 실제 소스와 자동화툴 사용자 보고를 대조한다. 인식한 기종에 맞게 작업을 자동 조정하며 모든 기종의 예외 기능을 새로 구현하지 않는다. SIM·통신사는 사용자 선택을 유지하고 10 시리즈 전용 작업은 추가하지 않는다.

이 문서와 unroot-relock.md가 현재 정책이다. 이전 integrated/full/readiness/scope 리뷰의 리락 무조건 차단 및 II/IV 일괄 거부 판단을 대체한다. 실기기/USB/COM 테스트는 수행하지 않았으며 REAL_STEPS와 Cargo 쓰기 기본값은 꺼짐을 유지한다.

## 루팅과 전체 펌웨어 업데이트

원본 root()는 사용자가 boot/init_boot를 고르고 해당 SIN을 UnSIN으로 변환한 뒤 Magisk로 패치해 양 슬롯에 기록한다. OS 전체를 새 버전으로 설치하는 작업이 아니다. 전체 ZIP을 받아도 이 경로에서 사용할 이미지만 추출하면 작업은 동일하다. 반면 설치된 OS A에 펌웨어 B의 부트 이미지만 넣는 것은 정상적인 업데이트가 아니다.

우리 firmware_fetch는 순정 부트 이미지 부분 다운로드/추출이며 현재 지문 대조가 있다. 새 버전 선택·계획 생성은 있으나 전체 ZIP 다운로드/플래시 엔진은 없다. 이번 변경에서 전체 OS 업데이트는 구현하거나 실행하지 않았다. [Magisk 공식 설치 절차](https://topjohnwu.github.io/Magisk/install.html)는 boot/init_boot 패치·기록을 설명한다.

## 내려받은 beta11 소스 확인

상위 작업 폴더의 src/cliInterface.py, adb.py, efs.py 및 src와 util의 Python·BAT·INI를 읽고 PDC/MBN/SoftBank/Pixel IMS/Shizuku/모뎀 작업을 검색했다. 바이너리는 실행하지 않았다.

| 파일 | SHA-256 |
| --- | --- |
| src/cliInterface.py | A88B53A59AFF3B637005CC11C561D8174D8E94C2A4C71959C0BF721C756DC5A6 |
| src/adb.py | 7F5187CA21B9CDE1A996D8E80B578656D8193AF5F30EA7064549AF544B59A1DD |
| src/efs.py | E7D05CCE40CABD5BAF10C0CBB37A2FD3724C72D307C45715066037BC9D640B80 |

- 원본은 PDC 프로파일 고정, SoftBank 모뎀 교체, Pixel IMS/Shizuku 설정을 자동 수행하지 않는다. EFS 포트 개방 → 사용자 선택 슬롯별 EFS 2회 업로드 → persist.dbg 4종 설정 경로다.
- 원본 efsPortOpen()에는 모델 분기나 persist.usb.eng 자동 실행이 없다. Mark IV의 보완은 Hanabi가 [배포 글 605919](https://cafe.naver.com/x1smart/605919)의 댓글 58627342에서 별도 adb shell/su/setprop persist.usb.eng 1로 안내했다. 기존 Rust diag.rs의 자동 보완은 이 수동 해결법을 반영한 것이며 원본 자동 코드로 오인하지 않는다.
- 원본 unRoot()는 순정 이미지 양 슬롯 복원, lockBootloader()는 oem lock이다. 우리 앱은 이미지/파티션/현재 펌웨어 및 결과 상태 검사를 결합한다.

## 삭제 여부와 독립적인 사례

Orca의 기존 Sony 카페 탭을 읽기 전용으로 사용했다. 서버 응답 기준 2026-10-05 10:40 UTC에 613331은 200, 그 안의 상세 절차 링크 598708·591068은 404였다. 상위 tasks/cafe-research-20261005/scope-availability.json과 articles.json에 공개 본문/댓글을 기록했다. 쿠키·회원 키는 저장하지 않는다.

| 출처 | 직접 확인한 내용 | 판단 범위 |
| --- | --- | --- |
| [615332](https://cafe.naver.com/x1smart/615332), Hanabi 댓글 | 5 II에서 자동화·수동 작업 실패를 보고한 사용자에게 EFS 포트 개방 직후 별도 터미널에서 수동 고정 작업을 안내 | 원본에 PDC 자동 구현이 없다는 근거이며, 앱에 새 PDC 자동화를 추가하지 않음 |
| [613584](https://cafe.naver.com/x1smart/613584) | II 사용자가 자동화 EFS 이후 실패/풀림을 보고. 댓글에서 PDC와 고정 순서를 논의 | 고정 작업 안내 필요. 모든 II가 패치 불가능하다는 근거가 아님 |
| [607891](https://cafe.naver.com/x1smart/607891), 앨리자·엘렌나 댓글 | 1 IV LGU에서 최신 자동화 후 발신 실패. 별도 모뎀 교체 및 user-agent 초기화 문제 설명 | 원본만으로 해결되지 않은 실제 사례. 일괄 교체/차단 근거로 확대하지 않음 |
| [614559](https://cafe.naver.com/x1smart/614559), 앨리자 댓글 | 5 IV에서 Hanabi 도구 사용 후 KT 외 수신 실패. PDC 후속 작업 안내 | IMS/5G만으로 실제 통화 성공을 판단하면 안 됨 |
| [614397](https://cafe.naver.com/x1smart/614397) | IV LGU 사용자가 최신 업데이트 후 EFS/토글만으로 발신·수신 성공을 보고 | 모뎀 교체가 모든 IV의 필수 조건이라는 판단을 반박. 버전/변경 이력 상세가 부족하므로 모든 최신 펌웨어의 해결로 단정하지 않음 |
| [615150](https://cafe.naver.com/x1smart/615150) | IV KT 5G 문제 해결 후 EFS 추가 덮어쓰기로 통화 복구. 댓글에는 PDC로 대응한 경험도 있음 | 단일 모뎀 교체 절차를 유일한 해결법으로 강제하지 않음 |
| [615748](https://cafe.naver.com/x1smart/615748), 엘렌나 댓글 | III 사용자 자동화 후 통화 실패에 EFS 일부 덮어쓰기 누락 가능성을 설명. Shizuku 적용만으로도 해결되지 않았다는 본문 | 전수 리드백·실제 통화 확인 유지. 모든 실패를 Pixel IMS 부족으로 단정하지 않음 |
| [608133](https://cafe.naver.com/x1smart/608133), Hanabi 댓글/작성자 후속 보고 | 5 V 리락 실패에 잘못된 이미지 안내를 수정. 작성자는 자기 펌웨어와 맞는 순정 이미지로 리락 성공 보고 | 현재 펌웨어·순정 이미지·파티션 대조 필요. 리락 영구 차단 근거가 아님 |

## 구현 결과

- deviceWorkflow는 인식 모델과 사용자가 고른 통신사/리락만 입력받는다. boot/init_boot, IV 개발 포트 보완에 맞는 계획 설명, II 외부 PDC 안내를 선택한다. 순정 이미지 다운로드·루팅·언루팅은 동일한 partition getter를 사용한다.
- IV 네트워크 주의는 KT/LGU 선택 때만, III/PRO-I 후속 앱 설정 안내는 패치와 리락을 선택했을 때만 표시한다. 계획 확인 화면도 같은 결과를 사용하며 SIM 감지로 입력을 바꾸지 않는다.
- II/IV의 일반 EFS 경로를 허용하고 PDC·모뎀 준비 여부 일괄 차단을 제거했다. 미확인 부트 파티션과 프리셋 충돌·실전/모의 엔진 혼합은 계속 거부한다. 새로운 PDC·모뎀 교체·외부 앱 설치 엔진이나 수동 확인 단계를 추가하지 않았다.
- 리락은 현재 OS/이미지 지문 대조 → 추출 출처/양 슬롯 최신 done 이력 → 부트로더 진입 → 동일 serial/모드/슬롯/이력 재검사 → oem lock(300초) → 명시적인 unlocked=no → OS 재부팅 응답 확인이다. 리락만 선택해도 순정 양 슬롯 복원을 수행한다.
- 이 게이트는 앱의 순정 부트 이미지 복원 조건을 확인한다. 외부 모뎀·커널·vbmeta 변경, AVB 서명/rollback index/전체 체인 또는 실제 파티션 리드백을 인증하지 않는다. 실기기 검증 부족만으로 항상 거부하던 정책을 없앤 것이다.

## 오프라인 검증

실제 store/API를 가짜 호출로 검증하고 Rust FakeTransport/이력/이미지 픽스처로 조건 통과와 거부·상태 미확인을 검사했다. 최종 프런트 101/101, svelte-check 0 errors/0 warnings, Rust 기본·all-features 각각 214 passed/8 ignored(222개), clippy all-targets/all-features -D warnings 통과. 모든 기본 쓰기 플래그/feature는 비활성이다.

정적 프런트 생산 빌드와 기본 feature release/NSIS 패키징 성공. 산출물은 main의 src-tauri/target/release/bundle/nsis/xperia-volte-activator_0.1.0_x64-setup.exe, 3,434,863 bytes, SHA-256 EEFDD041074676B3E198DB5A5AAD649AD07EB295A79DCB344C64E57383DE4A84. 설치·앱 실행·기기 테스트는 하지 않았다. 로그는 src-tauri/target/relock-workflow-package-20261005.log에 있다. 이 설치 파일의 쓰기 기능은 기본 비활성 상태다.
