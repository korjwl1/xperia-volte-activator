# 02 — Tauri command 계약 (프론트 ↔ 백엔드)

status: 진행 중 — 읽기 전용 일부 구현 (`adb_status`, `device_list`, `storage_sizes`: `src-tauri/src/adb.rs`). 나머지는 mock/계약만.

규칙: 프론트는 `src/lib/api/` facade로만 호출한다. command 이름/인자/반환은 이 문서가 단일 진실 공급원이다.
이벤트 스트림은 Tauri `emit` → facade의 콜백/스토어로 전달된다.
**구현 원칙(AGENTS.md 현재 단계)**: 기기·PC에 영향 주는(쓰기·설치·삭제·플래시) 명령은 작성 금지. 읽기 전용만 구현.
**기기 통신 구현체**: `adb_client` 크레이트(ADB 프로토콜 순수 Rust) — 1순위 실행 중인 adb 서버(5037) 재사용, 2순위 USB 직접 연결. adb 바이너리 설치/경로 탐색 불필요.

## device (M1)

```ts
invoke('adb_status') → AdbStatus                   // ✅ 구현: 연결 수단 점검 { available, mode: "adb-server"|"usb-direct"|"none", detail }
invoke('device_list') → DeviceStatus[]            // ✅ 구현: adb_client 연결 + getprop 덤프 + which su (읽기 전용)
invoke('device_status', { serial }) → DeviceStatus   // §4 상태 감지 통합 (adb 프롭/프로브) — 미구현
// 이벤트: 'device:changed' → { serial, mode }  // WM_DEVICECHANGE/폴링 (§9-3) — 미구현
```

## storage (M3 선제 — 백업 탭 실측 용량)

```ts
invoke('storage_sizes', { serial? }) → Record<string, number>
// ✅ 구현: 단일 셸 실행(adb_client) — 폴더별 du 병렬 + 3자 앱 APK 크기(stem) + df 여유 공간
// 반환 키: dcim/download/pictures/movies/music/documents/recordings/
//          android-data/sdcard-total/sdcard-free/
//          apk-total/apk-count/apk-sampled
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
