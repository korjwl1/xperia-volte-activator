# 루팅과 언루팅

기준일: 2026-10-07. 담당: `magisk/mod.rs`, `magisk/patch.rs`, `firmware.rs`, `boot_image.rs`, `apk_verify.rs` 및 위자드의 root/unroot 러너.

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
