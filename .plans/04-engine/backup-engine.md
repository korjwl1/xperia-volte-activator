# 백업·복구 엔진 (M3) — 구현 설계

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
