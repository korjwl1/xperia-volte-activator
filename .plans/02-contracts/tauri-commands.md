# 02 — Tauri command 계약 (프론트 ↔ 백엔드)

2026-10-05 추가: `flash_history_archive({ confirm: boolean }) -> string`은 PC의 손상 flash-history.jsonl을 별도 파일로 보관한다. 확인 없으면 실패하며 정상 이력은 삭제하지 않는다. 공유 쓰기 잠금으로 기기 변경과 동시 실행하지 않고 USB를 열지 않는다. 보관 후에는 새 순정 양 슬롯 기록이 필요하다.

2026-10-05 개발 CLI: 기존 Tauri 명령 시그니처·반환·이벤트 이름은 유지한다. AppHandle이 필요한 엔진은 `events::Events`를 받는 공통 실행 함수에 위임하고 `dev-cli` 실행 파일도 같은 함수를 호출한다. CLI 입력/출력·명령 카탈로그는 별도 개발 계약이며 일반 앱에 CLI 명령을 추가하지 않는다. 세부 사항은 `../04-engine/dev-cli.md`.

2026-10-05 `device_list` 읽기 전용 결과 확장: DeviceOut는 `baseband: string`, `observedAtMs: number`를 제공한다. 각 SIM의 `ims`는 `status`, `registration`, nullable boolean `voice/sms`, `transport`, `technology`를 제공한다(`src/lib/types.ts`). `status`는 no-sim/sim-not-ready/query-failed/unsupported-format/conflicting-evidence/not-registered/registering/voice-unavailable/registered/wifi-only/cross-sim/other-network/transport-unknown 중 하나다. 구형 `imsRadioTech`와 신형 `imsTransportType` 로그를 지원하며 현재 미등록 상태·최근 해제/등록 중 이벤트·모순된 근거를 정상 판정에 사용하지 않는다. 조회 명령은 원시 출력에서 종료 상태/권한 오류를 판별하고 필요한 줄만 반환한다. 기존 `volte=on`은 셀룰러 IMS 음성 준비 요약으로 유지하며 LTE/NR 및 실제 통화 성공은 별도 결과다. 새 쓰기 명령 없음. 작업 전/완료 후 진단은 기존 `api.deviceList()` facade를 재사용한다.

status: 구현 — 등록된 명령 전체는 `src-tauri/src/lib.rs`. 기기·PC에 쓰는 명령도 구현돼 있지만 Cargo 쓰기 기능(`fastboot-write`·`root-write` 등)과 프론트 `REAL_STEPS` 게이트 뒤에 있고 기본은 모두 꺼져 있다. 실기기 미검증 항목은 `../04-engine/device-test-checklist.md`.

규칙: 프론트는 `src/lib/api/` facade로만 호출한다. command 이름/인자/반환은 이 문서가 단일 진실 공급원이다.
이벤트 스트림은 Tauri `emit` → facade의 콜백/스토어로 전달된다.
**구현 원칙**: 부트·EFS 변경은 해당 Cargo 쓰기 기능 + 프론트 `REAL_STEPS`가 모두 켜졌을 때 실행된다. 백업·일반 파일/문자 복원은 별도 Cargo 쓰기 기능이 없으며 프론트 실전 플래그로 활성화한다. CLI에서 폰을 바꾸는 명령은 매번 `--allow-device-write`를 요구한다. 읽기 명령은 항상 동작한다.
**기기 통신 구현체**: `adb_client` 크레이트(ADB 프로토콜 순수 Rust) — 1순위 이미 실행 중인 adb 서버(5037, TCP 포트 확인으로만 판단) 재사용, 2순위 USB 직접 연결. adb 바이너리 설치/경로 탐색 불필요.
  - adb_client는 로컬 서버 연결 시 `adb start-server`를 실행하므로, 존재하지 않는 adb 경로를 넘겨 바이너리 실행을 차단한다(서버를 새로 띄우지 않음).

## device (M1)

```ts
invoke('engine_capabilities') → { fastbootWrite: boolean, rootWrite: boolean, efsWrite: boolean } // PC 전용; 현재 Cargo 기능
invoke('adb_status') → AdbStatus                   // ✅ 구현: 연결 수단 점검 { available, mode: "adb-server"
//   adb 서버가 응답 오류를 내면 { available: false, mode: "adb-server", detail: <오류> } — USB 직접 연결로 넘어가지 않는다|"usb-direct"|"none", detail }
invoke('device_list') → DeviceStatus[]            // ✅ 구현: adb_client 연결 + getprop 덤프 + which su + dumpsys isub + TelephonyDebugService (읽기 전용)
//   USB 직접 연결에서 USB 디버깅 허용 대기 중이면 오류 대신 state "unauthorized" 자리표시 1개(serial "usb-0", serialMasked "USB #1").
//   adb 서버가 응답 오류를 내면 USB로 넘어가지 않고 오류. 재사용하던 USB 연결이 끊겼으면(재부팅·재연결) 한 번 다시 연결
//   - Xperia 전용(사용자 지시 2026-10-03): ro.product.manufacturer가 Sony가 아닌 기기는 목록에서 제외, USB 직접 연결은 VID 0x0FCE만,
//     미승인 기기는 PC에 Sony USB 장치가 있을 때만 표시
//   - state: "device"(준비) / "unauthorized"(USB 디버깅 허용 대기) / "offline" … — 준비 안 된 기기도 상태만 담아 반환
//   - firmware: ro.build.id (display.id는 " release-keys" 접미어) / productName: ro.semc.product.name 우선, 없으면 모델 표
//   - fingerprint: ro.build.fingerprint (시리얼 없음) — 업데이트 확인(fw-verify)에서 같은 기기·지역·대상 버전 대조. 자리표시 항목은 빈 문자열
//   - bootloader: ro.boot.flash.locked + ro.boot.vbmeta.device_state 일치 시 확정, 불일치·잠김인데 su 존재(위장 가능성)면 "unknown"
//   - rooted: true | false | "unknown" — su 경로가 보이면 true. 안 보여도 부트로더가 잠김으로 확정될 때만 false,
//     언락·판별 불가 상태나 출력 끊김은 "unknown"(셸에 su를 숨긴 루팅과 구분 불가 → 리락 전 언루팅 포함)
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
// 받기 전에 여유 공간 확인(.sin 크기 + 16 MiB), 부족하면 "NO_SPACE|<사유>" 오류 → 프론트에서 다른 저장 위치 선택
// 추출 이미지 옆에 출처 기록 <이미지>.json(partition, fingerprint, sha256) 저장, 이미지 종류가 파티션과 맞지 않으면 거부.
// 네트워크 작업은 300초 상한. 초과 시 "펌웨어 받기 시간 초과 — 네트워크 연결을 확인한 뒤 다시 시도해 주세요" 오류(작업 스레드는 취소되지 않음)
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

## root (M4 — 설계 `.plans/04-engine/root.md`, 사용자 승인 2026-10-04)

**구현 상태**: 4 명령 전부 ✅ 구현(src-tauri/src/magisk/ — FakeADBDevice·ZIP 픽스처 단위 테스트, 이 구현의 실기기 테스트 미실시). wizard 루팅 단계 실전 연결(패치→부트로더→기록→복귀→설치→su 승인) 포함.

Magisk 자동 패치 (사용자 조작 없음) — 2026-10-03 XQ-DQ44 / Android 15 / Magisk v30.7로 검증된 절차:

```ts
invoke('magisk_prepare') → { version, apkPath, sha256 }
//   GitHub releases(topjohnwu/Magisk latest)에서 Magisk-v<ver>.apk 다운로드 → 앱 데이터 캐시(재사용)
//   기기 무관·준비 단계 — 게이트 밖(firmware_fetch와 같은 성격)
invoke('boot_image_check', { serial, path, fingerprint }) → string // 이미지 SHA-256 + 기기별 지문 대조 기록 저장
//   이미지 옆 출처 기록(<이미지>.json)이 필요 — 없거나 sha가 다르면 "펌웨어를 다시 받아 주세요"로 거부.
//   기기와 대조하는 지문은 기록의 값. 넘겨받은 fingerprint는 비어 있거나 기록과 같아야 한다. 이미지 종류·파티션도 검사
invoke('magisk_patch', { request: { serial, apkPath, imagePath, partition, imageSha256, fingerprint, apkSha256 } }) → { path, origSha256, patchedSha256, bytes, log: string[] }
//   위 검증 절차 2~5: 페이로드 추출(zip) → push/chmod → boot_patch.sh(종료 코드 판정) →
//   new-boot.img 검증(ANDROID! 매직 · 원본/2 ≤ 크기 ≤ 원본 · 해시 ≠ 원본) → pull → 작업 폴더 정리(고정 경로만)
//   기기 측 이미지명은 고정 boot.img — 로컬 파일명은 셸에 넣지 않는다(§12.5). 결과는 앱 데이터 magisk/patched-<sha256>.img
//   apkPath는 magisk_prepare가 돌려준 캐시 경로여야 한다(앱 데이터 magisk/ 안, GitHub 다이제스트 기록·apkSha256·서명 핀 일치) — 아니면 "루팅 준비를 다시 진행해 주세요".
//   imagePath는 출처 기록이 필요하고 기록의 partition이 요청과 같아야 한다. fingerprint는 기록과 같거나 비어 있어야 한다.
//   이벤트 'magisk:log': string — 스크립트 출력·진행
invoke('magisk_install', { serial, apkPath, apkSha256 }) → void // apkPath 캐시 조건(magisk_patch와 같음)·필수 ZIP 항목·CRC·해시·서명 핀 확인 후 adb install
invoke('root_reboot', { serial, target: 'os'|'bootloader' }) → void   // adb reboot — fastboot_reboot의 adb 짝
// 기록은 fastboot_flash 재사용. expectedSerial과 패치 결과 expectedSha256을 함께 전달한다.
// 검증(절차 7)은 기존 root_check + su-grant 수동 개입 재사용
// 패치·설치는 root-write + REAL_STEPS.root. root_reboot는 root-write 또는 fastboot-write 필요.
// 예외: target='os'는 efs-write만 켜진 빌드에서도 허용한다(bootloader는 불가).
// 프론트 facade는 REAL_STEPS.root/fastboot이면 os·bootloader 모두, REAL_STEPS.verify만 켜져 있으면 os만 허용(최종 확인의 재부팅).
// 시리얼은 필수이며 선택 기기가 없을 때 임의의 첫 기기로 대체하지 않는다.
```

상세 절차(2026-10-03 실측): APK 페이로드(lib/arm64-v8a/{libmagiskboot,libmagiskinit,libmagisk,libinit-ld,libbusybox}.so + assets/{boot_patch.sh,util_functions.sh,stub.apk}) →
`KEEPVERITY=true KEEPFORCEENCRYPT=true PATCHVBMETAFLAG=false RECOVERYMODE=false LEGACYSAR=false ./busybox sh -o standalone ./boot_patch.sh <img>` —
실측 로그 "Stock boot image detected → Patching ramdisk → Repack", 종료 코드 0, new-boot.img 8 MiB(원본과 동일 크기).

## host (PC 측, 읽기 전용)

```ts
invoke('disk_free', { path }) → number   // ✅ 구현(src-tauri/src/host.rs): 경로가 속한 드라이브 여유 바이트, 경로 없음 → 에러
//   15초 제한 — 넘으면 "여유 공간 조회 시간 초과 — 드라이브 연결 상태를 확인해 주세요"
// 폴더 선택: @tauri-apps/plugin-dialog open({ directory: true }) — capability dialog:allow-open
```

## env (M1 — §12.6)

```ts
invoke('env_check') → EnvCheckItem[]            // ✅ 구현(src-tauri/src/env.rs, 읽기 전용·관리자 권한 불필요)
//   현재 항목: diag-driver — 드라이버 저장소(레지스트리 HKLM\SYSTEM\DriverDatabase\DriverPackages)에 qcser.inf_* 패키지가 있으면 pass(패키지 설치 여부 — 포트 연결까지 보장하지 않음),
//   없으면 warn(설치 안내), 레지스트리를 읽지 못하면 info(단정하지 않음). 드라이버를 설치하지 않는다(사용자 결정 2026-10-04: 감지+안내).
//   예정 항목: WebView2/adb서버/번들해시/프리셋/디스크/QPST(정보)
// env_fix(자동 수리)는 두지 않는다 — 드라이버 설치 등은 안내만(사용자 결정 2026-10-04). 프론트 api.envCheck는 조회 실패 시 null(이전 결과 유지)
```

## plan / fastboot (M2 — 설계 `.plans/04-engine/fastboot.md`, 사용자 승인 2026-10-03)

**구현 상태**: FakeTransport 검증, 실기기 미검증. 쓰기·재부팅은 Cargo `fastboot-write` 기본 비활성 + `REAL_STEPS.fastboot`로 보호. 리락은 현재 OS/추출 출처/양 슬롯 복원 이력·동일 기기·bootloader 검사 뒤 조건부 실행한다. 전체 AVB 인증은 구현 범위가 아니다.

```ts
invoke('fastboot_getvar') → Record<string,string>   // ✅ 읽기 전용 — rusb FF/42/03 open, getvar:all 파싱(unlocked·current-slot·slot-successful:_a/_b·max-download-size …)
//   fastboot 모드 Sony 장치가 정확히 1대일 때만 open(다중 기기 거부 — §9-3)
invoke('fastboot_unlock', { code, confirm, expectedSerial }) → { unlocked: boolean }
//   "oem unlock 0x{code}" — 16자리 hex 검증, 로그·이벤트에 코드 마스킹(§12.5). 실행 후 getvar로 이중 확인
invoke('fastboot_lock', { confirm, partition, stockPath, expectedSerial }) → { unlocked: boolean }
//   추출 출처/파티션/해시와 양 슬롯 복원 이력 재검사. 동일 serial·bootloader·슬롯 확인 뒤 oem lock.
//   unlocked=no를 명시적으로 확인해야 성공한다. 프런트는 모드 전환 전에 현재 OS 지문을 대조한다.
invoke('relock_gate_check', { partition, stockPath, deviceKey? }) → { ok, reasons: string[], checked: [{partition, slot, ok, detail}] }
//   기기 키·추출 출처 필수. 이력 없음·손상·슬롯 누락·최신 미완료는 실패. 조건 충족 시 ok=true.
invoke('fastboot_flash', { partition, path, confirm, expectedSerial, expectedSha256 }) → void
//   partition은 boot·init_boot만(그 외 파티션은 직접 호출도 거부). 전송 버퍼 해시 대조, 부트 이미지 형식·크기 검사.
//   이미지 종류가 파티션과 맞아야 한다(init_boot = 헤더 v4·커널 없음, boot = 커널 있음). 빈 expectedSha256은 거부.
//   응답 대기는 응답 1건 전체 기준: 일반 10초, getvar:all 30초, 언락·리락·flash·본문 확인 300초, DATA 전송 전체 600초.
//   양쪽 슬롯 존재 확인 → download/flash → 기기별·슬롯별 이력. flash·언락 응답은 최대 300초 대기(기기 작업 중 10초로 끊지 않음).
invoke('fastboot_reboot', { target: 'os'|'bootloader', expectedSerial }) → void
//   언락·기록·재부팅은 fastboot serialno == expectedSerial 확인 후에만 실행한다.
// 이벤트 'fastboot:log': string — INFO/TEXT 프레임·진행(코드·IMEI·식별정보 마스킹)
//   파괴 명령은 confirm=true 필수. 쓰기/재부팅은 fastboot-write 없으면 USB open 전 오류.
//   unlocked는 명시적 yes/no만 반환; 조회 실패는 오류. reboot는 OKAY만 성공(타임아웃도 오류).
//   실전 unlock/flash는 is-userspace=no 필요; 미지원/fastbootd면 차단.
invoke('plan_generate', { profile, toggles, deviceStatus }) → PlanStep[]   // §3-2 매트릭스 + §3-3 의존성 — 프론트 mock/plan.ts가 단일 공급원(유지)
```

## backup (M3 — 설계 `.plans/04-engine/backup-engine.md`, 사용자 승인 2026-10-03)

**구현 상태**: `backup_run`·`backup_cancel`·`backup_manifest_check`·`smsie_prepare`·`smsie_collect`·`restore_run`·`smsie_restore_stage`·`smsie_restore_finish` ✅ 구현(src-tauri/src/backup/ — FakeADBDevice 단위 테스트로 검증, 실기기 미검증).
완결 게이트(§3-3): wizard가 언락/리락 시작 전 이번 실행 summary 또는 `backup_manifest_check`(파일·해시 대조) 통과를 강제 — 백업 미선택 계획은 기존 이중 확인 모달로.

```ts
// backup_scan_items는 설계 후보이며 현재 명령으로 등록되지 않았다.
invoke('backup_prepare', { serial, dest }) → string  // 고유 백업 폴더 절대 경로
invoke('backup_run', { serial, items: string[], dest, resumeDir?: string, runId: string }) → BackupSummary
//   runId: 프론트가 실행마다 만든 식별값(crypto.randomUUID). 취소는 이 값으로 대상을 지정한다.
invoke('backup_cancel', { runId?: string }) → void — runId 실행만 취소(시작 전에 오면 시작 즉시 멈춤), 없으면 실행 중인 백업
invoke('backup_delete', { dir }) → void — 완료 화면에서 사용자 확인 후. 절대 경로 · 이름 backup-* · 유효한 manifest.json · 실제 폴더(링크·정션 아님)일 때만 삭제.
//   manifest.json을 맨 마지막에 지운다(중간 실패 후 다시 시도해도 백업 폴더로 확인됨). 다른 기기 변경 작업(백업·복원·fastboot·Magisk) 중에는 거부(전역 실행권)
// 실행: 항목별 열거 → pull(sha256 동시 계산) → manifest 원자 저장 → quarantine 격리 → 설정·연락처 덤프 → sms-ie 산출물 수령
// 이벤트 'backup:progress': { itemId, phase: 'scan'|'copy'|'quarantine'|'settings'|'contacts'|'smsie',
//   file: string|null, filesDone, filesTotal, bytesDone, bytesTotal }
// BackupSummary = { deviceKey: string|null, complete, files, bytes, errors: string[], dir,
//   items: { id, status: 'pending'|'done'|'partial'|'skipped', files, bytes }[] }
//   complete = 전수 열거 완료 + 오류 0 (§6-2) — 파괴 단계 게이트의 입력
// backup:progress에 항목 저장 후 'done'|'partial'|'pending'을 전송. 파일 카운터만으로 완료 판정하지 않음.
invoke('backup_manifest_check', { dir }) → BackupSummary | null   // 기존 백업 폴더 완결 검사 (백업 스킵 시 게이트용)
invoke('restore_run', { serial, dir, items: string[] }) → { logs: string[], failures: string[], smsiePending: boolean }
invoke('smsie_prepare', { serial, download: boolean }) → string[]
invoke('smsie_collect', { serial, backupDir, confirmComplete?: boolean }) → { ready: boolean, summary: BackupSummary|null, cleanupWarning: string|null }
// 기본 false: ZIP/JSON 검사·PC 수집만. ready는 선택한 산출물 모두의 파일 검사 통과이며,
// 폰 앱 내보내기 성공을 확인한 confirmComplete=true 전에는 manifest가 완결되지 않는다.
// 확인 + 선택 항목 전체 검사 + manifest 원자 저장 뒤에만 기기 임시 사본 정리.
// 정리 실패는 cleanupWarning이며 이미 저장한 정상 PC 백업을 실패로 바꾸지 않는다.
invoke('smsie_restore_stage', { serial, dir, items: string[] }) → string  // 선택한 문자/통화 파일만 검증·전송
invoke('smsie_restore_finish', { serial }) → string[]  // 기기별 영속 기록으로 원복, 실제 역할 확인 전 실패는 reject/기록 유지
invoke('contacts_restore_check', { serial, dir }) → { backedUp: number, onDevice: number }
invoke('contacts_restore_finish', { serial, dir }) → void // 사용자 가져오기 확인 후: 같은 기기·백업 수 충족 재검사 → /sdcard/contacts-restore.vcf 정리
// 순서(§6-5): 선택 기록 무결성 검사 → APK 재설치 → tar 스트리밍 복원(원본 mtime 보존, 선택 quarantine만) → 설정 화이트리스트 6키(adb_enabled 제외) →
//   deviceidle whitelist → 연락처·sms-ie 복원(수동 개입 포함)
// 이벤트 'restore:progress': { itemId, phase: 'apk'|'files'|'quarantine'|'settings'|'contacts'|'smsie',
//   file?, filesDone, filesTotal, bytesDone, bytesTotal }
```

- 폐기: `backup_estimate`(용량은 storage_sizes 실측이 담당), 구안 `backup_scan_items → BackupGroup[]`(항목 정의는 프론트 mock이 단일 공급원)
- 문자·통화 기록은 SMS Import/Export(tmo1/sms-ie) 세미수동 — 설치·pm grant·cmd role·파일 전송 자동, 앱 내 내보내기/가져오기는 수동 개입 단계(ManualPrompt)
- 연락처는 contacts/raw_contact_entities 조회로 vCard 생성 — 자동·완결 게이트 포함
- 항목 id는 mock/apps.ts의 id 그대로(settings-all, apk, app-data, dcim, download, pictures, movies, music, documents, recordings, fs-rest, calllog, sms, contacts)

2026-10-04 리뷰: 백업/복원/SMS 변이 명령은 동시에 하나만 실행하며 중복은 reject한다. 알 수 없는/중복/빈 선택도 reject한다. 검사는 기록된 전체 파일과 선택 격리 파일의 SHA-256·크기 대조이며, 신규 설정 덤프도 해시를 기록한다. 예전 artifact-only 덤프는 존재 여부만 확인한다. 재개는 원본 모델·마스킹 시리얼을 대조하고 Done 파일도 재검증한다. restore failures가 비어 있지 않으면 프런트 단계가 실패한다. 상세 한계와 실기기 미검증 목록은 [전체 리뷰](../04-engine/code-review.md)를 따른다.

후속 점검: SMS 역할 변경 전에 `<앱 데이터>/sms-role/<SHA-256(ro.serialno)>.json`을 원자 저장하고 재시작/재연결 시 재조회한다. 원본 시리얼은 기록하지 않는다. 기기 식별 실패·손상 기록·원복 기록 없음·사용자가 다른 기본 앱 선택·명령 뒤 역할 불일치는 reject한다. 원래 sms-ie 사용자는 그 기본 앱을 유지한다. 격리 tar는 미선택 본문을 seek로 건너뛰고 모든 헤더의 경로/유형을 검사한다. 설정 덤프 읽기 실패는 reject이며 파일별 한 번 해석 후 적용한다. 실기기 테스트 없음.

## EFS (M5)

```ts
invoke('efs_tool_check') → EfsToolCheck                    // 내장 버전·EFS/root/fastboot 빌드 기능, 기기 접근 없음
invoke('efs_config_get') → EfsConfiguration | null         // {port,presetRoot,snapshotRoot}, 앱 로컬 설정
invoke('efs_config_set', { configuration }) → void         // PC 설정만 저장, COM을 열지 않음
invoke('efs_resolve_preset', { folder, configuration?: EfsConfiguration }) → string
// manifest의 ./util/SonyEFS/... → 설정 루트의 절대 경로. 폴더 이름과 그 이름의 고정 해시를 함께 확인.
// configuration 생략 시 앱 설정 사용, 지정 시 검증된 실행별 설정 사용. PC 작업만 수행.
invoke('efs_validate_presets', { presetDirs }) → void       // 최대 2개 전체 선택의 공유 EFS/NV 내용·mode/type 충돌 검사, 기기 접근 없음
invoke('efs_diag_open', { serial }) → void                 // 선택 기기 모델 조회 → Mark IV persist.usb.eng=1 설정·리드백 → 공통 DIAG 설정. 셸 종료 상태 검사, SIM 조회 없음. 기본 게이트 차단
invoke('efs_preflight', { port }) → EfsPreflight            // hello/query/설정 응답 확인; USB 토폴로지·속도·전원 실측을 주장하지 않음
invoke('efs_snapshot', { port, presetDir, dest }) → EfsSnapshotResult // 변경 대상 전수 before-image, mode/type/times·전체 128 B NV
invoke('efs_upload', { port, presetDir }) → EfsUploadResult  // 슬롯당 1회; written/skipped/warnings, 오류는 즉시 Err
invoke('efs_verify', { port, presetDir }) → EfsVerifyReport // 모든 비어 있지 않은 EFS/NV 대상 리드백
invoke('efs_rollback', { port, snapshot }) → EfsUploadResult // snapshot = 스냅샷 폴더, 전수 사전 검사 후 복원·리드백
invoke('efs_cancel') → void                               // 소유 작업의 토큰만 취소, 기본 차단 빌드에서도 사용 가능
invoke('volte_props_set', { serial }) → string[]            // 명시한 ADB 기기에 persist.dbg 4종 설정 후 재부팅; EFS 게이트·작업 소유권·취소 공유
// EfsToolCheck = { version, path:"built-in", native:true, deviceExecution, rootExecution, fastbootExecution }
// 각각 efs-write / root-write / fastboot-write의 컴파일 여부. 실전 EFS 계획 첫 단계에서 필요한 기능을 확인한다.
// EfsUploadResult = { errors:[], filesSeen, planned, skipped, warnings }
// EfsVerifyReport = { ok, files, matched, planned, skipped, missing, mismatches, warnings }
// EfsSnapshotResult = { path, filesSeen, warnings }
// EfsWarning = { code, target, message }
// 네이티브 오류 = { code, operation, message, status?, cleanup? }
// facade EfsResult<T> = {ok:true,value:T} | {ok:false,error:string,details?:EfsError}
// 이벤트 efs:progress = {operation,file,n,total}, efs:log = {cmd,line}
```

사용자 승인 2026-10-04: 네이티브 구현 허용, **실기기 작업 금지**. `efs-write`와 `REAL_STEPS.efs`는 기본 false.
`src/lib/api/efs.ts`가 명시적 COM 설정과 상대 프리셋 경로를 해석하며 모든 컴포넌트 호출은 기존 `api` facade 경유.
wizard는 모든 선택 프리셋을 DIAG 전환 전에 검사하고, 슬롯별 스냅샷 → 두 번 업로드 → 전체 선택 업로드 후 슬롯별 검증 순서를 유지한다.
공유 EFS/NV 값이 다르면 `presetConflict`로 기기 접근 전에 중단한다. 숫자 NV ID는 글로벌이며 SIM별 ID로 임의 변환하지 않는다.
직접 facade 호출하는 통합 코드도 `efsValidatePresets(선택 전체)`를 **DIAG/업로드 전에** 반드시 호출한다.
서로 같은 글로벌 값은 허용한다. 혼합 통신사 충돌을 자동 해결하거나 NV 검증에서 제외하지 않는다.
이전 wrapper의 `efsSnapshot(dest)`는 `efsSnapshot(dest,preset.folder)`로 변경, `efsUpload(folder)`/`efsVerify(folder)`는 그대로 사용하며 COM은 설정에서 공급한다.
원본 `uploadDirectory/downloadDirectory -v`는 **processNvItems**이며 verbose가 아니다. `-n`은 파일 metadata 접미사 생략이다.
네이티브 검증은 다운로드 폴더나 metadata 파일명을 비교하지 않고 확정된 실제 EFS 경로·ROOT NV ID를 직접 읽는다. 65,534개 NV 스캔 없음.
0바이트 NV는 `emptyNvSkipped` 경고로 명시적 미변경 처리하고 written/verified 분모·분자에서 제외한다 (KT 슬롯당 6789·6849 두 항목).
짧은 NV 요청은 원본처럼 지정된 바이트만 전송하며, 알려진 논리 길이는 C# ItemsFactory 측정값과 대조한다.
알 수 없는 짧은 항목은 `nvPrefixVerification` 경고와 함께 명시된 prefix만 비교한다. 나머지 128 B tail을 0으로 채우거나 변경됐다고 주장하지 않는다.
스냅샷·복원은 정규화하지 않은 128 B 전체 값을 사용한다. 상세 근거·한계: `04-engine/efs-native.md`.

EFS 실행 규칙 (카페 조사 반영, 2026-10-03 — tasks/research-cafe-omd-volte.md):
- 프리셋: src/lib/data/efsPresets.ts manifest(통신사·슬롯·폴더·파일 수·SHA-256, 버전 20250901 balance)와 일치하는 폴더만 사용.
  실행 직전 폴더 해시를 다시 계산해 manifest와 다르면 진행하지 않음. 이름("for V" 등)을 적용 가능 기종의 근거로 삼지 않음
- 원본 beta11 계승: 슬롯별로 efs_upload 2회(1차·2차) → efs_verify 전수 리드백·해시 비교.
  두 번 썼다는 것만으로 성공 판정하지 않음 — 누락·해시 불일치는 도구가 정상 종료해도 실패
- 실패하면 해당 단계를 failed로 유지(wizard.failStep)하고 다음 단계·리락으로 넘어가지 않음. 허용: 이 단계 다시 시도 / 중단.
  사유에 실패 항목·슬롯·시도 횟수·구조화된 native 오류·cleanup 결과를 남김. 서브프로세스·종료 코드 판정은 사용하지 않음
- 입력 오류(잘못된 통신사·슬롯)는 명시적 오류 타입으로 반환 (원본 efs.py의 문자열 raise 계승하지 않음)
- 진단: EFS(DIAG)와 펌웨어 기록(newflasher)은 USB 경로가 다름 — "무조건 USB 2.0" 대신 실제 인터페이스·드라이버·케이블·실패 명령을 보여줌.
  ADB/fastboot 대상은 Sony 기기만(에뮬레이터·Google Play Games ADB 기기 혼입 방지)

통신 확인 구분 (IMS 판정 개선은 실기기 실측 후):
- 파일 주입 확인(efs_verify) / IMS 음성 사용 가능(mMmTelCapabilities Voice) / IMS 등록 상태·등록 기술(LTE vs IWLAN) / 실제 발신·수신(사용자 확인)
- 자동으로 판독할 수 없는 항목은 unknown. 문자·MMS·5G 데이터·해외 로밍은 별도 확인 항목 — 국내 통화 성공을 로밍 성공으로 표시하지 않음
- "VoLTE 활성화 설정"(persist.dbg.*_avail_ovr)은 설정을 켠다는 뜻이지 통신사 서비스 검증이 아님

## flasher / session (M6)

```ts
invoke('fw_prepare', { fwDir, opts }) → StagedDir           // §8 스테이징 (원본 불변)
// 플래시 허용 목록(파일을 지우지 않고 목록으로 검사): 패치 유지 업데이트 = modem*·dsp*·.ta(boot 하위 포함)·userdata 제외 /
//   재패치 업데이트 = modem·dsp 포함, .ta·userdata 제외. 모델·지역 펌웨어 일치 확인, 슬롯 A 고정 명령을 기기 상태 확인 없이 일괄 실행하지 않음
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
// facade 반환: ApiResult<string|null>. 실제 파일 없음은 ok(null), 읽기 오류는 error.
invoke('journal_archive', { key, tag: 'done'|'discarded' }) → void  // <key>.<tag>.json으로 보관(마지막 1개, 디버깅용)
// key = SHA-256(모델|시리얼) 앞 16바이트 hex — 파일 이름에 시리얼을 그대로 쓰지 않음, Rust에서 16~64자 hex만 허용
// data = RunJournal (types.ts): 선택 옵션·계획·단계별 상태/로그(단계당 최근 300줄)·멈춘 사유. optional imsUnverified/imsVerified/callVerified boolean으로 등록·사용자 통화 확인 구분. 언락 코드·IMEI 없음
// 저장/보관은 프런트 직렬 큐, Rust 디스크 I/O는 blocking. JSON 저장 검사, 저장/로드 모두 8MiB 상한.
// 로드한 디스크 JSON은 domain/journal.ts에서 옵션·SIM·계획/실행 목록·인덱스·상태 등을 검사한 후 재개.
```

## guard (작업 중 PC 보호 — 사용자 승인 2026-10-03)

```ts
invoke('run_guard', { active: boolean, reason?: string }) → void
// 켬: 절전 방지(PowerCreateRequest/PowerSetRequest SystemRequired) + Windows 종료 방지(ShutdownBlockReasonCreate)
// 메인 창 서브클래스가 보호 중 WM_QUERYENDSESSION에 FALSE 응답 → Windows가 "종료를 막고 있습니다: <사유>" 표시
// 이벤트 'run-guard': "query"(종료 보류 — 기록 저장) | "end"(사용자가 그래도 종료 — 사유 기록)
// 프론트: begin() 시 켬, complete/중단·오류(markStop)/창 닫기/처음으로 시 끔. 폰 확인 대기 중에는 유지
// 단, 기기 쓰기 엔진이 아직 진행 중이면 끄지 않고 그 엔진이 끝날 때 끈다
// 메인 창 스레드의 실제 적용/상태 확인 결과까지 기다리고 실패 시 보호 상태를 해제한다.
// 종료 가로채기(창 서브클래스)가 설치되지 않은 환경에서 active=true는 오류(보호되지 않는데 켜졌다고 보고하지 않음).
```

## 수동 확인·PC 준비

```ts
invoke('root_check', { serial? }) → boolean            // su -c id 결과에 uid=0 — Magisk 허용 창이 뜰 수 있음, 기기 변경 없음
invoke('firmware_dir_check', { dir, partition }) → { file, path, fingerprint, imageBytes }
// PC 폴더(한 단계 하위 포함)의 SIN 후보는 정확히 하나여야 한다. 같은 폴더 update.xml의 지문 필수.
// SIN을 raw IMG로 PC 캐시에 원자 추출한다. path가 패치·기록 입력이며 file은 표시용 SIN 이름이다.
// 추출 이미지 옆에 출처 기록 <이미지>.json(partition, fingerprint, sha256)을 함께 저장한다. 이미지 종류가 파티션과 맞지 않으면 거부.
```

## 점검 반영 (Codex gpt-6.1-sol 리뷰, 2026-10-03)

- device_list: serial = 서버 모드는 adb 전송 식별자(TCP 연결 등에서 ro.serialno와 다를 수 있음), USB 직접 연결은 ro.serialno — 이후 명령의 serial 인자와 같은 값.
  준비된 기기를 하나도 읽지 못하면 빈 목록이 아니라 오류(프론트는 연속 2회 실패 시에만 카드 제거)
- bootloader: 두 프롭이 모두 유효하고 일치할 때만 확정(한쪽만 있으면 unknown) — 위 계약과 구현 일치
- storage_sizes: 없는 폴더는 0, 측정하지 못한 폴더는 키 없음(프론트 "측정 불가", 그 외 파일 합계도 계산 불가)
- 오류 문자열의 전체 시리얼은 마스킹(scrub_serial) — 로그·진행 기록 유출 방지
- firmware ZIP 파서: 범위 밖 읽기는 panic 대신 오류, 항목 크기 상한 256 MiB
- run_guard: 켜기에 실패하면 전부 끈 상태로 되돌리고 오류 반환(프론트는 경고 로그 후 다음 시작 때 재시도)
- SimInfo.volte: "on"(셀룰러 IMS 음성 = VoLTE) | "wifi"(Wi-Fi 통화로만 등록 — VoLTE 아님) | "off" | "unknown".
  TelephonyDebugService에서 슬롯별 mMmTelCapabilities(Voice), mImsMmTelRegistrationState(0/1/2), 등록 로그의 마지막
  "handleImsRegistered … imsRadioTech=WWAN|WLAN"을 읽음 (2026-10-03 XQ-DQ44 덤프로 필드 확인). 최종·리락 전 확인은 "on"만 통과, "wifi"면 Wi-Fi를 끄고 확인 안내
- EFS 프리셋 버전 조사(tasks/research-efs-preset-versions.md, 2026-10-03): 20250901 balance = 확인된 최신 공통 세트(도구 beta9~beta11 동일, 카페 재첨부 ZIP과 SHA-256 동일).
  manifest에 원본 출처·ZIP 해시, 이전 성공 후보(KT/LGU beta7, 자동 롤백 금지) 기록. performance 세트는 배포 구성 문제로 사용하지 않음.
  갱신 판정은 데이터 해시 기준, 새 세트는 격리 → 차이·XML·슬롯 경로 검토 → 실물 확인 → 승격
- 네이티브 EFS(2026-10-04): EfsTools/.NET 설치 검사는 폐기. 원본 net5.0·카페 net8.0 런타임 조사 기록은 레퍼런스이며 앱 실행 의존성이 아니다. .NET 8은 오프라인 C# 골든 픽스처 재생성에만 사용한다.
- 게시자 기준 추적(tasks/research-efs-preset-versions.md "게시자 기준 추적"): 20250901 이후 EFS 후속 배포 없음. 1 VII 등의 속성 모듈·IMS 앱·APN·eSIM 모뎀 패키지는 EFS와 별개 자료로 분리 관리

## 보안 설정 (2026-10-05)

- capability: 외부 열기는 `opener:allow-open-url`의 `https://*`만(이전 `opener:default`의 http·mailto·tel·폴더 열기 제외), `dialog:allow-open`, `core:window:allow-destroy`.
- CSP(tauri.conf.json): 운영 `default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src ipc: http://ipc.localhost …`, 개발용 `devCsp`는 HMR·인라인 스크립트 허용.
  release 실행 파일에서 첫 화면·OMD 화면이 CSP 위반·콘솔 오류 없이 동작함을 WebView2 원격 디버깅으로 확인(2026-10-05).

2026-10-06 복원: restore_run과 smsie_restore_stage는 manifest.deviceKey를 현재 ro.serialno SHA-256과 대조한 뒤 기기에 쓴다. 원본 키가 없는 구형 백업은 자동 복원을 거부한다. 파일 복원은 /sdcard의 고유 임시 폴더에 tar를 모두 받고 성공 후 파일별 rename, 성공·실패 모두 정리를 시도한다. 교체 중 실패해도 이미 교체된 파일은 완전한 파일이며 나머지는 재실행 대상이다. 임시 폴더 정리 실패는 오류로 표시한다. 연락처 0개는 전송 없이 성공하고 실제 가져오기 후 contacts_restore_finish가 고정 임시 파일을 정리한다.
