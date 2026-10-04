# 루팅 엔진 (M4) — Magisk 자동 패치·기록·설치 구현 설계

status: implemented (실기기 검증 대기 — 최신 통합 리뷰: integrated-review.md, root/fastboot 양쪽 실행 게이트 필요)

- 상위 정책: `tasks/plan.md` §12(Magisk 산출물 근거 강화)·M4 마일스톤, §3-3(의존성)
- 절차 근거: `.plans/02-contracts/tauri-commands.md` root 절 — **2026-10-03 XQ-DQ44·Android 15·Magisk v30.7 실기기 검증 절차**(사용자 조작 없음)
- 의존 구현: firmware_fetch(순정 이미지, ✅) · fastboot_flash(양 슬롯 기록, ✅ feat/fastboot-unlock) · root_check(검증, ✅)

## 범위

**구현 (In)**
1. firmware.rs ZIP 리더 제네릭화(`RangeRead` 트레이트) + 로컬 파일 구현 — APK 페이로드 추출에 재사용(신규 `zip` 크레이트 불필요)
2. `src-tauri/src/magisk/` 모듈 — Magisk APK 확보(GitHub releases)·페이로드 추출·기기 스테이징·boot_patch.sh 실행·결과 검증·pull·정리
3. Tauri 명령: `magisk_prepare`(다운로드) · `magisk_patch`(스테이징+패치+검증) · `magisk_install`(APK 설치) · `root_reboot`(adb 재부팅)
4. 기록은 기존 `fastboot_flash` 재사용(이미 `fastboot-write` feature 게이트) — 새 코드 없음
5. wizard 루팅 단계 실전 연결(패치→부트로더→기록→재부팅 대기→설치→su 승인) — `REAL_STEPS.root` 기본 꺼짐 + Cargo feature `root-write` **이중 게이트**(fastboot-write 패턴 계승)

**제외 (Out)**
- 실기기 테스트(전부) — FakeADBDevice + ZIP 픽스처 단위 테스트
- 언루팅(순정 이미지 재기록 + Magisk 앱 제거 안내 — 별도 설계), 커스텀 boot 이미지 사용(계약상 "출처 불명" 취급 유지)

## 검증된 절차 (2026-10-03 실측 — 계약 문서 그대로)

1. GitHub releases API(topjohnwu/Magisk latest) → `Magisk-v<ver>.apk` 다운로드 → 앱 데이터 캐시
2. APK에서 추출(arm64): libmagiskboot.so→magiskboot, libmagiskinit.so→magiskinit, libmagisk.so→magisk,
   libinit-ld.so→init-ld, libbusybox.so→busybox, assets/boot_patch.sh, assets/util_functions.sh, assets/stub.apk
3. 도구 + 순정 `<partition>.img`를 기기 작업 폴더로 push, chmod 755
4. `KEEPVERITY=true KEEPFORCEENCRYPT=true PATCHVBMETAFLAG=false RECOVERYMODE=false LEGACYSAR=false
   ./busybox sh -o standalone ./boot_patch.sh <img>` (셸 권한, 루트 불필요) → new-boot.img
   실측 로그: "Stock boot image detected → Patching ramdisk → Repack", 종료 코드 0
5. new-boot.img pull → `ANDROID!` 매직 확인 · 크기 ≤ 원본(파티션 적합) · 해시 ≠ 원본(실측은 8 MiB 정확히 일치) → 기기 작업 폴더 삭제
6. fastboot `<partition>_a/_b` 기록(기존 fastboot_flash) → 재부팅 → Magisk APK adb install
7. 검증: su 승인(사용자) 후 `su -c id` = uid=0 — 기존 root_check + su-grant 수동 개입

## 모듈 구조

```
src-tauri/src/magisk/
├─ mod.rs      — tauri 명령(4종)·feature 게이트(root-write)·GitHub 다운로드(D12 패턴)
├─ patch.rs    — 핵심 로직(전부 &mut dyn ADBDeviceExt 주입형): 스테이징·패치 실행·검증·pull·정리
└─ (firmware.rs) — RangeRead 트레이트 + LocalZip + zip_extract_named(제네릭화)
```

- 기기 작업 폴더: `/data/local/tmp/xvolte-magisk` **고정 경로만** rm -rf (smsie 원칙 동일)
- 부팅 파티션은 프론트 `bootPartition(model)` 계약 그대로 명령 인자로 받음(init_boot/boot)

## 계약 (02-contracts root 절 정정)

```ts
invoke('magisk_prepare') → { version, apkPath, sha256 }        // GitHub latest 다운로드(캐시 재사용) — 기기 무관
invoke('boot_image_check', { serial, path, fingerprint }) → string // 현재 펌웨어 대조 + 이미지 해시
invoke('magisk_patch', { request: { serial, apkPath, imagePath, partition, imageSha256, fingerprint, apkSha256 } }) → PatchResult
//   스테이징 → boot_patch.sh → 검증(ANDROID!·크기≤원본·해시≠원본) → pull → 작업 폴더 정리
//   PatchResult = { path, origSha256, patchedSha256, bytes, log: string[] }
//   이벤트 'magisk:log': string — 패치 스크립트 출력
invoke('magisk_install', { serial, apkPath, apkSha256 }) → void // 입력 APK 재검사 후 adb install
invoke('root_reboot', { serial, target: 'os'|'bootloader' }) → void   // adb reboot (fastboot_reboot의 adb 짝)
// 기록: fastboot_flash(partition, path, confirm, expectedSerial, expectedSha256) 재사용
// 패치·설치는 root-write + REAL_STEPS.root. ADB 재부팅은 root-write 또는 fastboot-write 필요.
```

- magisk_prepare는 펌웨어 다운로드(firmware_fetch)와 같은 성격 — 준비 단계라 게이트 밖(기기 무관, 캐시 쓰기만)

## wizard 연결 (root 단계, REAL_STEPS.root)

```
[자동] boot_image_check(현재 버전 지문·이미지 확인) → magisk_prepare(다운로드·버전 로그)
[자동] magisk_patch(이미지 = firmware_fetch 결과 경로)
[자동] root_reboot(bootloader) → fastboot 모드 폴링(기존 usbModeIs)
[자동] fastboot_flash(partition, patched) — fastboot-write 게이트 통과분
[자동] fastboot_reboot(os) → adb 재연결 대기(기존 usbDebugReady 폴링)
[자동] magisk_install(APK)
[수동] su-grant — 기존 프레임워크(확인 버튼 = root_check 실측, 자동 감지 폴링)
```
- 실전 su-grant는 패치·기록·설치 후 런타임에 추가한다. 재시도/재개 시 안내를 초기화해 준비 전에 승인을 요청하지 않는다. 실전 root/fastboot 중 하나라도 켜져 있으면 목업 건너뛰기를 제공하지 않는다.

## 테스트 전략 (실기기 없이)

1. ZIP 픽스처 빌더(stored 방식 + EOCD/central directory 직접 구성) → LocalZip + zip_extract_named 왕복
2. 페이로드 추출: 항목명 매핑(lib/arm64-v8a/... → 기기 측 이름), 누락 항목 오류
3. patch.rs (FakeADBDevice): 스테이징 push·chmod·스크립트 실행(실측 출력 재생)·new-boot pull(ANDROID! 매직)·정리(rm -rf 고정 경로만)
4. 검증 실패 분기: 매직 없음 / 해시 동일(원본과 동일 = 패치 안 됨) / 크기 초과 → 각각 오류, 작업 폴더는 항상 정리
5. GitHub 파서: 릴리스 JSON에서 `Magisk-v*.apk` 자산 선택(디버그·기타 자산 제외)

## 자체 리뷰 반영 (2026-10-04)

- **셸 보간 제거**: 기기 측 부트 이미지명은 고정 `boot.img`. 결과는 내용 해시 기반 `magisk/patched-<sha256>.img`로 원자 저장한다.
- **원자 다운로드**: magisk_prepare가 임시 파일→rename으로 저장 — 중단 시 반쪽 APK가 캐시로 오인되지 않게. 256 MiB 상한(디스크 채우기 방어)
- **크기 하한**: 패치 결과 ≥ 원본/2 — 매직·해시 검증만으론 9바이트 가짜가 통과할 수 있었음
- **실패 판정 정합화**: must_ok는 종료 코드 + stderr만(기존 settings.rs 규칙) — 출력에 "not found" 문자열이 있으면 실패 오판하던 휴리스틱 제거
- **waitUntil**: 단일 질의·독립 제한 시간·취소 감시로 예외와 멈춘 질의에도 종료한다.
- **sub 체크포인트**: 실전 루팅도 시뮬레이션과 같은 6단위로 갱신 + 기록·설치 후 즉시 journal 저장

## 리스크·검증 대기

| 항목 | 상태 | 대응 |
|---|---|---|
| boot_patch.sh 출력·종료 코드 판정 | 실측 1회(v30.7) | 종료 코드 0 + new-boot.img 검증(매직·크기·해시)으로 판정 — 로그 마커는 참고용만 |
| 타 Magisk 버전·기종 | 미실측 | 버전·모델별 성공 기록은 journal에 남긴다(추후 EFS_VERIFIED 패턴) |
| init_boot 외 파티션(boot) 크기 규칙 | 미실측 | 크기 ≤ 원본 규칙 + 일치 시 로그("원본과 동일 크기") |
| GitHub 릴리스 자산명 변형 | 미실측 | `Magisk-` prefix + `.apk` 필터, 실패 시 오류로 사용자 안내 |

## 구현 순서 (커밋 단위)

1. ✅ `docs(plans)`: 이 문서 + AGENTS 예외 + 02-contracts root 절 정정
2. ✅ `refactor(firmware)`: RangeRead 트레이트 제네릭화 + LocalZip + zip_extract_named(테스트 포함)
3. ✅ `feat(magisk)`: 페이로드 추출·patch.rs 핵심(FakeADBDevice 테스트)·GitHub 다운로드
4. ✅ `feat(magisk)`: Tauri 명령 4종(root-write feature 게이트) + facade + REAL_STEPS.root
5. ✅ `feat(front)`: wizard 루팅 단계 실전 연결(모드 전환 2회·su-grant 마무리·waitFor 폴링)
6. ✅ `docs(plans)`: 상태 배지 갱신
