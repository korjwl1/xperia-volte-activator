# 백업·복구 엔진 (M3) — 구현 설계

2026-10-06 케이블 분리 신고 후 점검: 재개 결과 영수증에는 권한 거부 뒤 SYNC 종료 순서 오류와 fs-rest pending이 남았다. USB 분리가 이 오류의 원인이라고 로그만으로 확정하지 않는다. 완료 기록은 약194GB이며 DCIM 761개·139.7GB와 이전 항목을 보존한다. 명시적인 RECV FAIL은 AOSP가 서비스 종료까지 수행하므로, 추가 QUIT 없이 원격 종료를 소비하도록 수정했다. vendor USB 35개·루트 엔진261개·CLI3개 오프라인 테스트 통과. 수정한 핸드셰이크 실행 파일의 device_list는 같은 Sony 폰을 정상 조회했으나 늦은 CLSE 재현 시험을 의미하지 않는다. 감독 스크립트는 .NET Process의 UTF-8 stdout/stderr와 PID에 해당하는 원자 저장 record.json을 사용한다. 콘솔 OEM 문자 변환으로 JSON 따옴표가 깨지던 문제를 피하고 팝업에는 짧은 원인·실제 조치·상세 로그 위치만 표시한다. 불완전한 backup_run 결과도 PC 백업 파일 검사를 진행하며 완결 상태로 오인하지 않는다. 전체 14개를 유지하고 fs-rest를 앱 데이터 재시도보다 먼저 수행하는 순서로 같은 폴더 재개를 준비했다.

2026-10-05 최종 리뷰: 2026-10-05 적대적 리뷰 보강: pull은 고유 임시 파일에서 크기·해시·내용 검사와 sync를 완료한 뒤 교체한다. 이어받기는 검증된 이전 파일 및 해시 기록을 보존한다. 실패한 재전송도 정상 사본을 잃지 않으며 선택에서 빠진 항목은 Skipped로 처리하고 재선택 시 다시 열거한다. sms-ie는 messages.ndjson ZIP 전체 CRC/행 JSON과 calls JSON 배열을 검사하고 선택 항목 전부가 준비돼야 한다. ZIP이 정상이어도 앱 작업 성공까지 증명하지는 못하므로 폰 앱의 내보내기 성공을 명시적으로 확인하기 전에는 manifest 완결 및 기기 사본 삭제를 하지 않는다. 복원은 미설치 sms-ie의 검증 APK 다운로드/설치와 권한 준비를 수행한 뒤 전송·SMS 역할 변경으로 진행한다. 이번 변경은 실기기 미검증이다.


status: implemented — 백업 실기기 검증 완료(2026-10-03, 읽기 전용), 복구는 모의 실행 검증·실기기 검증 대기

2026-10-04 코드 리뷰 보강은 [code-review.md](code-review.md) 참조. 전수 해시 검증·매니페스트 선택 복원·기기 대조 후 재개·취소/중복 실행·셸 종료 코드/SMS 역할 실패 처리를 수정했다. 아래 2026-10-03 실측 기록은 당시 버전에 대한 기록이며 이번 변경은 실기기에서 테스트하지 않았다. 신규 설정 덤프는 FileEntry 해시를 기록하고, 과거 artifact-only 덤프는 존재 여부만 확인한다. 문자 복원 준비 계약은 items를 받아 선택 기록만 검증·전송한다.

후속 점검: SMS 원복은 `sms_role.rs`의 기기별 영속 기록을 역할 변경 전에 저장하고 실제 역할 확인 뒤 삭제한다. 재연결·원래 sms-ie 사용자·기본 앱 변경을 구분하며, 기록이 없거나 손상됐으면 추측하지 않는다. 격리 파일 검증/복원은 미선택 본문을 seek로 건너뛰고 모든 tar 헤더의 경로/유형을 검사한다. 설정 복원은 각 덤프를 한 번 읽고 모든 입력 읽기에 성공한 뒤 키를 적용한다. 가짜 I/O 검증만 수행했고 실제 Android 역할/강제 종료/대용량 성능은 미실측이다.

APK 세션 생성/확정도 셸 종료 코드를 검사하고 정확한 성공 응답만 인정한다. 빈/읽을 수 없는 APK는 전송하지 않고 세션을 정리하며 abandon 실패는 경고 로그에 남긴다. 실제 Package Manager 오류 출력·세션 정리는 테스트하지 않았다.

- 상위 정책: `../tasks/plan.md`(v4) §5·§6·§9·§12.5, `../tasks/recovery.md` 전체
- 승인 배경 (사용자 승인 2026-10-03): **실전 Rust 코드 작성 허용. 단 실기기 테스트는 계속 금지** —
  AGENTS.md "백업 실행 백엔드 작성 금지"에 예외로 추가한다. 검증은 전부 단위 테스트(가짜 기기)로 한다.
- 원칙: 이 문서가 백업·복구 엔진의 단일 설계도. 구현과 같은 커밋에서 갱신한다.

## 범위

**구현 (In)**
1. Rust `src-tauri/src/backup/` 모듈 — 백업 엔진: 기기 파일 열거, pull+해시 동시 계산, manifest 원자 저장, Windows 비호환 격리(quarantine), 진행 이벤트
2. Rust 복구 엔진 — tar 스트리밍 복원(mtime 보존), 설정 화이트리스트 복원, APK 재설치, 통화·문자·연락처 복원
3. Tauri command `backup_scan_items` / `backup_run` / `backup_manifest_check` / `restore_run` + 진행 이벤트
4. facade 확장(`src/lib/api/`) + wizard 실행 엔진 연결 — **실전 실행 플래그는 기본 꺼짐**(시뮬레이션 유지)
5. 완결 게이트: 파괴 단계(언락/리락) 시작 전 백업 완결 검사 (plan §3-3)
6. 단위 테스트 — `FakeADBDevice`(ADBDeviceExt 가짜 구현)로 실기기 없이 전 과정 검증
7. `.plans` 문서 갱신(02-contracts 정밀화, 00-architecture, README 구조) + AGENTS.md 예외 기록

**제외 (Out)**
- 실기기 테스트 전부 (사용자 지시 — 실측 필요한 항목은 "검증 대기"로 문서화만)
- UI 재디자인 (기존 백업 탭·실행 화면 그대로, 연결부만)
- 언락/리락/EFS/펌웨어 기록 등 다른 단계의 실전화
- 구글 백업 연동, QPST, 백업 폴더 ACL(§12.5 — 아래 리스크 참조, 선택 구현)

## 아키텍처

### 모듈 구조 (신규)

```
src-tauri/src/backup/
├─ mod.rs          — 진입점·tauri command·이벤트 emit
├─ model.rs        — Manifest·ManifestEntry·BackupItemSpec·Summary·ProgressEvent (serde)
├─ winname.rs      — Windows 파일명 규칙 판정 (§6-3)
├─ walker.rs       — 기기 파일 열거 (list() 재귀) + 항목별 제외 규칙
├─ puller.rs       — 항목별 백업: stat → pull(HashingWriter) → mtime 적용 → manifest 기록
├─ quarantine.rs   — 비호환 파일 tar 격리 + 기기 측 스트리밍 복원 (§6-3)
├─ settings.rs     — 설정 덤프 수집 / 화이트리스트 복원 (recovery.md 1-2)
├─ contacts.rs     — 연락처 vCard 직접 내보내기(adb 셸) / 복원 안내
├─ smsie.rs        — SMS Import/Export 연동 — 설치·권한·역할·파일 전송 자동화
├─ restore.rs      — 복구 순서 제어 (§6-5)
└─ fake_device.rs  — (테스트 전용, cfg(test)) ADBDeviceExt 가짜 구현 — 메모리 가상 파일시스템
```

기기 접근은 기존 `adb::with_first_device`(adb_client, 서버 모드 우선→USB 직접)를 그대로 재사용한다.

### 백업 폴더 구조 (§6-2 계승)

```
backup-<ts>-<model>/
├─ manifest.json            # 항목별: 원본 경로·크기·mtime·sha256·오류·완결 상태
├─ sdcard/DCIM|Download|…   # 파일 항목(기명 폴더)
├─ sdcard/__rest__/         # fs-rest (그 외 전체)
├─ android-data/<pkg>/      # app-data
├─ apks/<pkg>/              # apk (base+split 전부)
├─ settings/                # settings_{system,global,secure}.txt · deviceidle_whitelist.txt · packages.txt
├─ contacts/                # contacts.vcf (vCard 1연락처 1카드, adb 셸 직접 조회)
├─ smsie/                   # messages.zip · calls.json — SMS Import/Export 앱 산출물 (해시 보존)
└─ quarantine/segNNN.tar    # Windows 비호환 파일 (절대 Windows에 풀지 않음)
```

### 항목 id → 백업 동작 매핑

| id | 소스 | 방식 |
|---|---|---|
| settings-all | settings list 3종 + dumpsys deviceidle whitelist + pm list packages -3 -f | 셸 덤프 → 텍스트 |
| apk | pm path가 가리키는 설치 폴더의 *.apk 전부 | pull |
| app-data | app_flags 결과 hasExternalData인 /sdcard/Android/data/<pkg> | pull |
| dcim…recordings (7종) | /sdcard/<폴더> | pull |
| fs-rest | /sdcard에서 기명 7폴더 + Android/data 제외한 나머지 전체(Android/media·obb 포함) | pull |
| contacts | `content query content://com.android.contacts/contacts` 목록 → 연락처별 `…/contacts/<id>/as_vcard` | vCard 직접 (자동·완결 게이트 포함) |
| calllog / sms | SMS Import/Export(tmo1/sms-ie) 앱이 (ND)JSON 산출 | 세미수동 — 아래 조사 참조 |

- fs-rest 제외 집합은 **프론트 용량 계산(storage_sizes 기반)과 동일 기준**을 쓴다 — 열거·용량·백업이 같은 경계를 보도록 `walker.rs`에 상수로 고정.
- 항목 미선택도 제외 집합에는 포함(기명 항목은 체크 여부와 무관하게 fs-rest에서 빠짐 — 용량 표기 정합성).

### 핵심 설계 결정

전송 보강 오프라인 검증: 엔진261개 통과(실기기 관련8개 ignored), USB 포함 transport28개 통과, 개발 CLI 통합3개 통과. dev-cli만 켠 release 빌드 및 PC-only 명령 카탈로그 확인 완료. 사용자가 요청한 현재 앱 데이터 완료 후 전환·DCIM 샘플 비교·동일 전체 선택으로 재개·전체 PC 해시 검증 절차를 세션 감독 프로세스에 예약했다. 실기기 샘플 비교는 아직 실행 전이므로 속도 개선율과 전체 완결은 미확정이다.

2026-10-06 전송 성능 보강: 순수 Rust 직접 USB/TCP 연결은 폴더 열거와 파일 복사 각각에서 SYNC 세션을 재사용한다. 파일마다 QUIT/CLSE/20ms 종료 드레인을 반복하지 않는다. 다른 서비스(빈 폴더 확인·64비트 크기 재조회 등)를 열 때는 캐시 세션을 먼저 닫아 비다중화 transport의 응답을 섞지 않는다. RECV 헤더·경로는 하나의 요청으로 전송한다. PC 저장은 256 KiB 버퍼를 쓰되 flush·sync_all·임시파일 rename·해시·mtime·격리·오류 기록을 유지한다. 종료 실패로 프로토콜 상태가 불명확하면 후속 요청을 차단하고 백업을 미완료로 남긴다. 서버 경유 transport는 기존 전송 구현을 유지한다.

개발 CLI 전용 `backup_transfer_probe`: 같은 제한된 파일 집합을 per-file/batch/batch/per-file 순서로 전송하며 실제 백업 저장 함수를 사용한다. 각 전송 후 PC 파일을 재독해 크기·해시를 대조하고 네 회차의 내용이 같아야 성공한다. 결과는 전송 시간·파일 수·바이트·세션 재사용 여부이며 USB 링크 대역폭 측정으로 해석하지 않는다. 별도 진단 폴더에 저장하며 기존 백업 manifest를 변경하지 않는다. 오프라인 검증 완료; DCIM 실기기 비교 및 전체 무결성 검증은 아직 대기 중.

1. **해시 — 전송 중 계산** (§6-1): `pull(source, &mut dyn Write)`의 출력을 sha2 해싱 래퍼로 감싼다. 별도 재독기 없음, 비용≈0.
2. **mtime 보존**: `stat()`으로 원본 mtime 확보 → manifest 기록 + `filetime` 크레이트로 로컬 파일 적용 (`adb pull -a` 상당). NTFS ctime 한계는 manifest에 명시(§6-1 표현 유지 — "완전 충실 주장 안 함").
3. **복원은 tar 스트리밍 통일**: `adb_client::push`는 mtime을 전송하지 않고 모드 0777 고정 → 파일별 push+`touch` 보정은 파일 수만큼 셸 왕복이 필요해 느리고 검증 어려움. 대신 폴더(항목) 단위로 **PC에서 tar 생성 → `shell(reader, writer)`로 `tar -xf - -C <dst>`에 스트리밍** — toybox tar가 mtime·원본 이름 그대로 복원(§6-3의 quarantine 복원 메커니즘과 동일, 하나의 구현을 양쪽에 재사용). 청크 상한으로 분할 스트리밍.
4. **열거는 SYNC `list()` 재귀** — 셸 find+stat 파식보다 타입 안전·테스트 용이. 권한 오류 등은 manifest에 error로 남기고 계속 진행(부분 완결 불허 — 완결은 "전수 열거 완료 + 오류 0").
5. **완결 게이트** (§3-3): `backup_run` 반환 Summary{complete, errors, files, bytes}. 실행 엔진은 언락/리락 단계 시작 전 ①이번 실행 백업 완결 또는 ②백업 스킵 시 `backup_manifest_check`(기존 백업 폴더 검사) 통과를 요구 — 불충분하면 `failStep`(failStep 경로는 기존 EFS 규칙과 동일).
6. **중단 재개** (§9-2): manifest를 항목 완료 시점마다 임시파일+rename으로 원자 저장(journal.rs 패턴). 재개 프로브 = 항목별 manifest 오류·누락 검사 → 누락 항목만 재백업. RunStep.sub 체크포인트(항목 단위)와 같은 단위.
7. **민감정보**: manifest는 경로·크기·mtime·해시만. 오류 문자열은 `scrub_serial` 적용(기존 규칙).
8. **진행 이벤트**: 파일 단위 emit + 프론트 부하 방지용 쓰로틀(기본 50ms). 페이로드는 아래 계약 참조.

### 계약 정정 (02-contracts backup 절 교체)

```ts
invoke('backup_scan_items', { serial, items }) → { items: { id, files, bytes, ok }[] }   // 사전 점검·재개 판정
invoke('backup_run', { serial, items, dest }) → BackupSummary
// 이벤트 'backup:progress': { itemId, phase: 'scan'|'copy'|'quarantine'|'settings'|'providers',
//   file?, filesDone, filesTotal, bytesDone, bytesTotal }
invoke('backup_manifest_check', { dir }) → BackupSummary | null   // 완결 게이트용 기존 백업 검사
invoke('restore_run', { serial, dir, items }) → RestoreSummary
// 이벤트 'restore:progress': { itemId, phase: 'apk'|'files'|'quarantine'|'settings'|'providers',
//   file?, filesDone, filesTotal, bytesDone, bytesTotal }
```

- 기존 `backup_estimate` 계약은 폐기 — 용량은 이미 `storage_sizes`가 실측 제공.

### 복구 순서 (§6-5 계승)

APK 재설치(`install()`) → 파일 tar 스트리밍(fs-rest → 기명 폴더 순) + quarantine 스트리밍 → 설정 화이트리스트(`settings put` — recovery.md 1-2 7키, `adb_enabled` 제외·안내만) → deviceidle whitelist 재적용 → providers 복원(insert, 실패 무시·경고) → 수동 체크리스트 안내(기존 FinishView).

### wizard 연결

- `data/runMode.ts`: `SIMULATED_RUN` 유지 + `REAL_STEPS: { backup: boolean, restore: boolean }`(기본 false) 세분화 — 백업·복구만 별도 전환. 실기기 검증 전엔 데스크톱에서도 시뮬레이션이 기본.
- 실행 엔진: step id가 backup/restore이고 실전 플래그 켜짐 → 시뮬레이션 tick 대신 실제 command 호출 + `listen('backup:progress')` 구독으로 progress·logs·sub 반영 (runGen 세대 가드 적용 — 기존 패턴).
- 백업 직전 안내(backup-notice) 모달은 그대로 유지.

## 테스트 전략 (실기기 없이)

1. **FakeADBDevice**: `ADBDeviceExt` 가짜 구현 — 메모리 가상 FS(경로→내용·stat). list/stat/pull/shell/install 기록·재생. 모든 엔진 함수는 `&mut dyn ADBDeviceExt`를 받도록 설계해 주입 가능하게.
2. walker: 가상 FS로 열거·제외 규칙(fs-rest 경계, Android/data 제외) 검증
3. winname: 판정 테이블 — 금지 문자 `: ? " * < > |`, 예약 이름 CON/PRN/AUX/NUL/COM1-9/LPT1-9, 끝 점·공백, 제어 문자, NTFS 대소문자 충돌(a.txt/A.txt 동일 폴더)
4. puller: 해시 정확성(sha256 검증 벡터), mtime 적용, 부분 실패 시 manifest 오류 기록·완결 불가 판정
5. quarantine: 비호환 이름 격리 → tar를 파서로 재읽어 원본 이름·내용·mtime 확인 → 스트리밍 복원 경로(FakeADBDevice의 shell 수신 검증)
6. restore: FakeADBDevice로 전달된 tar·settings put 호출 순서·install 호출 검증
7. providers: content query 출력 파서(키→값 quoting, 유니코드) — fixture는 표준 형식 샘플
8. **왕복 시나리오**: 가상 FS → backup_run(임시 폴더) → 가상 FS 초기화 → restore_run → 해시·mtime 전수 비교

## 구현 순서 (커밋 단위)

1. ✅ `docs(plans)`: 이 설계 문서 + AGENTS.md 예외 기록 + 02-contracts 정정
2. ✅ `feat(backup)`: model·winname·walker + FakeADBDevice 테스트 기반
3. ✅ `feat(backup)`: puller(해시·mtime·4GiB wrap 재확인)·manifest 원자 저장·quarantine 격리
4. ✅ `feat(backup)`: settings·contacts(vCard)·smsie 수집기
5. ✅ `feat(backup)`: backup_run·backup_manifest_check·이벤트 + facade + wizard 연결(플래그 기본 꺼짐)
6. ✅ `feat(restore)`: tar 스트리밍 복원·화이트리스트·APK 세션 설치·smsie 복원 + restore_run·게이트
7. ✅ `feat(front)`: 완결 게이트(언락/리락 전 — REAL_STEPS.backup 켜졌을 때) + journal backupDir 재개
8. ✅ `docs(plans)`: 상태 배지 갱신 + 검증 대기 항목 표

## 리스크·검증 대기 (실기기 확인 필요 항목)

| 항목 | 상태 | 대응 |
|---|---|---|
| ~~/sdcard/Android/data 셸 접근 (Android 11+)~~ | **실측 해소(2026-10-03)** — XQ-DQ44·Android 15에서 셸 열거 확인 | (예비 대응 유지) 실패 시 manifest 오류 + UI 경고 |
| content query 셸 권한(연락처)·smsie 권한(pm grant·cmd role) | 미실측 | 연락처 실패는 항목 partial(위장 성공 금지), smsie 권한 실패는 로그만 |
| toybox tar 스트리밍 복원 호환(ustar 긴 경로) | 미실측 | GNU longname 헤더 사용, 단위 테스트로 아카이브 구조 검증 |
| tar 스트리밍 백분률 산정 | 설계 | 항목 시작/종료 시점 보고(exec 블로킹) — 스트리밍 중 실시간은 후속 개선 |
| 백업 폴더 ACL(현재 사용자 한정, §12.5) | 미결정 | windows-sys로 직접 구현 시 범위 증가 — 1차 생략하고 문서 기록, 2차 선택 구현 |
| 대용량(수십 GB) 이벤트 빈도 | 설계 | 파일 단위 이벤트 + 50ms 쓰로틀(구현 완료) |

## 통신 데이터 접근 조사 (2026-10-03, 웹·GitHub)

- **문자(SMS)·통화 기록 직접 쿼리는 최신 Android에서 불가**: `content query --uri content://sms`·`content://call_log/calls`는
  READ_SMS/READ_CALL_LOG 런타임 권한 필요(4.4+)한데 adb shell(uid 2000)은 미보유 → SecurityException.
  최신 SO·포렌식 가이드("helper APK 없이는 불가") 확인. (Android 15 XQ-DQ44 기준 기대 불가)
- **연락처는 adb shell에서 가능**: `content://com.android.contacts/contacts` 조회 + `as_vcard` URI로 vCard 개별 조회 —
  Android 14 실사례(포렌식 triage 스크립트, XDA LineageOS 21 복구 사례)로 동작 확인됨. 쓰기(복원)는 확실치 않음 → 복원은 vcf push + 연락처 앱 가져오기(수동 1탭) 또는 sms-ie.
- **SMS Import/Export (github.com/tmo1/sms-ie, GPL-3.0)**: 루트 불필요, 문자+MMS+통화기록+연락처+차단번호 (ND)JSON 왕복.
  자동화 intent 없음(SAF 파일 선택 UI) — 하지만 설치(`install`)·권한(`pm grant READ_SMS READ_CALL_LOG READ_CONTACTS`)·
  기본 SMS 앱 역할(`cmd role add-role-holder android.app.role.SMS <pkg>`)·파일 전송은 PC에서 자동화 가능.
  APK는 GitHub Releases에서 서명 인증서 SHA-256(C1:05:E6:D9:…:BE) 공개 — 런타임 다운로드 후 지문 검증(D12 Magisk 패턴, 번들하지 않음).
  구현(2026-10-04, `apk_verify.rs`): 릴리스 API 자산 다이제스트(SHA-256)·크기 대조 + APK Signing Block 서명자 인증서 핀(`c105e6d9…fdbe`, v2.11.1 standard-release에서 계산·openssl 교차 확인 — 위 공개 지문과 일치)을 통과해야 캐시에 저장·설치. 다이제스트 기록(`<apk>.sha256`)과 맞는 캐시만 재사용, 오프라인이면 검증된 최신 캐시만. 인증서 핀은 서명 암호 검증이 아니며 설치 시 Android가 서명을 검증한다. F-Droid 빌드(다른 키)는 해당 없음.
  주의(앱 README): 기본 SMS 앱 전환 중 수신 문자 유실 방지를 위해 비행기 모드 안내 필요.

### sms-ie 세미수동 흐름 (문자·통화 기록)

백업: [자동] APK 확보(캐시/GitHub) → install → pm grant → [수동 개입: 앱에서 "Export messages/call logs" + 대상 폴더 선택(기기 측 smsie-tmp)]
→ [자동] 산출 파일 pull → manifest 해시 기록 → 기기 측 임시 삭제
복구: [자동] 파일 push → pm grant(쓰기 권한) → [수동 개입: 비행기 모드 안내 + "Import"] → [자동] 역할 원복 안내·완료 검증(문자 개수 `content query --uri content://sms`…는 권한 차단이므로 앱 화면 표시 수치 사용자 확인)
— 수동 개입은 기존 ManualPrompt 프레임워크(magisk-patch와 같은 패턴)로 단계화. 완결 게이트는 "파일 수신+해시 일치"까지만 보증(내용 검증은 앱 책임).

## 경로 기준 (실측 2026-10-03, XQ-DQ44 · Android 15 — examples/probe_sdcard.rs 읽기 전용 조회)

- `/sdcard` = `/storage/emulated/0` (내장 공유 저장소, 사용자 0) — adb 셸·SYNC 표준 경로. **기기 실측 확인**
- **`/sdcard/Android/data` 셸 접근 가능 실측** — Android 15에서도 shell로 열거됨(아래 리스크 표에서 해소)
- **외장 microSD 제외 (사용자 확인 2026-10-03)** — 기기에 `/storage/439F-190E` 감지됐으나 백업 범위에서 제외.
  외장 SD에 중요 파일을 둔 경우 이 백업에 포함되지 않는다는 점을 UI 안내에 남긴다(향후 항목 추가 가능)
- 직장 프로필(work profile, `/storage/emulated/10` 등) 제외 — 사용자 0만

## 의사결정 기록 (사용자 승인 2026-10-03)

1. **복원 = tar 스트리밍 통일** — adb_client push는 mtime 미보존(0777 고정)이므로, 폴더(항목) 단위 tar를 기기 셸 `tar -xf -`로 스트리밍해 mtime·원본 이름 보존. quarantine 복원과 같은 구현 재사용.
2. **통화·문자·연락처 = 하이브리드(C안)** — 연락처는 adb vCard 직접(자동·완결 게이트 포함), 문자·통화기록은 sms-ie 세미수동. 전 항목 실측 전까지 "검증 대기" 배지.
3. **실전 실행 플래그 기본 꺼짐** — 데스크톱 빌드에서도 시뮬레이션이 기본. `REAL_STEPS` 플래그 전환으로만 실동작.


## 실기기 검증 (2026-10-03, XQ-DQ44 / Android 15 — 사용자 요청, 폰에서 읽기만)

- 대상: 설정·APK·연락처·Download·Music·Documents·Recordings·그 외 파일(사진·영상·앱 데이터 40GB·문자·통화 제외)
- 결과: 460파일 · 11.2GB · 682초 — 파일 455개 크기·해시·수정 시각 재대조 문제 0, APK 323개 zip 정상, 연락처 295명 vCard(전화 312건) 폰과 일치
- 이어서 백업: 같은 폴더로 재실행 → 끝난 7항목 건너뛰고 미완료(연락처)만 다시 받아 2초 만에 완결
- 복원 모의 실행(live_restore_dryrun): 실제 백업 폴더를 가짜 기기로 복원 — tar 133개 파일 이름·크기·해시·수정 시각, APK 321개 바이트, 연락처 파일, 설정 명령 대조 문제 0
- 실측으로 고친 것
  - 연락처: 셸에서 contacts/<id>/as_vcard는 "No files supported by provider"로 읽을 수 없음 → raw_contact_entities(전원·전체 행)를 읽어 vCard 3.0 생성.
    포함: 이름·전화·이메일·회사/직함·주소·메모·별명·웹사이트·생일 / 미포함: 사진·그룹(라벨)·메신저
  - 설정 화이트리스트: stay_on_while_plugged_in은 global 네임스페이스
- 백업 폴더: 시작 시 backup_prepare가 지정 폴더 아래 backup-<시각>-<모델> 생성 후 절대 경로 반환 → 진행 기록(journal backupDir)에 먼저 저장.
  끊기면 같은 폴더로 이어서: 끝난 항목 건너뜀, 미완료 항목은 이전 파일을 지우고 다시 받아 덮어씀, 격리 세그먼트 번호는 이어서
- 연락처 복원: 복구 후 "연락처 가져오기" 수동 단계(연락처 앱 → 설정 → 가져오기 → .vcf → contacts-restore.vcf) + 폰 연락처 수 ≥ 백업 수 확인(contacts_restore_check)
- 실행: XVOLTE_LIVE_BACKUP_DEST / _ITEMS / _RESUME 로 live_backup, XVOLTE_RESTORE_DRYRUN_DIR / XVOLTE_RESTORE_SPOOL 로 live_restore_dryrun (둘 다 #[ignore])

## 코드 리뷰 반영 (2026-10-04, 실기기 테스트 없음 — FakeADBDevice 단위 테스트)

- 복원 `exec`: adb_client는 stdin 전송 직후 반환하고 출력은 별도 스레드가 읽는다. 출력 수집기가 해제될 때(기기 명령 종료)까지 최대 300초 기다린 뒤 종료 코드 표식을 판정한다. 끝나지 않으면 실패.
- tar 스트리밍: 헤더 크기는 manifest 기록값, 파일이 짧으면 실패(아카이브 어긋남 방지). 이름은 유닉스 경로 바이트 그대로(Windows에서 `\`가 `/`로 바뀌지 않게). 파일마다 진행률 보고. 격리 복원도 `-C /sdcard` + 상대 이름, 복원할 것이 없는 세그먼트는 기기 명령을 보내지 않는다.
- 격리 세그먼트: 추가 실패 시 그 항목을 잘라내고 세그먼트를 닫는다. 검증은 세그먼트별 오류를 따로 모아 한 세그먼트 손상이 전체 격리 검증을 막지 않는다. 시작 시 남은 `.qtmp-*` 정리.
- 파일명: 기기 파일명 안의 `\`·`/`는 금지 문자(격리). 다른 기기 파일이 같은 로컬 경로를 차지하면 충돌(격리). 예약 이름에 COM0/LPT0·위첨자·CONIN$/CONOUT$ 추가, 끝 공백·점을 뗀 이름으로 판정.
- 이어서 백업: manifest `deviceKey`(SHA-256 시리얼)로 같은 기기 확인(예전 manifest는 마스킹 시리얼로 확인 후 키를 채움). 선택 항목만 재검증하고 문제는 그 항목 오류로 남긴다. 앱 목록 조회 실패는 apk 항목만 partial.
- 연락처: `content query` 출력은 "No result found." 또는 "Row: "로 시작해야 한다(오류 문구+종료 코드 0을 연락처 0명으로 오인 금지). vcf는 원자 저장.
- deviceidle 복원: 실제 덤프 형식 `user,<패키지>,<uid>`의 사용자 지정 항목만 재적용.
- 문자·통화 수집: 파일 백업과 같은 pull 경로(해시·mtime·4GiB 이상 크기 재확인). 고른 항목이 모두 끝나면 기기 임시 폴더 삭제.
- 요약: 진행 전(Pending)·사유 없는 partial 항목도 "<id>: 미완료"로 이유를 보인다. 무결성 검증에서 멈춘 복원은 문자 수동 복원을 이어 안내하지 않는다.

2026-10-06 후속 검토: manifest의 optional excludedItems는 이번 선택만 나타낸다. 완료 상태·산출물은 선택 해제로 바꾸거나 삭제하지 않으며 재선택 시 로컬 전수 검증 후 재사용한다. 선택 항목만 완결·요약 판정에 포함한다. 격리 tar는 항목별 끝 표시·sync 이후에 manifest를 저장한다. 취소는 폴더/파일 사이에 확인하고 NO_SPACE는 다음 파일·항목을 시작하지 않는다.

2026-10-06 문자·통화 안내 흐름: 폰의 고정 작업 폴더는 /sdcard/volte_sms_backup. 앱 설치·권한·폴더 생성·ADB 앱 실행까지만 자동, 내보내기/가져오기/저장 버튼은 사용자 조작이다. 기본 산출물은 messages-YYYY-MM-dd.zip 및 calls-YYYY-MM-dd.json이며 실제 판별은 날짜와 중복 번호를 고정하지 않는 messages*.zip/calls*.json. 생성 감지 후 ZIP/JSON·CRC·원본 크기/mtime·해시를 검사하고 PC smsie/에 저장한다. 앱 성공 확인과 manifest 저장 전에는 폰 사본을 삭제하지 않는다. 복구는 같은 고정 폴더로 실제 manifest 파일을 전송하고 해당 파일명을 사용자에게 안내한다.

2026-10-06 연락처 추가 내보내기(후속 사용자 요청으로 제거): 처음에는 contacts*.json을 VCF와 함께 보관했으나, 현재는 기본 contacts/contacts.vcf 백업·복원만 유지한다. SMS 수집기는 날짜·중복 번호·대소문자와 관계없이 contacts*.json을 읽기 전에 제외하며, 연락처 JSON은 문자·통화 준비/완결 판정에 영향을 주지 않는다. 연락처 기록에 JSON을 병합하던 분기와 JSON 연락처 검증을 제거했다. 기존 PC 백업의 추가 JSON도 사용자 요청에 따라 제거하고 영수증·합계를 갱신한다.

2026-10-06 폰 미연결 작업: 사용자가 SIM 변경으로 기기를 끈 뒤 기기 실행을 중단했다. 재개 검사 뒤 각 폴더 항목의 목록 열거 전에 scan 진행 이벤트를 먼저 보낸다. LIST/STAT의 마지막 응답 수신 확인·분할 목록·잘린 응답을 오프라인으로 보완하며 실기기 지연의 원인을 확정하지 않는다. 설정·연락처(VCF+JSON)·문자·통화·APK 완료 영수증은 유지한다. 사진·앱 데이터 등 나머지 항목과 전체 백업 완결은 기기 재연결 뒤 검증한다.

2026-10-06 재연결 관찰: 잠금 해제 후 Android/data의 LIST/QUIT/CLSE 응답이 계속 완료되고 수천 개 하위 폴더를 열거하는 것을 확인했다. 단일 scan 시작 이벤트만으로는 정상 열거를 정지로 오인할 수 있어 폴더별 발견 진행을 추가했다. scan의 filesDone은 0이며 filesTotal은 현재 발견 파일 수, file은 폴더·파일 확인 수 안내이다. 복사 진행률로 계산하지 않으며 GUI 로그는 초당 한 번으로 제한한다. 완료 판정은 여전히 전체 선택 항목 Done 및 PC 파일 해시 검증으로 결정한다.
2026-10-06 파일 수신 오류: 앱 데이터 21,933개 폴더 목록 조회 완료 뒤 USB RECV의 짧은 응답 슬라이스 패닉으로 실행이 실패했다. 완료된 5개 항목은 유지되고 나머지는 미완료다. 경계 독립 스트리밍 수신·명시적 종료/실패 처리 및 회귀 테스트를 추가하며 같은 백업 폴더로 재개한다. 케이블·잠금 문제로 확인된 오류가 아니므로 이번 개발 CLI 감독 팝업은 실제 오류 원인을 포함한다.

2026-10-06 전환 점검: 앱 데이터는 권한 거부 3건(파일 2개·목록 조회 불가 폴더 1개) 때문에 partial이며, 완료만 기다리는 감독 조건이 실행 파일 교체를 놓쳤다. DCIM은 기존 엔진에서 761개·139,672,493,772바이트 복사 후 done으로 저장됐다. 진행 중 작업을 다시 중단하지 않고 이 저장 지점에서 전환하도록 PC 세션 감독 조건을 고쳤다. 강제 종료 뒤 첫 새 연결에서 CLSE 응답 오류가 났고 한 번 재시도 후 사진 샘플 ABBA 4회 해시 검증이 통과했다. 현재 남은 백업은 수정한 세션 재사용 엔진으로 재개 중이다. 최종 전수 PC 검증과 권한 누락 해결은 아직 완료되지 않았다. 재연결 단계에는 늦게 도착하는 이전 스트림 메시지를 시간·개수 제한 안에서 무시하는 ADB 프로토콜 처리를 추가하며, 실행 중인 백업 프로세스의 실행 파일은 교체하지 않는다.

2026-10-06 재개/검증 비용 개선:
- PC 파일 해시 검사는 CPU 가용 수에 따라 최대 4개 작업자로 실행한다. 공유 작업 큐에서 파일을 하나씩 가져와 크기 편차에 따른 대기를 줄이고, 오류는 원래 목록 순서로 보고한다. ADB 세션과 GUI 콜백은 이 작업자에 전달하지 않는다. 저장장치/CPU에 따라 실제 개선 폭은 달라진다.
- 재개 전 검증에서 정상 파일 영수증도 반환한다. partial 항목을 보존하기 위해 같은 파일을 다시 해싱하거나 파일마다 임시 ItemRecord를 만드는 두 번째 검증을 없앴다. 영수증은 이번 실행 메모리에서만 재사용하고 다음 실행에서는 디스크 파일을 다시 검사한다.
- partial 파일 항목은 원본 목록의 크기·수정시각이 검증한 영수증과 같으면 재전송하지 않는다. 일반 파일은 기존 로컬 경로·크기·수정시각도 확인하고, 격리 파일은 검증한 tar 영수증을 재사용한다. 시각 0 또는 32비트 LIST 크기 wrap 등 비교할 수 없는 경우는 기존 pull로 처리한다. 원본 내용이 바뀌면서 크기와 수정시각이 모두 그대로인 변화는 이 quick-check로 감지하지 않는다(일반 파일 전송 시스템의 메타데이터 기반 재개와 같은 한계).
- 새 기록과 기존 기록의 병합은 remote→index 맵으로 처리하여 파일마다 전체 목록을 검색하지 않는다. 원본에서 삭제된 정상 사본 및 변경 파일의 재전송 실패 시 이전 정상 사본을 보존한다. 보존된 사본도 경로 충돌 검사에 등록한다.
- 마지막 PC 전수 해시 검사는 유지하며 병렬 검사 경로를 사용한다. 읽기 권한 누락과 손상은 여전히 partial/오류이며 완료로 바꾸지 않는다. 실행 중인 USB 복사는 중단하거나 실행 파일을 덮어쓰지 않는다.
- 회귀 검증: partial 재개에서 정상 일반/tar 파일의 pull 0회, 변경/실패/손상/시각 미확인 파일만 재전송, 최종 PC 검증 통과; 변경 원본의 재전송 실패 및 원본 삭제 시 이전 사본 유지; 병렬·순차 검사 결과 동일 및 손상/중복/누락 영수증 제외.
- PC 표본 실측: 같은 SSD 백업 파일 11개·1,073,722,564바이트, 캐시 예열 후 release ABBA 4회 모두 완결·크기 일치. 평균 순차 0.681초 → 병렬 0.217초(약 3.14배). 캐시 포함 표본으로, 전체 전수 검사 개선 폭은 별도 측정한다. 현재 복사는 기존 엔진을 유지하고, 종료 기록 확인 후 PC 검사만 별도 병렬 검사 실행 파일로 전환한다.

2026-10-06 최종 결과 및 접근 거부 진단:
- PC 파일 검사 완료: 194,630,644,037바이트, 18:13:43.963~18:16:35.465(171.502초). 수집 사본의 해시/크기 오류는 없으며 앱 데이터의 원본 접근 거부 3건만 남았다. 13/14항목 done, app-data partial을 유지한다.
- 명시적 읽기 진단에서 shell UID 2000, ext_data_rw GID 1078을 확인했다. 거부 파일 2개는 mode 700이고 앱 UID 소유, 거부 폴더는 mode 770이며 소유자·그룹 모두 앱 UID다. 원본 /sdcard와 /storage/emulated/0 양쪽에서 폰의 head/ls도 동일하게 거부했다. 캐시 위치라고 자동 제외하거나 중요하지 않다고 단정하지 않는다.
- 개발용 ignored access_diagnostics는 환경 입력의 제한된 공유 저장소 경로만 조회하고 대상 기기 해시를 대조한다. id/권한/앱 UID/1바이트 읽기 또는 목록 확인 및 제한된 보안 로그를 PC 보고서로 저장한다. chmod·삭제·루트 변경 없음. 제품 빌드에 포함되지 않는다.
- 요약은 항목당 첫 오류만 반환하던 동작을 고쳐 모든 고유 원인을 반환하고, 수집/검증/파일 기록에 같은 오류가 있어도 중복하지 않는다. 모든 원인이 읽기 권한일 때는 앱 접근 불가와 별도 데이터 보관 안내를 표시하며, 혼합 I/O/해시 오류는 일반 실패로 남긴다. 미완결 게이트는 유지한다.

## 2026-10-06 후속 사용자 결정: 읽기 권한 거부 앱 데이터 전체 제외

앞선 partial 유지 관찰 이후 사용자가 권한 오류 앱들의 **PC 앱 데이터 백업 전체 삭제**를 선택했다. 신규 백업 종료 후에도 같은 정책을 적용한다. APK 및 다른 백업 항목·폰 원본은 대상에서 제외한다.

- omissions 모듈은 선택된 app-data 기록의 `/sdcard/Android/data/<package>` 원본 SYNC RECV Permission denied 또는 폴더 열거 권한 거부를 판별한다. 패키지·기기 모델·사용자 경로를 하드코딩하지 않는다. PC 쓰기 권한, USB, 해시 오류는 이 정책으로 무시하지 않는다.
- `android-data/<package>` 폴더 전체와 모든 quarantine tar 세그먼트의 해당 앱 사본(과거 중복 버전 포함)을 삭제한다. 다른 tar 경로·원시 이름·mtime·내용은 유지하며 삭제할 항목이 없는 세그먼트는 다시 쓰지 않는다. 일반 파일/정션·루트 경계 사전 확인, tar 정확한 길이 읽기·원자 교체를 사용한다.
- 삭제 전에 manifest에서 해당 앱 복원 영수증을 제거하고 `omittedApps: [{package,reasons,removedFiles,removedBytes,cleanupPending:true}]`를 원자 저장한다. 삭제 완료 후 pending을 false로 저장한다. 중단·실패 시 pending이 완결/복원을 차단하며 다음 실행이 동일 제외의 정리를 이어서 처리한다. 승인된 제외 앱 데이터는 재개 시 다시 수집하지 않는다.
- removedFiles/removedBytes는 제외한 정상 복사 영수증의 합계다. 폴더에 남은 과거 임시 파일이나 tar의 과거 중복 사본도 제거하지만 이 영수증 합계에 중복 가산하지 않는다.
- 정상 정리 후 남은 선택 항목이 모두 정상일 때에만 complete=true다. 이는 제외 앱 데이터가 없는 범위의 완료이며 전체 앱 복원 성공을 뜻하지 않는다. UI는 제외 앱 안내 후 명시적인 [다음]을 기다린다. 다른 권한 거부·전송·해시 오류는 여전히 partial/complete=false다.
- 개발 CLI의 `backup_clean_unreadable_apps`는 기존 완료/부분 완료 PC 폴더에 같은 정책을 적용한다. 기기 연결·쓰기 없이 공통 작업 잠금을 사용한다.
- 회귀: 전체 앱 삭제/다른 앱·APK 유지, 혼합 오류 유지, 선택 해제 보존, 경로 오류/Windows 정션 차단, tar 모든 버전 제거·긴 원시 이름/mtime 유지, 정리 실패 후 복원 차단·재시도.

## 2026-10-06 원본 속성 보존 및 Claude 리뷰 후속 수정

원본 소유자/그룹(UID/GID 및 이름), 전체 st_mode(유형/특수 비트 포함), 크기, inode, hard-link 수, 블록 수, atime/mtime/POSIX ctime, 제공될 때의 birth time, 노출될 때의 SELinux context를 source_metadata로 기록한다. 시각의 원시 문자열도 저장해 제공되는 소수초/시간대 정보가 Windows 파일시스템을 왕복해도 사라지지 않게 한다. ctime은 생성 시각으로 사용하지 않으며 Toybox의 미지원 %W/%w는 null이다. ACL/임의 xattr/심볼릭 링크 대상은 지원하지 않는 것으로 명시한다. /sdcard의 속성은 FUSE가 합성한 shell-visible 값일 수 있고, 기록 보존은 소유권/보안 레이블 재적용 권한을 부여하지 않는다. 복원에서 이전 숫자 UID를 그대로 chown하지 않는다. 전체 앱 복원 성공이나 모든 메타데이터 재적용을 보장하는 기능이 아니다.

신규 파일 백업은 LIST 전체 열거(빈 디렉터리 포함) → stat 배치(128개/24KiB 이내, 경로 단일 인용, 파일명 출력 정렬에 의존하지 않음) → 원본 속성 sidecar와 manifest 선행 저장 → 기존 copy/hash → size+mtime와 payload hash 연계 → 최종 sidecar/manifest 저장 순서다. sidecar는 content-addressed JSON이며 manifest에 SHA-256 영수증을 저장한다. 검증/복원 게이트에서 sidecar 손상도 오류로 처리한다. 원본 읽기 후 변경된 속성은 역사적 원본 속성으로 추정하지 않는다. 재개 시 재사용하는 기존 payload의 원본 속성 기록을 보존한다.

기존 백업의 개발 CLI backup_metadata_enrich는 같은 기기 해시를 대조한 뒤 속성만 읽어 추가한다. 결과는 after-copy-enrichment이며 예전 접근 시각/변경된 소유자/사라진 빈 폴더를 소급 복구했다고 주장하지 않는다. 이미 있는 원본 snapshot은 덮어쓰지 않는다. 파일 내용 재전송/루팅/chmod/폰 원본 삭제가 없다.

GUI의 명시적 '백업만 실행'은 backupOnly 옵션으로 backup 한 단계만 만들고 live backup만 활성화한다. REAL_STEPS 기본값/Cargo default=[]는 유지한다. 패치/언락/루팅/복원 옵션을 표시하지 않으며 저장 기록의 backupOnly 계획에 다른 엔진이 끼면 거부한다. 일반 시작은 backupOnly를 해제한다. 기존 작업 기록을 먼저 읽고 재개/새 시작을 처리한다. 완료 화면의 실제/목업 표기도 이 모드를 따른다.

리뷰 결함 수정:
- smsie_probe: 파일 목록만 읽는다. GUI의 확인은 진행 중인 probe를 기다린 뒤 collect를 하나만 실행하고 실제 IPC 오류를 표시한다. 본 백업 미완결이면 export를 열지 않으며 Done 문자/통화 백업은 재수집하지 않는다. 자동 진행이라는 잘못된 문구를 제거하고 export 모달의 Esc/배경 닫기는 막는다.
- resume 전수 PC 해시: 최대 4개 작업자를 유지하고 1MiB 읽기마다 취소 확인, 파일/바이트 진행을 GUI로 보낸다. UI callback은 호출자 스레드에서만 실행한다. 언락 전 backup_manifest_check도 같은 진행/실행별 취소 경로를 사용한다. 파괴 단계의 디스크 전수 검증을 단순 존재/mtime 검사로 대체하지 않는다.
- omissions: 제외 대상이 없으면 삭제용 폴더/재분석 지점 검사를 하기 전에 반환한다. 실제 삭제의 경계/정션 검사는 유지한다. 제외 앱의 현재 source-metadata도 필터링한다.
- ADB OPEN: 10초/16응답 이내 늦은 다른 스트림 CLSE를 처리하고 실패하면 sync_broken을 설정하여 연쇄 OPEN을 차단한다.
- RECV PC 쓰기 실패: DATA 중간에 QUIT을 보내지 않고 ADB CLSE로 해당 스트림을 닫아 남은 WRTE를 제한적으로 ACK/폐기한다. cleanup은 10초/1024프레임 제한이며 원격 CLSE를 이미 받은 스트림은 즉시 종료한다. 원래 PC 쓰기 오류를 보존하며 복구 가능한 경우 다음 파일은 새 스트림을 연다.
- APK 재개: 설치 경로 변경을 재검사한다. 설치 경로 해시를 포함한 파일명으로 새 base/split 코호트를 받으며 성공한 경우 이전 코호트를 활성 영수증에서 교체한다. 실패 시 이전 정상 사본을 보존하고 Partial로 남긴다. base.apk 부재도 오류다. 복원은 manifest의 활성 코호트만 설치한다.
- 손상 tar: 정상 헤더와 manifest hash에 맞는 payload만 원자적으로 살리고 복구 못한 영수증은 Partial로 표시하여 재전송한다. 다른 세그먼트의 검증된 파일을 버리지 않으며 미검증/손상 내용을 완료로 승격하지 않는다.
- .xvolte-PID-SEQ.tmp: manifest로 확인된 PC 백업 안의 정확한 생성명만 정리한다. 연결/재분석 지점은 따라가지 않고 명시적 영수증에 있는 이름은 보존한다.
- 문자 정리: 검증/확인/manifest 저장 후 선택 항목의 정확한 파일 경로만 rm -f 한다. 다른 사용자 파일/폴더/연락처 JSON은 건드리지 않는다. 복원 후에도 폴더 전체를 지우지 않고 파일을 남겨 직접 삭제하도록 안내한다.
- 실패 모달의 키에서 로그 길이를 제거하여 로그가 늘어날 때 재팝업하지 않는다. 단계별 [다음] 및 제외 앱 안내 확인 절차는 유지한다.

권한 거부 앱의 전체 PC 앱 데이터 삭제/해당 백업의 영구 제외는 사용자 명시 선택(2026-10-06)이다. APK와 폰 원본은 보존한다. 잘못된 정책으로 재확인하여 작업을 멈추지 않는다. 다시 수집하려면 새 백업을 시작하고 해당 앱의 내보내기/접근 가능 여부를 별도로 확인한다.

후속(리뷰 진행 중 추가): source_metadata의 기기 통신 실패는 METADATA_TRANSPORT로 분리하여 이분 재조회하지 않고 중단한다. 각 Attributes에 observedAt/captureContext를 추가해 재개에서 보존한 과거 관찰값의 출처를 구분한다. 기존 디렉터리 속성/사라진 빈 폴더의 기록도 보존하며 원본 payload association이 false인 경우 receipt complete=false 및 구체 사유를 반환한다. 실패한 APK 교체 등에서 stale payload hash 연계는 associate에서 초기화한다. 메타데이터 재개 병합은 HashSet으로 존재 판정을 하여 파일 수 제곱 탐색을 제거하고 publish 직렬화/해시 중복을 없앴다. tar repair scratch는 고유 .xvolte-PID-SEQ.tmp를 create_new로 먼저 예약하여 기존 파일을 덮어쓰지 않는다. source_metadata_enrich는 개발 기능에만 컴파일된다. 이 후속 변경들은 추가 회귀 검증 예정이며 최종 리뷰 시 현재 코드를 다시 확인할 것.


## 2026-10-06 재검사 지적 후 추가 수정

사용자가 기존 PC 백업 폐기를 명시 지시했고 Claude가 D:\20261006\backup-20261006-114718-XQ-DQ44를 삭제했다. 폰은 초기화되지 않았고 원본이 남아 있다. .xva-session은 진단 기록으로 보관한다. 기존 194GB에 enrich나 재해시를 실행하지 않으며 새 백업 경로를 검증한다.

- H1: Receipt.captureContext로 사후 enrichment를 원본 snapshot과 구분. 사후 누락은 원래 내용 백업의 complete/restore를 막지 않고 별도 경고, 미완결 enrichment는 재시도/교체 가능. CLI Vec<Receipt>의 complete=false는 실패 기록. 원본 snapshot은 덮어쓰지 않음.
- H2: 정상 tar에 대한 PC IO 오류를 구조 손상으로 처리하지 않음. 구조 손상만 salvage, 다른 IO는 즉시 오류 반환. 원본 손상 세그먼트는 recovery에 별도 보관(제외 앱 데이터가 포함되면 보관하지 않는 승인 정책 예외). 사후 tar 검사 오류가 있으면 타 영수증을 오염시키지 않고 반환.
- M1/M2: 실패한 변경 원본 재수신 후 정상 이전 payload를 사용하면 그 SHA에 대응하는 이전 속성/observedAt도 복구. 동일 size/mtime/type의 선저장 stat은 해시 영수증이 아직 없어도 보존.
- M3: 누락/손상 sidecar는 새 관측으로 회복, 잃어버린 이전 속성을 복구했다고 주장하지 않고 limitations에 남김. 제외 정리의 손상 sidecar는 미완결/재수집 사유로 기록하고 앱 본문 정리를 막지 않음.
- M4/M5: 결정적인 형식 해석 실패는 단일 경로 3회에서 중단. 단일 stat의 ENOENT 문구를 확인한 관측 중 소멸은 SOURCE_GONE로 기록하며 새 내용 백업을 막지 않음. 사후 보정에서는 누락 경고로 남김.
- M6/R4 최종: 항목 전체 전송 뒤의 재-stat은 제거했다. 이미 복사된 파일의 후속 변경까지 오류로 잡는 구간이며 중복 기기 조회 비용이 있다. 복사 전 속성 snapshot/관찰 시각과 실제 수신 크기·PC hash 연계를 보존한다. 사용 중 앱의 원자적인 filesystem snapshot이나 데이터베이스 일관성을 보장하지 않는 한계를 명시한다.
- M7/M8: 재개 검사를 취소하면 검사 도중 변경된 상태를 manifest에 저장하지 않음. PC 검사 실패 + 원본 소멸은 이전 영수증에 손실 기록을 남겨 Done으로 사라지지 않게 함.
- M9: 제외 앱 폴더 canonical basename은 패키지와 대소문자까지 일치해야 삭제. 잘못된 패키지 폴더 삭제 차단.
- M10~M15: 단독 백업 이전 버튼은 기기 화면. 이전 기록 Esc는 기기/경고 화면으로 돌아가 새 실행이 기록을 덮어쓰지 못함. 일반 작업 재개는 경고 동의가 필요. 빈 계획 launch 방어/버튼 비활성화. smsie_prepare 완료 후 probe, 재준비는 진행 중 probe를 기다림. 성공한 probe는 이전 오류 제거. 문자 안내 안에 중단 버튼과 기존 중단 확인 제공. backupOnly journal은 kind/wipe/manual/config까지 제한.
- M16/M17: LIST 응답 중에는 incomplete 상태 유지, 오류 시 CLSE 중단/대기 WRTE ACK 소비. 잘못된 UTF-8을 잘못된 경로로 바꿔 받지 않고 명시 오류. 실제 160KiB/32KiB 프레임 writer 실패 테스트와 미소비 데이터/QUIT 없음 검증, 1024 close/16 OPEN 상한/peer close 테스트 추가.
- M18/M19: stat/push도 공통 finish 경로, shell 실패는 bounded close + reconnect. exec/shell stdin/out은 하나의 세션 리더가 입력 ACK와 출력/CLSE를 순서대로 처리. detached reader와 Drop에서 endpoint를 동시에 읽는 코드를 제거. source/PC read 오류는 다음 서비스에 남은 프레임을 섞지 않음.
- tar hash에도 256KiB 취소와 진행 이벤트. 중복 metadata load 검사 제거. GUI gate의 native 오류는 null로 합치지 않고 실제 오류 전달. 반복 실패 모달은 실패 회차별 표시. USB payload를 할당 전에 16MiB로 제한.

검증 중간 결과: frontend 129 pass / svelte-check 0 errors, backup:: 128 pass + 3 ignored, vendor 37 unit + 4 doc pass. 이후 최종 full test 결과로 갱신한다. 새 실기기 전체 백업/복원 검증은 아직 수행하지 않았다.

최종 관측 보완: stat -L로 /sdcard 등 루트 링크의 대상 속성을 수집한다([Toybox 원본](https://raw.githubusercontent.com/landley/toybox/master/toys/other/stat.c)). 권한 거부와 구분되지 않는 ADB mode=0을 소멸로 간주하지 않으며, 단일 stat 오류의 ENOENT 문구가 확인될 때만 SOURCE_GONE로 분류한다. 원본 시각의 부호 있는 epoch와 Toybox의 unsigned 출력도 해석하며 원시 문자열은 그대로 보존한다. 중단 확인 모달은 안내 모달보다 높은 z-index를 사용한다.

R1–R7 최종 구현 상세와 검증 결과는 [backup-review-followup.md](backup-review-followup.md) 참조. 주요 후속: truncated tar EOF/GNU longname 분류, 실제 SHA가 있던 파일만 손실 기록, 속성 재사용 SHA+size+mtime 조건, 항목 전체 후속 stat 제거, 문자 준비 busy/중단 확인, 제외 앱이 들어 있는 진단 .damaged 정리, 성공한 exec abort 뒤 연결 재사용. 최종 frontend 132, Rust lib 293+integration3, vendor39+doc4 통과. check/build 성공. 새 실기기 전체 백업과 복원은 아직 미검증이다.

최종 낮음 결함 보완: stat 형식·연결 중단 사유와 사용자 취소를 구분한다. PC_READ_IO(공유 잠금/읽기 거부 등)는 재개 검사 뒤 기존 manifest 저장/폰 재수집 전에 실제 오류로 반환한다. 실제 손상·NotFound는 기존 재수집 흐름을 유지한다. 최종 frontend133/Rust295+integration3/backup134/vendor39+doc4 통과, check/build 성공.

## 2026-10-07 원본 속성: 복사 시점 기준 (사용자 지시)

- **배경:** 실기기 GUI 백업에서 `com.towneers.www/app_shareds`가 문제를 일으켰다. 복사 직전 stat 값은 1,731B였고, 그 뒤 앱이 파일을 479B로 다시 썼다(mtime 00:06). 받은 사본은 479B이며 해시 검증을 통과했다. 그런데 "미리 잰 stat과 사본 불일치"를 미완결로 처리해 백업 전체가 실패했다.
- **원칙:** 백업은 각 파일이 복사된 순간의 내용이다. 무결성은 받은 바이트의 크기와 해시로 보장한다. 미리 잰 stat과 다르다는 이유로 실패, 미완결, 재수신을 하지 않는다. `publish`의 complete와 errors에서 payload 불일치를 뺐고, 불일치 수(`payloadMismatches`)는 정보로만 남긴다.
- **`refresh_copied`:** 항목 끝에서 미리 잰 값과 어긋난 파일만 다시 stat한다(`captureContext=copy-time`). 크기가 받은 사본과 같으면 그 관측으로 원본 속성을 바꾼다.
  - 일반 PC 파일은 mtime과 영수증 mtime을 실제 시각으로 맞춘다.
  - 격리 tar 파일은 헤더 mtime을 유지한다.
  - 복사 뒤 또 바뀌어 크기가 다르면 이전 관측을 그대로 둔다.
  - 다시 받지 않는다.
- **제외 정책 범위:** 권한 거부만 대상이다(라이트룸 사례). 넓히지 않는다.

## 2026-10-07 백업 폴더 이름 고정과 기존 백업 갱신 (사용자 지시)

- **폴더 이름:** `backup-<시각>-<모델>` 대신 저장 위치 아래 `xva-<모델>-backup` 하나를 쓴다. 이전 이름도 삭제·정리 검사에서 이 앱의 폴더로 인정한다(`is_backup_dir_name`).
- **`prepare_backup_root`:**
  - 폴더가 없으면 새로 만든다.
  - 있으면 같은 폰(device_key)의 manifest인지 확인하고 `existing=true`로 돌려준다.
  - 기록이 없거나 손상된 폴더, 다른 폰의 백업, 링크·파일이면 거부하고 건드리지 않는다. 빈 폴더는 새 백업으로 쓴다.
- **기존 백업 갱신**(`resume_dir` 없이 찾은 폴더):
  - 재개와 같이 같은 기기를 확인하고 PC 검사를 한다.
  - 완료 항목도 다시 훑는다. 파일 항목은 안 바뀐 파일을 크기·시각 확인으로 다시 받지 않고, 폰에서 사라진 파일의 사본은 유지한다.
- **초기화 판정:**
  - 연락처·설정·문자·통화는 다시 수집하면 덮어쓴다. 그래서 manifest의 `installKey`(SHA-256 android_id, 공장 초기화마다 바뀜)가 현재 폰과 같을 때만 다시 수집한다.
  - 다르거나(백업 이후 초기화) 읽을 수 없으면 기존 사본을 그대로 둔다. 키가 없는 이전 백업은 이번 갱신에서 기록한다.
- **끊긴 실행의 재개**(진행 기록의 폴더 = `resume_dir`): 기존대로 완료 항목을 건너뛴다.
- **GUI:** 이번에 준비한 폴더는 재개 경로 없이 실행해 엔진이 갱신으로 처리한다. "기존 백업을 찾았습니다 — 바뀐 파일만 받아 갱신합니다"를 로그에 남긴다.
- **실기기 확인 (2026-10-07, XQ-DQ44, CLI):**
  - 이름을 바꾼 기존 백업(`D:\20261006\xva-XQ-DQ44-backup`)을 `backup_prepare`가 `existing=true`로 찾았다. 새 폴더는 만들지 않았다.
  - 1차 갱신(LIST v1)은 47분이 걸렸다. APK 2개와 앱 데이터 약 240개를 새로 받았지만, 4GiB를 넘는 DCIM 동영상 13개(약 100GB)는 크기가 32비트로 잘려 비교할 수 없어서 다시 받았다.
  - 2차 갱신(LIS2 구현 후)은 4분 23초가 걸렸다. DCIM을 포함해 바뀌지 않은 파일은 다시 받지 않았다.
  - 두 번 모두 결과는 complete=true, 72,672개, 194.72GB이고, 초기화 판정 키(installKey)를 기록했다. 문자·통화는 CLI 시험 대상에서 뺐고 기존 사본을 유지했다.
