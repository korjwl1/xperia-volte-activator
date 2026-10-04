# 언루팅·리락 게이트 (M4 후속)

status: implemented (언루팅 엔진·이력 진단 구현, 실제 리락 차단, 실기기 검증 없음)

2026-10-04 병합 후 리뷰에서 리락 허용 조건을 수정했다. [integrated-review.md](integrated-review.md)가 최신 검토 결과다.

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
- 진단 통과도 실제 리락의 허용 조건이 아니다. relock_gate_check는 항상 ok=false와 차단 사유를 반환하거나 입력/이력 오류를 반환한다.
- fastboot_lock은 모든 빌드에서 USB 접근 없이 거부한다. 프로토콜의 oem_lock 메서드는 가짜 전송 테스트에서만 컴파일된다.
- 프런트의 실전 리락은 mode-wait 재부팅 전에 차단한다. 수동 우회 옵션은 제공하지 않는다.

## 계약

```ts
invoke('relock_gate_check', { partition, stockPath, deviceKey }) → RelockGate
invoke('fastboot_lock', { confirm, partition, stockPath, expectedSerial }) → reject
```

deviceKey는 fastboot serialno의 SHA-256이다. 프런트는 선택한 기기 serial로 계산한다. ADB 전송 식별자가 fastboot serial과 다르면 실제 쓰기를 안전하게 거부하며 임의 매핑하지 않는다.

## 검증 범위

가짜 이력·가짜 전송·프런트 API를 사용해 누락/손상 이력, 슬롯 누락, 최신 실패 시도, 다른 기기, 루팅→언루팅 이력 진단과 실제 리락 차단, 재부팅 실패, 취소 중 구독 등록, 수동 IMG 전달을 검증했다. 실기기 부팅·기록·언루팅 효과·권한 동작·리락은 실행하지 않았다.

순정 출처·AVB 키·rollback index·vbmeta 및 전체 체인 증명과 실기기 검증이 완료되기 전에는 리락 차단을 해제하지 않는다.
