# 02 — Tauri command 계약 (프론트 ↔ 백엔드)

status: 진행 중 — 읽기 전용 구현 (`adb_status`, `device_list`, `storage_sizes`, `app_flags`: `src-tauri/src/adb.rs` / `disk_free`: `host.rs`). 나머지는 mock/계약만.

규칙: 프론트는 `src/lib/api/` facade로만 호출한다. command 이름/인자/반환은 이 문서가 단일 진실 공급원이다.
이벤트 스트림은 Tauri `emit` → facade의 콜백/스토어로 전달된다.
**구현 원칙(AGENTS.md 현재 단계)**: 기기·PC에 영향 주는(쓰기·설치·삭제·플래시) 명령은 작성 금지. 읽기 전용만 구현.
**기기 통신 구현체**: `adb_client` 크레이트(ADB 프로토콜 순수 Rust) — 1순위 이미 실행 중인 adb 서버(5037, TCP 포트 확인으로만 판단) 재사용, 2순위 USB 직접 연결. adb 바이너리 설치/경로 탐색 불필요.
  - adb_client는 로컬 서버 연결 시 `adb start-server`를 실행하므로, 존재하지 않는 adb 경로를 넘겨 바이너리 실행을 차단한다(서버를 새로 띄우지 않음).

## device (M1)

```ts
invoke('adb_status') → AdbStatus                   // ✅ 구현: 연결 수단 점검 { available, mode: "adb-server"|"usb-direct"|"none", detail }
invoke('device_list') → DeviceStatus[]            // ✅ 구현: adb_client 연결 + getprop 덤프 + which su + dumpsys isub + TelephonyDebugService (읽기 전용)
//   - state: "device"(준비) / "unauthorized"(USB 디버깅 허용 대기) / "offline" … — 준비 안 된 기기도 상태만 담아 반환
//   - firmware: ro.build.id (display.id는 " release-keys" 접미어) / productName: ro.semc.product.name 우선, 없으면 모델 표
//   - bootloader: ro.boot.flash.locked + ro.boot.vbmeta.device_state 일치 시 확정, 불일치·잠김인데 su 존재(위장 가능성)면 "unknown"
//   - rooted: su 바이너리 존재 여부 (UI "루팅 미감지" — 부재 증명 아님)
//   - serialMasked: 앞 6자 + **** (문자 단위)
//   - SIM state: gsm.sim.state 원값 — ABSENT=미삽입, PIN_REQUIRED 등은 그대로 전달(프론트 simStateLabel)
//   - SIM type: dumpsys isub "Active subscriptions" 구간의 isEmbedded 실측 (구독 없는 슬롯은 1=물리/2=eSIM 가정)
//   - SIM volte: TelephonyDebugService 덤프의 ImsPhone mMmTelCapabilities Voice(*#*#4636#*#* IMS 상태와 동일 출처)
//       "on"=IMS 음성 등록 / "off"=미등록 / "unknown"=덤프 판독 불가·SIM 없음 (실측 ~1.2s)
//   - SIM carrier: gsm.sim.operator.alpha 쉼표 구분 슬롯 값 ("[,SK Telecom]"), 비어 있으면 <key>.2 폴백
//   - USB 직접 연결로 2대 이상 감지 시 셸을 열지 않고 자리표시 항목(serialMasked "USB #n")만 반환 → 다중 기기 경고
//   - 실패 시 facade는 null 반환(빈 목록과 구분) — 프론트는 연속 2회 실패 시에만 "기기 없음" 처리
invoke('device_status', { serial }) → DeviceStatus   // §4 상태 감지 통합 (adb 프롭/프로브) — 미구현
// 이벤트: 'device:changed' → { serial, mode }  // WM_DEVICECHANGE/폴링 (§9-3) — 미구현
```

## storage (M3 선제 — 백업 탭 실측 용량)

```ts
invoke('storage_sizes', { serial? }) → Record<string, number>
// ✅ 구현: 단일 셸 실행(adb_client) — 폴더별 du 병렬 + 3자 앱 APK 크기(split APK 포함, 합산은 Rust — 기기 셸 산술 32비트) + df
// 반환 키: dcim/download/pictures/movies/music/documents/recordings/
//          android-data/sdcard-total/sdcard-free(폰 여유 — 백업 위치 판단에는 쓰지 않음)/
//          apk-total/apk-count
// APK: pm list packages -3 -f의 설치 폴더에서 *.apk 일괄 stat, du와 병렬 — 실측(XQ-DQ44) 91개 앱 10,620,520,740 B, 전체 약 4.0s (이전 9.5s)
// facade 실패 시 null → UI "측정 불가"(mock 값으로 대체하지 않음)
```

기기 대상 명령 공통: serial 지정 시 정확히 일치하는 기기만 사용(USB 직접 연결은 연결 후 ro.serialno 대조), 미지정 시 준비된 기기가 1대일 때만 실행(다른 기기로 폴백 금지). 프론트는 wizard.device.serial을 전달한다.
USB 직접 연결은 한 번에 한 작업만 가능(최대 30초 대기) — 프론트는 앱 목록 → 설정 개요 → 용량 순으로 순차 호출한다.
USB 직접 연결 인증 키: 표준 ~/.android/adbkey(또는 ANDROID_USER_HOME) 있으면 사용, 없으면 앱 데이터 폴더(app_local_data_dir)/adbkey에 1회 생성(PKCS8 PEM) 후 재사용 — 매 연결마다 폰 허용 팝업이 뜨지 않도록 (사용자 승인 2026-10-02, 유일한 PC 쓰기)

## apps (읽기 전용)

```ts
invoke('app_flags', { serial? }) → { pkg, allowBackup, hasExternalData, googleBackedUp }[]
// ✅ 구현: pm list packages -3 + dumpsys package packages(pkgFlags ALLOW_BACKUP) + ls /sdcard/Android/data
//          + dumpsys backup(현재 구글 백업 전송의 "pkg - N state bytes", N>0 = 실제 백업 기록), 실기기 ~1.5s
invoke('settings_overview', { serial? }) → { systemCount, secureCount, globalCount, restoreItems[{namespace,key,label,value}], batteryExemptApps }
// ✅ 구현: settings list 개수 + 자동 복원 대상 7키(settings get) + dumpsys deviceidle whitelist의 user 항목 수, 실기기 ~0.6s
// 분류 규칙(AI·추정 없음, 모든 폰 공통) — src/lib/data/appRules.ts
//   lost(데이터 소실 주의) = 알려진 OTP 앱 등(특정 폰 무관한 앱별 사실) / restored = 외부 데이터 존재 / relogin = 그 외(앱만 재설치)
//   relogin 안내 문구는 실제 구글 백업 기록(googleBackedUp) 유무로 구분. 공개 앱 백업 DB는 없음(2026-10 조사)
// facade api.appClasses(): 큐레이션(mockApps) 우선 병합, 없으면 allowBackup ? 불완전 : 불가 (recovery.md 2-2)
```

## host (PC 측, 읽기 전용)

```ts
invoke('disk_free', { path }) → number   // ✅ 구현(src-tauri/src/host.rs): 경로가 속한 드라이브 여유 바이트, 경로 없음 → 에러
// 폴더 선택: @tauri-apps/plugin-dialog open({ directory: true }) — capability dialog:allow-open
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
