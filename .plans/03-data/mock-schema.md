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
  volte { enabled, ims:'registered'|'none'|'unknown', reason? }, sims[], usb{topology,controller,speed} }
EnvCheckItem { id,label,state:'pass'|'warn'|'fail'|'info',detail,fixable:bool }
Profile = 'clean-return'|'keep-root'|'unroot-only'
PlanStep { id,kind,title,desc,optional,enabled,risk:'safe'|'warn'|'danger',wipe:bool,manual?:ManualId }
BackupGroup { id,label,items:BackupItem[],bytes }
BackupItem { id,label,cls:'full'|'partial'|'none',note?,checked }
AppItem { pkg,label,cls,allowBackup,note? }
RunStep { id,title,status:'pending'|'running'|'done'|'failed'|'skipped'|'manual-wait',progress,logs[] }
```

## mock 시드 (src/lib/mock/*)
- device.ts: 잠김/비루팅/VoLTE off/SIM2=SKT — "신규 상태"(매트릭스 1행)
- apps.ts: 백업 그룹 4카테고리(설정/앱/파일/메시지, 항목 단위 checked).
  파일은 표준 폴더 7(DCIM/Download/Pictures/Movies/Music/Documents/Recordings) +
  "그 외 전체 파일 시스템"(미체크 기본) — Audiobooks/Podcasts/Ringtones/Alarms 등 기기 특화 폴더는 그 외에 포함.
  표준 폴더/APK/Android/data 용량은 `storage_sizes` 실측으로 갱신(도착 전엔 프론트 스켈레톤)
- env.ts: WebView2 pass, 드라이버 pass, adb pass, 프리셋 warn(미설정), QPST info(미설치=정상)
- plan.ts: clean-return 기본 단계열 (백업→언락→루팅→EFS+검증→언루팅→리락→최종검증→복구)
- run.ts: 시뮬레이션 러너 (진행률/로그/수동대기/오류 유발 토글)
