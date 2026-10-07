# 루팅과 언루팅

기준일: 2026-10-08. 담당: `magisk/mod.rs`, `magisk/patch.rs`, `firmware.rs`, `boot_image.rs`, `apk_verify.rs` 및 위자드의 root/unroot 러너.

## 부트 파티션 선택

`src/lib/data/devices.ts`의 모델 접두사 표가 GUI 계획의 기준입니다. CLI 호출자는 이 표에 맞는 파티션을 명시해야 하며, CLI 자체가 GUI 전체 분기를 실행하지 않습니다.

| 모델 접두사 | 대상 |
|---|---|
| XQ-DQ / XQ-DE / XQ-EC | `init_boot`: 1 V / 5 V / 1 VI |
| XQ-CT / XQ-CQ / XQ-CC / XQ-AT / XQ-AS / XQ-BC / XQ-BQ / XQ-BE / XQ-DC / XQ-ES | `boot`: IV·II·III·PRO-I·10 IV/V/VI |
| 표에 없는 모델 | 확인 불가, 자동 패치 차단 |

현재 Android 버전만으로 선택하지 않습니다. Xperia 10 V/VI도 코드의 `boot` 표를 따릅니다. 표에 있다는 것이 전체 기종 검증 완료라는 뜻은 아닙니다.

## Magisk 루팅

```mermaid
sequenceDiagram
  participant W as 위자드 또는 CLI
  participant R as Rust 엔진
  participant P as Xperia
  W->>R: 현재 순정 이미지/펌웨어 대조
  W->>R: 검증된 Magisk APK 준비
  R->>P: 도구 및 순정 IMG 스테이징
  R->>P: boot_patch.sh 실행
  P-->>R: new-boot.img
  R->>R: 헤더/크기/해시/출처 검사
  W->>R: 대상 모드/같은 기기 확인 후 양 슬롯 기록
  R->>P: 재부팅 및 Magisk 앱 설치
  P-->>W: 사용자 su 승인 / uid=0 확인
```

APK는 릴리스 자산 digest·크기와 인증서 핀을 확인하고 캐시합니다. 인증서 핀 일치 자체를 APK 암호 서명 검증이라고 표현하지 않습니다. 설치 시 Android가 서명을 검증합니다. 오프라인이면 검증된 캐시만 사용합니다.

기기에서 고정 스테이징 경로와 셸 권한으로 패치 스크립트를 실행합니다. `KEEPVERITY=true`, `KEEPFORCEENCRYPT=true`, `PATCHVBMETAFLAG=false` 등 현재 옵션을 사용합니다. 외부 입력을 셸 명령으로 직접 보간하지 않습니다.

결과는 적절한 부트 헤더, 원본 대비 크기 범위, 원본과 다른 SHA-256, 파티션 적합성으로 검사합니다. 결과 출처에는 순정 부모 이미지·지문·파티션을 남깁니다. 일부만 성공하면 루팅 완료로 표시하지 않습니다.

XQ-DQ44의 `init_boot` 기록은 fastbootd에서 수행합니다. bootloader의 기록 거부를 무시해 같은 명령을 반복하지 않습니다. 모드·드라이버·슬롯 검사는 [부트로더](bootloader.md)를 참조하세요.

## 루트 권한 확인

`su -c id`의 uid=0이 근거입니다. 현재 Magisk 환경에서는 PATH에 su가 없고 `/debug_ramdisk/su`만 있을 수 있으므로 공통 `device_io::su!` 경로를 사용합니다.

폰 화면이 잠겨 있거나 꺼져 있으면 승인 창을 볼 수 있는 상태를 먼저 확인합니다. Magisk에서 Shell 권한을 거부한 경우 승인 설정을 안내하며, 거부/응답 없음은 루트 없음이나 작업 성공으로 바꾸지 않습니다.

## 언루팅

현재 설치 버전의 순정 이미지를 대조한 뒤 양 슬롯에 재기록하고 OS 복귀·루트 상태를 확인합니다. Magisk 앱 삭제 등의 수동 안내가 남을 수 있습니다. 다른 버전 IMG를 임의로 덮어쓰지 않습니다.

리락 요청이면 순정 복원이 필수 선행 조건입니다. 언루팅만으로 리락이 자동 실행되는 것은 아닙니다. 새 버전 업데이트 뒤 루팅 재적용에는 새 버전의 이미지가 필요합니다. 전체 업데이트 기록 엔진은 아직 미구현입니다.

XQ-DQ44/Magisk 30.7 CLI 단계별 루팅은 확인됐으나 다른 모델·Magisk 버전과 전체 GUI·언루팅 검증은 분리해서 기록합니다. [기기별 기록](../devices.md)과 [실기기 체크리스트](../../.plans/04-engine/device-test-checklist.md)를 확인하세요.

## 예정: 업데이트 전 이미지 패치

업데이트 안내·공통 백업 선택 후 루팅 유지가 선택되면 **목표 버전**의 순정 boot/init_boot를 업데이트 전에 같은 폰에서 패치하고 PC에 보관할 계획입니다. 이미지 생성 자체와 부트 기록 권한은 별개입니다. 이미지 패치는 기존 루트가 없어도 가능하지만 수정 IMG 기록/부팅에는 unlocked 상태가 필요합니다. 이미 언락돼 있어도 비루팅 사용자의 기본 선택은 비루팅 유지입니다. 잠금을 유지하려고 별도 패치를 할 필요는 없습니다.

Newflasher의 순정 SIN 기록과 Magisk raw IMG 기록은 별도 단계입니다. 수정 IMG를 SIN으로 이름 변경/삽입하거나 이전 버전 패치 IMG를 남기지 않습니다. 같은 목표 펌웨어의 순정 부트를 Newflasher로 기록한 뒤 검증된 fastboot/fastbootd 경로에서 새 패치 IMG를 적용합니다. [Newflasher 소스](https://github.com/munjeni/newflasher/blob/master/newflasher.c), [Magisk 설치 문서](https://topjohnwu.github.io/Magisk/install.html)가 각 입력/기록 경로의 근거입니다.

현재 `magisk_patch_with_events`는 `boot_image::verify_fingerprint`로 **현재 설치 지문**을 검사합니다. 따라서 미래 버전 사전 패치를 기존 API에 그대로 넘기는 흐름은 아직 불가능합니다. 출발 지문·목표 펌웨어의 검증된 출처·파티션·순정 부모/패치 해시·같은 기기를 묶은 준비 계약을 추가하고 일반 루팅의 지문 검사는 유지해야 합니다.

검증된 조합에서는 Newflasher 후 직접 fastboot로 전환해 OS 첫 부팅 전에 기록할 수 있도록 설계합니다. 순정 기록 완료 증명·기기/모드/슬롯 검사와 현재 API의 제약을 해결한 뒤에만 활성화합니다. 미검증 조합은 순정 OS로 부팅해 목표 지문을 확인한 후 기존 기록 경로로 준비 IMG를 적용합니다. boot 기종의 모드 검증을 init_boot 기종 결과로 대신하지 않습니다.

기존 Magisk 유지 또는 이미 unlocked인 기기의 명시적 신규 루팅 선택에만 이 단계를 넣고, locked 기기에 재언락·리락을 추가하지 않습니다. 지원하지 않는 루팅 방식·판별 불가 상태는 유지 가능으로 표시하지 않습니다. vbmeta 검증 해제·암호화 옵션 변경을 기본 추가하지 않고 목표 Android의 Magisk/모듈 호환을 따로 확인합니다.

순정 업데이트 뒤 패치 기록이 실패하면 업데이트 성공과 루팅 유지 실패를 따로 표시하고 검증된 이미지/진행 기록을 보존합니다. 해당 경로는 구현/실기기 검증 전입니다. [분기 계획](../../.plans/04-engine/workflow-modes-20261007.md)의 상태·재개·검증 기준을 따릅니다.
