# 언루팅·리락 게이트 (M4 후속)

2026-10-05 최종 리뷰: 2026-10-05 적대적 리뷰 보강: boot_image_check는 기기 키·이미지 해시·추출 파티션·펌웨어 지문의 대조 기록을 저장한다. Magisk 결과도 순정 SHA-256 출처와 기기 대조 기록을 저장한다. fastboot_flash/fastboot_lock은 이 기록과 실제 전송 버퍼를 검사하므로 CLI의 임의 해시만으로 통과할 수 없다. device_list에서 다른 펌웨어를 관찰하면 이전 기록은 사용할 수 없다. 날짜만으로 임의 유효 기간을 만들지는 않는다. 손상 이력은 확인 후 flash_history_archive로 원본을 보관할 수 있지만 순정 양 슬롯을 다시 기록하고 검증해야 리락할 수 있다. 외부 도구의 미관찰 변경이나 전체 AVB를 인증하는 기능은 아니다.


status: implemented / gated (조건부 리락·언루팅 구현, 쓰기 기본 비활성, 실기기 검증 없음)

2026-10-05 사용자 지시에 따라 무조건 차단을 제거하고 원래 §3-3의 조건부 경로로 복귀했다. 이번 변경은 [model-workflow-recheck-20261005.md](model-workflow-recheck-20261005.md)가 최신 기준이다. 과거 integrated-review의 무조건 차단 정책은 대체되었다.

## 언루팅 절차

원본 unRoot의 순정 IMG 양 슬롯 기록과 Magisk 앱 수동 삭제 안내를 계승한다. `REAL_STEPS.root && REAL_STEPS.fastboot` 및 백엔드 쓰기 feature가 필요하다.

1. 자동 다운로드 IMG 또는 수동 SIN에서 추출한 raw IMG 확보. 수동 입력은 update.xml 지문 필수이며 SIN 후보가 여러 개면 거부한다.
2. ADB 상태에서 boot_image_check로 현재 펌웨어 지문·이미지 기본 형식/크기를 확인하고 SHA-256을 확보한다.
3. root_reboot(bootloader)와 모드 대기. 실패·취소·제한 시간 초과 시 중단한다.
4. fastboot_flash에 expectedSerial과 expectedSha256을 전달해 같은 기기·같은 버퍼인지 확인한 뒤 양 슬롯 기록. 슬롯별 started/done/failed 이력을 동기 저장한다.
5. fastboot_reboot(os)의 실제 성공 응답과 같은 기기의 ADB 복귀를 확인한다.
6. root_check가 여전히 uid=0을 반환하면 실패 처리한다. 조회 실패는 미확인 로그를 남긴다. 사용자가 Magisk에서 루트 해제 여부를 확인하고 앱을 직접 삭제한다. 앱 삭제는 자동 수행하지 않는다.

펌웨어 업데이트가 시뮬레이션인 계획과 실전 부트 기록을 섞지 않는다. 한 슬롯만 성공하면 실패로 중단하며 자동 롤백은 구현되지 않았다.

## 이력 진단과 실제 리락

flash-history.jsonl은 전송 이력이다. 이력 해시가 입력 이미지와 같아도 순정 출처·서명·AVB·전체 부트 체인이나 실제 리드백을 증명하지 못한다. ANDROID! 매직도 순정 인증이 아니다.

- 파일 없음·읽기 실패·16 MiB 초과·잘못된 UTF-8·손상 JSON·필수 값 누락·잘못된 해시/상태는 실패 처리한다. 손상된 줄을 건너뛰지 않는다.
- 현재 기기의 64자리 식별 키가 있어야 진단한다. 다른 기기의 완료 이력을 섞지 않는다.
- 해당 파티션 양 슬롯의 최신 항목이 done이고 입력 해시와 같아야 슬롯 진단을 통과한다. 이력 없는 슬롯은 unknown으로 실패한다. 이후 started/failed가 있으면 이전 done으로 통과시키지 않는다.
- 같은 기기의 다른 파티션 이력이 있으면 단일 이미지로 전체 체인을 증명할 수 없다고 판정한다.
- relock_gate_check는 이미지 종류/크기, 추출 때 저장한 출처·파티션·SHA-256, 기기별 양 슬롯 최신 done 이력을 검사하고 만족하면 ok=true를 반환한다.
- 프런트는 Android 상태에서 boot_image_check로 현재 폰 지문과 추출 이미지 지문을 다시 대조한다. 이후 이력 게이트가 통과해야 root_reboot(bootloader)를 수행한다. 계획의 relock에는 선행 mode-wait를 두지 않는다.
- fastboot_lock은 confirm=true와 fastboot-write가 필요하다. 로컬 출처·이력을 다시 읽고, 동일 fastboot serial·is-userspace=no·has-slot=yes 및 실제 serial 해시로 게이트를 다시 확인한다. unlocked=yes면 oem lock을 보내고, 명시적인 unlocked=no가 확인되어야 성공한다. 상태 조회 실패를 잠김으로 처리하지 않는다.
- 리락 응답은 초기화를 고려해 언락과 같은 300초 상한을 사용한다. OS 재부팅 응답 확인 뒤에만 단계를 완료하며, 초기화 뒤 기본 설정/최종 통신 확인을 별도로 진행한다.
- 리락만 선택한 경로도 루트 감지 결과와 관계없이 현재 펌웨어의 순정 이미지 양 슬롯 복원을 포함한다. 수동 우회는 추가하지 않았다.

## 계약

```ts
invoke('relock_gate_check', { partition, stockPath, deviceKey }) → RelockGate
invoke('fastboot_lock', { confirm, partition, stockPath, expectedSerial }) → { unlocked: false } | error
```

deviceKey는 fastboot serialno의 SHA-256이다. 프런트는 선택한 기기 serial로 계산한다. ADB 전송 식별자가 fastboot serial과 다르면 실제 쓰기를 안전하게 거부하며 임의 매핑하지 않는다.

## 검증 범위

가짜 이력·가짜 전송·프런트 API로 추출 출처 누락/변조/파티션 불일치, 현재 펌웨어 불일치, 누락/손상/다른 폰/최신 미완료 이력, 동일 기기/bootloader/슬롯 확인, 조건 통과 후 oem lock, 잠금 상태 미확인·재부팅 실패·취소를 검증했다. 실기기 부팅·기록·언루팅 효과·리락은 실행하지 않았다.

게이트는 앱이 추출·기록한 부트 이미지의 복원 조건을 확인하는 기능이다. AVB 키·rollback index·vbmeta·외부 도구의 변경 및 전체 체인/실제 파티션 리드백을 인증하지 않는다. 기존 무조건 차단은 제거했으며 기본 쓰기 플래그/feature는 실기기 검증 전까지 꺼둔다.
