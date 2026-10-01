# 03 — mock 데이터 스키마 및 실측 시드

status: implemented

## 시드 출처
- 기기/프롭: 현재 실측(`tasks/recovery.md` 1-3, 2026-09-29, XQ-DQ44 / 67.2.A.3.178 / Android 15)
- 앱 분류: `tasks/recovery.md` 2-3 실측(B_OK 31/B_NO 60 + 큐레이션)
- serial은 부분 마스킹 규칙(AGENTS.md 5) 적용

## 핵심 타입 (src/lib/types.ts)

```ts
DeviceMode = 'android'|'bootloader-fastboot'|'fastbootd'|'flashmode'
DeviceStatus { serialMasked, model:'XQ-DQ44', productName:'Xperia 1 V', firmware, android,
  mode, bootloader:'locked'|'unlocked'|'unknown', rooted:bool|'unknown',
  sims: { slot, type:'physical'|'esim', carrier|null, volte:'on'|'off'|'unknown', patchedWith? }[],
  usb{topology,controller,speed} }
  // patchedWith는 DIAG 리드백(M5) 전까지 판별 불가 — 표시하지 않음
EnvCheckItem { id,label,state:'pass'|'warn'|'fail'|'info',detail,fixable:bool }
VolteConfig { sims: { slot:1|2, carrier: CarrierId|null }[], mode }   // null = 패치 안 함, LGU→LGU_V 자동(resolveCarrier)
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
  "그 외 전체 파일 시스템"(미체크 기본) — Audiobooks/Podcasts/Ringtones/Alarms 등 기기 특화 폴더는 그 외에 포함.
  표준 폴더/APK/Android/data 용량은 `storage_sizes` 실측만 사용(mock 용량 없음). 설정·통화·문자·연락처만 estBytes(추정).
  mockApps = 앱별 복구 가능성 큐레이션(실기기 app_flags와 병합)
- device.ts: 브라우저 dev 전용 (데스크톱은 실측만, mock으로 위장하지 않음). env 체크는 빈 목록
- plan.ts: 실행 계획 단일 생성기 buildPlan (00-architecture "실행 계획" 참조)
- 실행 시뮬레이션 러너는 stores/wizard.svelte.ts (진행률/로그/수동대기/USB 오류 유발 토글)
