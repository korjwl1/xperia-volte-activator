# 기기별 동작과 검증 메모

2026-10-08 `newflasher-add`: 새 ReSukiSU 외부 패치 입력은 모델 표의 `init_boot` 기종에 한정합니다. 이는 LKM/GKI·커널·Android 버전 호환성 검증 완료를 뜻하지 않습니다. `boot` 기종은 현재 Magisk 경로와 새 ReSukiSU 경로를 구분하며 후자는 차단합니다. 기존 XQ-DQ44/Magisk 실측은 새 엔진 감지·전환·모듈 도구의 실기기 검증으로 승계하지 않습니다. 세부 동작은 [루트 도구](structure/root-tools.md), 미검증 목록은 [단일 체크리스트](../.plans/04-engine/device-test-checklist.md)에 유지합니다.

2026-10-08 `newflasher-add`: Newflasher 네이티브 오프라인 코어와 PC 검사 API를 추가했다. **실제 기기 업데이트/USB 연결은 검증하지 않았다.** 첫 예정 profile은 XQ-DQ44 같은 지역 source→target 조합이며, 아래 기존 VoLTE/루팅 실측은 이 업데이트 엔진의 성공 근거가 아니다. [진행 기록](../.plans/04-engine/newflasher-native-progress.md).

기준일: 2026-10-08. 코드의 모델 분기와 실제 단말 검증 기록을 함께 관리합니다. **전체 GUI 호환성 테스트 완료 기기는 아직 없습니다.** 같은 기종이라도 모델·지역·펌웨어·통신사·SIM 조건이 다르면 별도로 기록합니다.

## 코드가 선택하는 알고리즘

기준 소스: `src/lib/data/devices.ts`의 `bootPartition/deviceWorkflow/modelSupport`, `types.ts`의 LGU 해석, Rust의 공통 기록/DIAG 엔진. `*`는 모델 접두사 표기이며 모든 접미사를 실측했다는 뜻이 아닙니다.

| 기기 | 모델 접두사 | 부트 대상 | 기종에 따른 처리/안내 | 실기기 범위 |
|---|---|---|---|---|
| Xperia 1 V | XQ-DQ* | init_boot | LG U+ 선택 시 LGU_V, 일반 EFS 경로 | JP XQ-DQ44 일부 단계 확인 |
| Xperia 5 V | XQ-DE* | init_boot | LG U+ 선택 시 LGU_V, 일반 EFS 경로 | 미검증 |
| Xperia 1 VI | XQ-EC* | init_boot | 일반 EFS 경로. 1 V/5 V의 LGU_V 자동 선택을 확대하지 않음 | 미검증 |
| Xperia 1 IV | XQ-CT* | boot | persist.usb.eng 보완. KT/LGU에서 외부 PDC·모뎀 대응 보고 안내 | 미검증 |
| Xperia 5 IV | XQ-CQ* | boot | 1 IV와 같은 개발 포트 보완·통신사 조건부 안내 | 미검증 |
| Xperia 1 III | XQ-BC* | boot | 리락 선택 시 Shizuku/Pixel IMS 후속 안내 | 미검증 |
| Xperia 5 III | XQ-BQ* | boot | III 리락 후속 안내 | 미검증 |
| Xperia PRO-I | XQ-BE* | boot | III 계열과 같은 외부 IMS 설정 안내 | 미검증 |
| Xperia 1 II | XQ-AT* | boot | 일반 EFS 진행 + 별도 수동 PDC 고정 안내 | 미검증 |
| Xperia 5 II | XQ-AS* | boot | II 수동 PDC 고정 안내 | 미검증 |
| Xperia 10 IV | XQ-CC* | boot | 부트 표에는 있음, modelSupport는 지원 미확인 | 미검증 |
| Xperia 10 V | XQ-DC* | boot | 부트 표에는 있음, modelSupport는 지원 미확인 | 미검증 |
| Xperia 10 VI | XQ-ES* | boot | 부트 표에는 있음, modelSupport는 지원 미확인 | 미검증 |
| 그 외, VII/VIII 포함 | 표 밖 | null | 부트 파티션을 추측하지 않고 자동 패치 차단 | 미검증 |

“일반 EFS 경로”는 알고리즘 선택입니다. 통신사 전체 호환을 보장하는 등급이 아닙니다. IV의 추가 안내는 KT/LGU 패치 선택일 때, III/PRO-I 안내는 리락 선택일 때, II 안내는 패치가 있을 때 적용됩니다. 10 계열의 부트 표와 지원 안내가 다른 점은 현재 구현 그대로 기록했습니다.

## 외부 조건의 처리 범위

- **II:** PDC 고정은 별도 수동 작업입니다. EFS 패치 경로 전체를 금지하지 않지만 앱이 PDC를 수행한 것으로 표시하지 않습니다.
- **IV:** 개발 포트를 보완합니다. KT/LGU 모뎀·PDC 대응 성공/실패 보고가 있으나 펌웨어에 따라 다릅니다. 앱은 해당 교체를 하지 않고 혼합 모뎀 리락의 위험을 안내합니다.
- **III/PRO-I:** 리락 후 외부 IMS 앱 설정 안내가 있습니다. 외부 앱 설치·설정까지 자동화하지 않습니다.
- **모든 모델:** 현재 공통 `fastboot_flash`는 fastbootd를 확인해 기록합니다. init_boot 모델만 실측됐으므로 boot 모델에서 같은 전환이 동작하는지는 별도 검증이 필요합니다. 언락·리락은 bootloader에서 수행합니다.

외부 보고를 계승한 소스 참고: [II](https://cafe.naver.com/x1smart/615332), [IV](https://cafe.naver.com/x1smart/614559), [III](https://cafe.naver.com/x1smart/615748), [PRO-I](https://cafe.naver.com/x1smart/613331). 이 링크는 앱의 실기기 성공 기록을 대신하지 않습니다.

## Xperia 1 V JP — XQ-DQ44

### 확인 환경

| 항목 | 값/범위 |
|---|---|
| 패치·루팅 세션 | 2026-10-07 |
| 펌웨어 | 67.2.A.3.178 |
| 통신사/SIM | SIM1 SKT(물리), SIM2 없음. 이전 감지 세션의 slot2 NOT_READY는 유형 unknown |
| 루팅 | Magisk 30.7, init_boot |
| 프리셋 | balance 20250901 SKT SIM1, 82개 대상, manifest SHA-256 `35e67f181a95f0126577304405575169321d6366db1d098dd320d8c59cb9d0a6` |
| 경로 | 언락 GUI, 루팅/EFS는 공유 개발 CLI 단계별. 전체 GUI 연속 실행 완료 아님 |

Android 버전·세부 PC/드라이버 버전은 해당 세션 원본에서 확인 후 추가합니다. 확인되지 않은 필드는 추측해 채우지 않습니다.

### 성공한 범위와 관찰

| 기능 | 관찰 결과 | 후속 확인 |
|---|---|---|
| ADB/IMS/SIM | 읽기 조회·물리 SIM·미확인 유형 구분, IMS 파이프 처리 확인 | 다른 기종/덤프 형식 별도 |
| Windows fastboot 드라이버 | Sony 1 V 드라이버 지정(UAC) 후 getvar 성공 | 다른 모델/PC 별도 |
| 언락 | oem unlock OKAY, 직후 unlocked=no; 부트로더 재시작 뒤 yes, OS 초기화 | 수정 엔진의 GUI 언락 전체 재실행 필요 |
| 순정 이미지·루팅 | init_boot 부분 취득·대조·패치, fastbootd 양 슬롯 기록, OS 부팅, Magisk 설치·su uid=0 | GUI 전체 루팅/다른 Magisk·boot 기종 미확인 |
| DIAG | su 개방, `05C6:90F7`, MSM/MDM/CNSS 포트 중 MSM이 hello/query 응답 | 포트 번호는 PC별. 사용자가 선택 |
| 스냅샷 | 82개 중 66개 저장·16개 기존 없음, NV 6862는 NOTACTIVE | 새 NOTACTIVE 처리의 다른 기기 검증 별도 |
| SKT EFS | 2회 업로드 오류 없음, 리드백 80/82 | 아래 차이의 원인/장기 영향은 미확정 |
| VoLTE | 4종 속성 설정·재부팅, 셀룰러 IMS registered·voice=true, LTE B7 관찰, 사용자 발신/수신 성공 확인 | 재부팅/대기 뒤 장기 유지·로밍/MMS 등 별도 |

부트로더의 init_boot 기록은 `Flashing is not allowed for partition`으로 거부돼 fastbootd를 사용했습니다. fastbootd USB ID는 `18D1:4EE0`이며 드라이버 지정이 필요했습니다. getvar:all 347개 출력으로 기존 INFO 상한을 넘은 문제, has-slot 미제공, `/debug_ramdisk/su` 경로도 이 세션에서 관찰됐습니다.

DIAG log ranges 응답과 메시지 SetMask가 기대 형식/지원과 달랐습니다. 명시적 미지원 응답만 경고로 처리하는 후속 수정이 작업 트리에 있습니다. transport/timeout 실패를 무시하는 정책은 아닙니다.

리드백 차이는 `qp_ims_service_enablement_config`와 `qp_ims_xcap_common_config`입니다. 첫 파일은 일부 바이트가 유지됐고 둘째는 이전 값이 남았습니다. 모뎀이 유지/재작성하는 것으로 보이나 원인은 확정되지 않았습니다. 사용자는 차이를 로그에 남기고 IMS를 확인하도록 결정했습니다. 읽기 실패는 계속 중단합니다. 관련 GUI/스냅샷/초기화 보완은 문서 작성 시 일부 미커밋 작업 트리 변경이며 릴리스 전체 검증은 아닙니다.

### 백업 세션 — 별도 범위

2026-10-06 해당 개발 환경에서 약 194GB 백업의 PC 전수 해시를 확인했습니다. 일부 앱 데이터의 원본 읽기 권한 거부는 기록에 남겼고 사용자가 선택한 제외 처리 뒤 보관 범위 완결을 확인했습니다. 연락처 JSON 추가 보관을 제거한 뒤 기록은 72,470개·194,253,683,751바이트입니다. 이 PC 백업은 이후 사용자 지시로 삭제됐습니다.

대용량 복사 성공은 이후 메타데이터/재연결/재개 수정이나 초기화 후 전체 복원 검증을 뜻하지 않습니다. 현재 모든 선택 항목이 완전히 복원된다는 보장은 없습니다.

### 남은 검증

언루팅, 리락, 초기화 뒤 전체 복구, 펌웨어 업데이트, 새 자동/수동/업데이트 화면, 전체 GUI 연속 실행, KT/LGU·SIM2·다른 지역판은 미완료입니다. 세부 항목의 완료 상태는 [실기기 체크리스트](../.plans/04-engine/device-test-checklist.md)에서 관리합니다.

## 업데이트·판올림의 외부 근거 — 앱 실측과 구분

| 근거 | 해당 범위/관찰 | 우리 앱에서의 상태 |
|---|---|---|
| [Hanabi 1 VI Android 15 수동 업데이트](https://cafe.naver.com/x1smart/606471), 2024-11-27 | 모뎀 제외·패치 유지, 리락 기기 진행, 중요한 자료 백업 안내. 정확한 모델/출발·목표 빌드 쌍은 글에 명시되지 않음 | 판올림에 이 전략을 적용한 근거. XQ-DQ44나 모든 1 VI 빌드의 검증으로 확대하지 않음 |
| [Hanabi Bluetooth 후속 안내](https://cafe.naver.com/x1smart/606482), 2024-11-28 | Android 15 이후 오디오 출력 문제에 개발자 옵션 조정·재부팅 안내 | 부팅/통신 성공과 주변 기능 호환을 별도 확인해야 하는 근거 |
| [앨리자 업데이트 가이드](https://cafe.naver.com/x1smart/609380) | 잠금 여부에 관계없이 순정 수동 업데이트, 데이터 유지 및 루팅 사용자의 목표 이미지 준비/후속 기록 안내. 2025-08-03 댓글에는 부팅 실패 뒤 공장초기화 선택 보고 | 데이터 보존을 보장으로 표현하지 않음. 댓글 사례의 정확한 원인은 미확정 |

XQ-DQ44의 전체 펌웨어 업데이트, Android 14→15 판올림, 데이터/VoLTE 동시 보존, 업데이트 전 Magisk 패치와 후속 기록은 모두 앱 실측 전입니다. 배포용 활성화는 실제 모델·지역·출발/목표 빌드·SIM·잠금/루트 상태별로 검증하고 [펌웨어 문서](structure/firmware.md)와 실행 체크리스트에 결과를 남깁니다.

## 새 기기/사례 기록 템플릿

기기/펌웨어/통신사 또는 알고리즘이 달라지면 새 사례를 추가합니다. 기존 성공 사례를 다른 조건으로 덮어쓰지 않습니다.

```markdown
### 기기명 — 정확한 모델 / 지역

- 날짜 / 코드 커밋 또는 미커밋 작업 트리 여부:
- 펌웨어 / Android / baseband:
- 업데이트 사례: 출발/목표 펌웨어·Android / 모델·지역 일치 / 데이터·VoLTE 유지 / 주변 기능 결과:
- Windows / 드라이버 / Magisk 버전:
- SIM 슬롯 / 물리·eSIM / 통신사·알뜰망:
- 시작 상태: bootloader / root / IMS:
- 실행 경로: GUI 또는 CLI / 실행 feature / 선택 작업:
- 부트 대상 / 사용 모드 / DIAG 포트 종류(번호는 환경 참고):
- 업데이트 루팅 정책 / 사전 패치 부모·결과 해시 / 순정 기록·후속 IMG 기록·OS 확인의 개별 결과:
- EFS 세트 / 통신사·슬롯 폴더 / SHA-256:
- 결과: 성공한 단계 / 실제 통화 / 실패·경고 / 재부팅·대기 뒤 상태:
- 알고리즘 차이와 근거: 관찰 또는 외부 보고/추정 구분:
- 수정 내용 / 재검증 결과 / 남은 미검증:
- 공개 가능한 증거와 체크리스트 링크:
```

원시 시리얼·IMEI·언락 코드·전화번호·키·개인 백업 내용은 기록하지 않습니다. 전체 호환성 완료로 표시하려면 사용 경로와 전체 절차의 검증 근거가 있어야 합니다. 새 결과가 생기면 [README](../README.md)와 해당 구조 문서도 함께 갱신합니다.
