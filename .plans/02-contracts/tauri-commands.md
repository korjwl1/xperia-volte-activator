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
//   - Xperia 전용(사용자 지시 2026-10-03): ro.product.manufacturer가 Sony가 아닌 기기는 목록에서 제외, USB 직접 연결은 VID 0x0FCE만,
//     미승인 기기는 PC에 Sony USB 장치가 있을 때만 표시
//   - state: "device"(준비) / "unauthorized"(USB 디버깅 허용 대기) / "offline" … — 준비 안 된 기기도 상태만 담아 반환
//   - firmware: ro.build.id (display.id는 " release-keys" 접미어) / productName: ro.semc.product.name 우선, 없으면 모델 표
//   - bootloader: ro.boot.flash.locked + ro.boot.vbmeta.device_state 일치 시 확정, 불일치·잠김인데 su 존재(위장 가능성)면 "unknown"
//   - rooted: su 바이너리 존재 여부 (UI "루팅 미감지" — 부재 증명 아님)
//   - serialMasked: 앞 6자 + **** (문자 단위)
//   - prep: { developerOptions(settings global development_settings_enabled), usbDebugging(adb_enabled), oemUnlockAllowed(getprop sys.oem_unlock_allowed) } — 판별 불가 null
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

## unlock / firmware (사전 준비)

```ts
invoke('read_imei1', { serial? }) → string
// ✅ 구현: service call iphonesubinfo 4 i32 0 s16 com.android.shell (getDeviceIdForPhone, phoneId 0 = SIM 슬롯 1)
//   트랜잭션 번호는 Android 버전마다 다르고 공식 문서 없음 → Android 15 / XQ-DQ44 실측 호출만, 15자리+Luhn 통과 시에만 채택
//   IMEI는 로그 금지, UI는 maskImei(앞 2·뒤 4), 전체 값은 [IMEI 복사] 클립보드에만
invoke('firmware_versions', { serial? }) → { model, installed, supported, versions: { version, android }[] }
// ✅ 구현: match/v2의 버전 목록 중 설치된 버전 이상만(최신순). 지원 기종 표에 없으면 supported=false
invoke('firmware_fetch', { serial?, partition: 'init_boot'|'boot', version?, dest? }) → { partition, path, version, fingerprint, imageBytes, downloadedBytes }
// dest: 저장 위치(사용자가 고른 폴더, 생략 시 앱 데이터 폴더) — <dest>/<model>_<version>/<partition>.img
// 받기 전에 여유 공간 확인(.sin 크기 + 16 MiB), 부족하면 "NO_SPACE|<사유>" 오류 → 프론트는 다른 위치 선택 팝업
// version 생략 = 설치된 버전(지문 완전 일치) / 지정 = 설치된 버전보다 새 버전만(지문 앞부분 기기·지역 일치 + 대상 버전 포함)
// ✅ 구현(src-tauri/src/firmware.rs): Sony 배포 서버 match/v2 → APP_SW 청크 → ZIP 끝부분 Range로 목록 → update.xml 지문 = ro.build.fingerprint 확인
//   → <partition>_*.sin만 Range로 받아 inflate·CRC → tar의 .000(ANDROID!) → 앱 데이터 폴더 firmware/<model>_<ver>/<partition>.img
//   실측(XQ-DQ44 67.2.A.3.178): 받은 용량 1.9 MB, 1.4초, 이미지 8 MB. 지원 기종 표: XQ-DQ44 JP(식별값 조회 API 폐쇄로 직접 관리)
//   실패(미지원 기종·서버 버전 불일치·지문 불일치·API 변경) 시 오류 문구 반환 → 프론트는 XperiFirm 폴더 직접 지정으로 폴백
```
조사 근거: ../tasks/research-unlock-firmware.md (언락 코드 발급은 reCAPTCHA 필수라 자동화하지 않음 — 공식 페이지를 시스템 브라우저로 열기만)

## usb / 폰 화면 (사전 준비·자동 감지)

```ts
invoke('usb_modes') → { mode: 'android'|'fastboot'|'flashmode'|'other', vendorId, productId }[]
// ✅ 구현(src-tauri/src/usbmode.rs, rusb): Sony VID 0x0FCE 장치의 디스크립터만 읽음(장치를 열지 않음)
//   fastboot = 인터페이스 FF/42/03, android = FF/42/01, flashmode = PID 0xADDE(⚠ 실기기 미검증)
//   실측: 일반 부팅 상태 → android(PID 0x320D)
invoke('open_settings_screen', { serial?, screen: 'developer'|'about' }) → void
// ✅ 구현: am start -a APPLICATION_DEVELOPMENT_SETTINGS / DEVICE_INFO_SETTINGS — 화면만 띄움(사용자 승인 2026-10-03)
```

## root (M4 — 실기기 검증된 절차, 구현은 백엔드 단계)

Magisk 자동 패치 (사용자 조작 없음) — 2026-10-03 XQ-DQ44 / Android 15 / Magisk v30.7로 검증:
1. GitHub releases API(topjohnwu/Magisk latest)에서 Magisk-v<ver>.apk 다운로드 → 앱 데이터 캐시
2. APK에서 추출: lib/arm64-v8a/libmagiskboot.so→magiskboot, libmagiskinit.so→magiskinit, libmagisk.so→magisk,
   libinit-ld.so→init-ld, libbusybox.so→busybox, assets/boot_patch.sh, assets/util_functions.sh, assets/stub.apk
3. 위 파일 + 순정 `<partition>.img`(firmware_fetch 결과)를 /data/local/tmp/<작업폴더>/ 로 push, chmod 755
4. `KEEPVERITY=true KEEPFORCEENCRYPT=true PATCHVBMETAFLAG=false RECOVERYMODE=false LEGACYSAR=false
   ./busybox sh -o standalone ./boot_patch.sh <img>` (셸 권한, 루트 불필요) → new-boot.img
   실측 로그: "Stock boot image detected → Patching ramdisk → Repack", 종료 코드 0
5. new-boot.img pull → ANDROID! 매직·크기(8 MB) 확인, 원본과 해시가 달라야 함 → 폰의 작업 폴더 삭제
6. fastboot로 <partition>_a/_b 기록(원본 CLI fastbootFlash와 동일) → 재부팅 → Magisk APK adb install
7. 검증: su 권한 요청(사용자 허용) 후 `su -c id` = uid=0

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


## journal (작업 진행 기록 — 사용자 승인 2026-10-03)

```ts
invoke('journal_save', { key, data }) → void        // <앱 데이터>/journal/<key>.json (임시 파일에 쓴 뒤 교체)
invoke('journal_load', { key }) → string | null     // 끝나지 않은 작업 기록 JSON
invoke('journal_archive', { key, tag: 'done'|'discarded' }) → void  // <key>.<tag>.json으로 보관(마지막 1개, 디버깅용)
// key = SHA-256(모델|시리얼) 앞 16바이트 hex — 파일 이름에 시리얼을 그대로 쓰지 않음, Rust에서 16~64자 hex만 허용
// data = RunJournal (types.ts): 선택 옵션·계획·단계별 상태/로그(단계당 최근 300줄)·멈춘 사유. 언락 코드·IMEI 없음
```
