# 02 — Tauri command 계약 (프론트 ↔ 백엔드)

status: draft (프론트는 mock으로 구현, 계약만 확정. M2+에서 Rust 구현)

규칙: 프론트는 `src/lib/api/` facade로만 호출한다. command 이름/인자/반환은 이 문서가 단일 진실 공급원이다.
이벤트 스트림은 Tauri `emit` → facade의 콜백/스토어로 전달된다.

## device (M1)

```ts
invoke('device_list') → DeviceInfo[]            // USB VID 0x0FCE 스캔 (모드 포함)
invoke('device_status', { serial }) → DeviceStatus   // §4 상태 감지 통합 (adb 프롭/프로브)
// 이벤트: 'device:changed' → { serial, mode }  // WM_DEVICECHANGE/폴링 (§9-3)
```

## env (M1 — §12.6)

```ts
invoke('env_check') → EnvCheckItem[]            // WebView2/드라이버/adb서버/번들해시/프리셋/디스크/QPST(정보)
invoke('env_fix', { id }) → FixResult           // WebView2 부트스트래퍼, PNPUTIL 상승 등 자동 수리
```

## plan (M2)

```ts
invoke('plan_generate', { profile, toggles, deviceStatus }) → PlanStep[]   // §3-2 매트릭스 + §3-3 의존성
invoke('fastboot_getvar', { serial }) → Record<string,string>              // read-only 프로브
// 파괴적: fastboot_oem_unlock / fastboot_oem_lock — 실행 전 confirm 인자 필수
```

## backup (M3)

```ts
invoke('backup_scan_items', { serial }) → BackupGroup[]     // 분류기(§6-4): allowBackup+폴더+큐레이션
invoke('backup_estimate', { groups, path }) → Estimate      // 항목별/합계 용량
invoke('backup_run', { groups, path }) → void               // 이벤트: 'backup:progress' {item, bytes, total}
invoke('restore_run', { backupDir }) → void                 // 이벤트: 'restore:progress'
```

## EFS (M5)

```ts
invoke('efs_preflight', { serial }) → PreflightReport       // §10-5: 토폴로지/스피드/전원/DIAG 건전성
invoke('efs_snapshot', { serial, paths }) → SnapshotRef     // before-image (전수)
invoke('efs_upload', { serial, presetDir }) → void          // 이벤트: 'efs:progress' {file, n, total}
invoke('efs_verify', { serial, snapshot }) → VerifyReport   // 전수 리드백 해시
invoke('efs_rollback', { serial, snapshot }) → void
```

## flasher / session (M6)

```ts
invoke('fw_prepare', { fwDir, opts }) → StagedDir           // §8 스테이징 (원본 불변)
invoke('newflasher_run', { staged }) → void                 // 이벤트: 'flasher:output'
invoke('session_save' | 'session_load' | 'session_resume')  // §9-1/9-2 상태+프로브
```

## 공통 이벤트 페이로드

```ts
type StepEvent = { stepId: string; phase: 'start'|'progress'|'done'|'fail'|'manual-wait'; progress?: number; log?: string }
```
