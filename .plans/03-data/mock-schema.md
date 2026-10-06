# 03 — mock 데이터 스키마 및 실측 시드

2026-10-05 최종 리뷰: BackupSummary.deviceKey는 nullable SHA-256 기기 키이며 초기화 게이트에서 선택 기기와 대조한다. SmsIeOutcome.cleanupWarning은 nullable 문자열이다. journalLoad facade는 ApiResult<string|null>로 없음과 오류를 구분한다.


status: implemented

2026-10-05: `DeviceStatus`에 optional `baseband/observedAtMs`, `SimInfo`에 optional `ims: ImsDiagnostic`을 추가한다. 진단은 등록·음성/SMS·접속 방식·기술 및 사유를 분리한다. mock은 slot1 no-sim, slot2 not-registered이며 실측처럼 정상 통화를 주장하지 않는다. `RunJournal.communication`은 before/latest 스냅샷과 슬롯별 `CallCheck`(outgoing/incoming/audio/afterReboot/afterIdle)를 저장한다. 스냅샷에는 시각·기종·펌웨어 지문·Android·베이스밴드·SIM 요약·선택 프리셋 버전/전체 SHA256만 넣고 시리얼·IMEI·전화번호·구독 식별자·원시 덤프는 넣지 않는다. 실제 발신·수신·음성 확인은 사용자 체크이며 자동 통화가 아니다.

2026-10-04: 실제 실행 계획은 `src/lib/domain/plan.ts`로 이동했다. 공통 `ApiResult<T>`와 nullable 진행 파일 필드는 `types.ts`, 디스크 진행 기록의 런타임 검증은 `domain/journal.ts`를 사용한다. mock은 개발용 데이터와 백업 항목 시드만 제공한다.

## 시드 출처
- 기기/프롭: 현재 실측(`tasks/recovery.md` 1-3, 2026-09-29, XQ-DQ44 / 67.2.A.3.178 / Android 15)
- 앱 분류: `tasks/recovery.md` 2-3 실측(B_OK 31/B_NO 60 + 큐레이션)
- serial은 부분 마스킹 규칙(AGENTS.md 5) 적용

## 핵심 타입 (src/lib/types.ts)

```ts
DeviceMode = 'android'|'bootloader-fastboot'|'fastbootd'|'flashmode'
DeviceStatus { serialMasked, model:'XQ-DQ44', productName:'Xperia 1 V', firmware, fingerprint?(ro.build.fingerprint — 백엔드 실측, mock에는 없음), android,
  mode, bootloader:'locked'|'unlocked'|'unknown', rooted:bool|'unknown',
  sims: { slot, type:'physical'|'esim'|'unknown', carrier|null, volte:'on'|'off'|'unknown', patchedWith? }[],
  usb{topology,controller,speed} }
  // patchedWith는 DIAG 리드백(M5) 전까지 판별 불가 — 표시하지 않음
EnvCheckItem { id,label,state:'pass'|'warn'|'fail'|'info',detail,fixable:bool }
VolteConfig { sims: { slot:1|2, carrier: CarrierId|null }[], firmware: string|null, bootloaderAction: "unlock"|"relock"|null }   // null = 패치 안 함, LGU→LGU_V 자동(resolveCarrier), bootloaderAction = 부트로더만 작업(패치·업데이트 없을 때만 유효)
ManualId += 'unlock-code'|'firmware-select', PlanStep.manual: ManualId[], RunStep.manualDone
Profile = 'clean-return'|'keep-root'|'unroot-only'
PlanStep { id,kind,title,desc,optional,enabled,risk:'safe'|'warn'|'danger',wipe:bool,manual?:ManualId[] }   // kind += 'volte-props'
BackupGroup { id,label,items:BackupItem[],bytes }
BackupItem { id,label,cls:'full'|'partial'|'none',note?,checked,estBytes? }   // estBytes = 실측 불가 항목의 고정 추정치
AppItem { pkg,label,cls,allowBackup,note? }
RunStep { id,title,status:'pending'|'running'|'done'|'failed'|'skipped'|'manual-wait',progress,logs[],manualDone }
DeviceStatus += state:'device'|'unauthorized'|'offline'|'usb'…,  SimInfo += state(gsm.sim.state 원값)
```

## mock 시드 (src/lib/mock/*)
- device.ts 시드: 잠김/비루팅/VoLTE off/SIM2=SKT eSIM — "신규 상태"(매트릭스 1행)
- apps.ts: 백업 그룹 4카테고리(설정/앱/파일/통화 및 문자, 항목 단위 checked). 통화 및 문자 = 통화 기록/문자/연락처 3항목.
  파일은 표준 폴더 7(DCIM/Download/Pictures/Movies/Music/Documents/Recordings) +
  "그 외 전체 파일 시스템"(다른 항목과 같은 기본값) — Audiobooks/Podcasts/Ringtones/Alarms 등 기기 특화 폴더는 그 외에 포함.
  표준 폴더/APK/Android/data 용량은 `storage_sizes` 실측만 사용(mock 용량 없음). 설정·통화·문자·연락처만 estBytes(추정).
  mockApps = 앱별 복구 가능성 큐레이션(실기기 app_flags와 병합)
- device.ts: 브라우저 dev 전용 (데스크톱은 실측만, mock으로 위장하지 않음). env 체크는 빈 목록
- plan.ts: 실행 계획 단일 생성기 buildPlan (00-architecture "실행 계획" 참조)
- 실행 시뮬레이션 러너는 stores/wizard.svelte.ts (진행률/로그/수동대기/USB 오류 유발 토글)
- EFS facade 브라우저 mock(`src/lib/api/efs.ts`): EfsUploadResult/EfsVerifyReport에 planned/skipped/warnings 추가. KT의 빈 NV 두 항목은 mock에서도 filesSeen/files/matched에서 제외. `simulation` 경고는 기기 결과가 아님을 명시한다. 실제 Tauri native 실패는 mock 성공으로 대체하지 않는다. REAL_STEPS.efs 기본 false로 기존 진행 시뮬레이션 유지.
- 패치 계획에 `efs-input`(기기 접근 전 설정·프리셋 확인)을 먼저 포함한다. `efs-preflight`의 수동 안내는 `su-grant`이며 세부 작업은 DIAG 전환·프로토콜 초기화·응답 확인. EFS 설정 화면은 mock 기본 모드에서 숨긴다.
- EFS facade의 설정 기반 메서드는 선택적 실행별 `EfsConfiguration` 인자를 받는다. 이를 지정하면 이후 PC 설정 변경이 해당 실행의 COM·프리셋·스냅샷 루트를 바꾸지 않는다.
- `EfsToolCheck`는 `deviceExecution`·`rootExecution`·`fastbootExecution`을 반환한다. 브라우저 mock은 모두 false, Rust는 각각 Cargo 쓰기 feature의 컴파일 여부를 반환한다.

- RunJournal v1 optional imsVerified/callVerified boolean 추가. 잘못된 타입은 거부, 이전 기록은 최종 통신 확인 재실행. 시뮬레이션의 단계 완료만으로 실제 IMS·통화 확인 플래그를 세우지 않는다.
2026-10-06 SimInfo.type에 unknown 추가. 구독 정보가 없거나 충돌할 때 슬롯 순서로 물리/eSIM을 가정하지 않는다. mock의 미삽입 slot1도 unknown이며 관찰한 slot2 구독만 esim이다. 기록 로더는 unknown을 그대로 보존한다.

2026-10-06 단계 대기: RunJournal v1의 optional `awaitingNext: string|null`은 cursor 직전 완료/스킵 단계 id와 대조한다. optional `backupOmissions: {apps: BackupSummary.omittedApps, pending:boolean}`은 제외 안내의 확인 여부를 유지하며 구조·개수·바이트·boolean 타입을 검사한다. 구형 기록은 필드 없이 읽을 수 있다. mock도 메인 단계 완료 후 자동으로 진행하지 않고 [다음]을 기다린다.
