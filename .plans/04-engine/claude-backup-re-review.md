# 백업 재리뷰 (Claude) — 2026-10-06 최종 갱신 (R1~R7 수정 확인 포함)

## 기준

- 대상: `backup-review-followup.md` 끝의 "재검사 지적 후 추가 수정"과 그 뒤 GUI N1~N3 수정까지 반영한 현재 작업 트리 코드.
- 기준 상황:
  - 폰은 초기화하지 않았다.
  - 기존 D: 백업은 삭제했고, 이 코드로 새로 받는다.
  - 그래서 결함은 "새 백업이 오류 없이 완결되는가, 사본을 잃는가"로 판정한다.
  - PC 사본이 유일해지는 상황(초기화 후 재개·복원)에만 해당하는 항목은 **[초기화 전 필수]**로 따로 표시한다.
- 방법: 코드 읽기만 했다. 이번 회차에는 Cargo/pnpm 실행, 코드 수정, 기기 통신을 하지 않았다.
- 표기:
  - **확인함**: 현재 파일로 직접 확인했다.
  - **추정**: 코드 경로는 맞지만 기기 동작이나 발생 빈도는 확인하지 않았다.
- 오프라인 검증: 사용자가 보고한 최종 결과는 Rust lib 289개와 통합 3개 통과, 프론트 129개 통과, `pnpm check` 오류 0, vendor 37개와 doctest 4개 통과다. 이 리뷰에서는 다시 실행하지 않았다.
- 정책: 권한 오류 앱의 PC 앱 데이터 전체 제외는 사용자가 승인한 정책이다. 결함으로 다루지 않는다.

---

## 0. R1~R7 수정 확인 (최종, Codex 후속 반영)

- 범위: Codex 후속 수정 이후 현재 파일에서 R1~R7만 직접 확인했다.
- 방법: 하위 에이전트·Cargo·기기 통신 없이 코드만 읽었다.
- 테스트: Codex가 보고한 결과는 backup 132개, vendor 39개, doctest 4개 통과다. 이 리뷰에서는 다시 실행하지 않았다.
- 결론: **R1~R7 모두 수정됨.** 새로 막히는 결함은 찾지 못했다.

| 항목 | 상태 | 근거 (확인함) |
|---|---|---|
| R1 잘린 tar → PC_IO | **수정됨** | `recovery.rs` `archive_error`가 tar 0.4.46의 구조 오류 문구를 `CORRUPT_ARCHIVE`로 분류한다. 대상은 `unexpected EOF during skip`, `members found describing a future member`, `size overflow`, `failed to read entire block`, checksum/numeric/octal이다. `raw_os_error`가 있는 실제 디스크 오류는 계속 `PC_IO`다(`transient_io_is_never_a_structural_archive_error`). inspect는 `entries_with_seek`와 `raw_file_position + size > 파일 길이` 검사로 본문 중간이 잘린 경우를 잡는다. 재작성 루프는 잘린 본문을 해시 불일치로 건너뛰고 다음 skip EOF에서 `break`한다. 고아 항목이 잘린 경우와 LongName이 잘린 경우를 두 번 실행해도 Err가 나지 않는 회귀 테스트가 있다(`truncated_orphan_and_longname_metadata_do_not_permanently_block_resume`) |
| R2 실패 영수증의 영구 손실 기록 | **수정됨** | `runner.rs` 손실 기록 루프가 `previous_entries.filter(sha256.is_some())`로 실제 PC 사본이 있던 영수증만 대상으로 한다. `previous_entries`는 Files/SmsIe 종류만이라 연락처 수가 remote에 들어가는 contacts는 해당하지 않는다. APK는 성공한 패키지의 이전 cohort를 제외한다 |
| R3 mtime만 바뀐 경우 속성 고착 | **수정됨** | `remember_metadata`의 재사용 조건과 항목 끝 `previous_metadata` 복구가 둘 다 "예전 속성의 size·mtime이 해당 사본 영수증과 일치"를 요구한다(`old.size == file.size && old.mtime as u32 == file.mtime`). `associate`도 `as u32`로 비교해 LIST mtime(u32) 기준과 일치한다 |
| R4 항목 전체 구간의 후속 stat | **수정됨(제거)** | `after-copy-check`/`metadata-check` 경로가 없다. 변경 감지는 복사 전 stat과 사본 size·mtime의 연결로만 한다. 같은 크기·mtime으로 내용만 바뀐 경우는 감지하지 못하며, 이는 limitations의 "not an atomic filesystem snapshot" 범위로 남는다 |
| R5 준비 중 중단 | **수정됨** | `smsiePrepare`가 `this.track(api.smsiePrepare(...))`로 `busy`에 포함된다. 그래서 `requestAbort`가 확인 창을 띄운다(문자 안내 중에는 항상). Rust 준비가 끝날 때까지 `busy>0`이라 [이어서]가 표시되지 않아 잠금 충돌로 재시작하는 경로가 없다 |
| R6 `.damaged`에 제외 앱 데이터 | **수정됨** | `omissions::clean`이 tar 재작성 뒤 `clean_recovery_history`를 실행한다. 생성 이름 형식(`seg<숫자>.tar-<숫자>.damaged`)만 대상으로 하며, 제외 패키지가 들어 있거나 손상 때문에 없다고 증명할 수 없는 진단본을 지운다. 관련 없는 진단본은 보존한다(테스트 `exclusion_removes_damaged_diagnostics_that_can_contain_omitted_app_data`). `.damaged`는 제외 앱이 없을 때만 만들어지므로, 이후 새 제외가 생기면 이 정리가 처리한다 |
| R7 exec 실패 후 연결 broken | **수정됨** | `shell.rs` `bidirectional_session`은 실패 시 CLSE로 중단하고 남은 프레임을 소비한다(`close_sync`). 이 close까지 실패할 때만 `sync_broken`을 설정한다. 상대가 먼저 닫은 경우(`peer_closed`)와 PC 입력 오류 모두 연결을 유지한다. 기기 출력 마지막 8KiB를 오류 문구에 남긴다. 100,000바이트 입력 회귀 테스트가 있다(`exec_serializes_input_ack_output_and_close_without_a_detached_reader`, `early_remote_close_and_aborted_pc_input_allow_following_cleanup`). 두 실패 후 `needs_reconnect()==false`이고 다음 shell(`rm stage`)이 성공한다 |

참고 (결함 아님):
- R1 분류에는 tar의 `two long name entries`, `two pax extensions`, sparse 관련 문구가 들어 있지 않다. 이 앱의 격리 tar는 sparse와 pax를 쓰지 않는다. 끝이 잘려서는 이런 오류가 생기지 않고, 임의 손상에서만 나온다. 그때는 `PC_IO`로 재개가 멈추지만 데이터는 지우지 않는다.

### 0-1. 낮음 3건 후속 수정 확인 (직접 읽기만)

- 범위: 해당 변경만 직접 확인했다. 이미 검토한 코드는 다시 보지 않았다.
- 테스트: Codex가 보고한 결과는 backup 134개 통과(3개 ignored)와 기본 check 성공이다. 이 리뷰에서는 다시 실행하지 않았다.

| 항목 | 상태 | 근거 (확인함) |
|---|---|---|
| 원본 속성 3회 중단이 "사용자 취소"로 표시됨 | **수정됨** | `puller.rs` `CancelFlag`에 사유 슬롯이 추가됐다. `stop_with_error`는 처음 들어온 사유만 기록하고, 사유가 없을 때만 `reason()`이 "사용자가 작업을 취소했습니다"를 반환한다. `source_metadata.rs`는 형식 해석 실패 3회째와 `METADATA_TRANSPORT`/`SYNC_BATCH_BROKEN`에서 `stop_with_error`를 쓴다. 이 사유는 `puller.rs:176`(항목 오류)과 `runner.rs:511`(재개 반환)에 그대로 나온다. 테스트는 사유에 "원본 속성 조회 실패가 반복"이 있고 "사용자"가 없음을 단언한다. GUI와 CLI 모두 `CancelFlag::from_shared(...)` 인스턴스 하나를 실행 전체에 넘긴다(`mod.rs:207`, `:386`) |
| 일시적 PC 읽기 오류로 재개 검증이 Partial을 저장하고 재수집함 | **수정됨** | `paths::read_error`는 `NotFound`를 그대로 두고 그 밖의 오류는 `PC_READ_IO|`로 표시한다. `existing_file`의 canonicalize와 verify의 open/read가 이 함수를 쓴다. 일반 파일, sidecar load, tar 세그먼트(`PC_READ_IO`/실제 `PC_IO`)에서 이 오류가 나면 `stop_with_error`를 켠다(`verify.rs:80`, `:218`, `:396`). `runner.rs` 재개는 검증 직후 `cancel.cancelled()`이면 manifest를 저장하기 전에 `"{실제 사유} — 완료 파일은 보존됩니다"`로 반환하므로 Partial 저장과 폰 재수집이 없다. 파일이 실제로 없으면(`NotFound`) 기존대로 Partial로 처리해 다시 받는다 |
| 감지 성공이 수집 실패 사유를 지움 | **수정됨** | `wizard.svelte.ts` `reportSmsieProbe`는 `smsieProbeError`를 따로 기억한다. 성공하면 `manualCheckError`가 감지 자신의 오류일 때만 지운다. 감지 실패도 기존 오류가 비었거나 감지 오류일 때만 덮어쓴다. 수집 실패 사유가 남는 테스트가 있다(`architecture.test.mjs:871`) |
| `pendingJournal` 차단 테스트 | **수정됨** | 새 wizard에서 `ensureOptions` 뒤 `plan.some(enabled)`로 유효한 계획을 확인한다. 그 다음 `pendingJournal`만으로 `launch`가 막히는지 검증한다(`architecture.test.mjs:911-918`) |

참고: 이 절에 낮음 참고로 적었던 두 가지(사용자 취소가 오류로 표시되는 경우, 같은 문구가 함께 지워지는 경우)는 0-2절에서 수정했다.

### 0-2. 남은 3건 수정 (Claude가 직접 수정, 사용자 지시)

| 항목 | 수정 내용 | 검증 |
|---|---|---|
| 사용자 취소 뒤 오류 중단이 겹치면 취소가 오류로 표시됨 | `puller.rs` `CancelFlag::stop_with_error`가 플래그를 `swap`으로 켠다. 이전에 꺼져 있었을 때만 사유를 기록하므로, 먼저 멈춘 원인(사용자 취소 또는 앞선 오류)이 유지된다. | `first_stop_cause_wins_over_later_errors`: 공유 플래그로 사용자 취소 뒤 오류가 오면 사유가 "사용자가 작업을 취소했습니다"로 남는다. 오류 두 개가 이어지면 첫 사유가 유지된다 |
| 감지 오류와 수집 실패 문구가 같으면 함께 지워짐 | `wizard.svelte.ts`: 문구 비교 대신 `probeOwnsCheckError` 소유 표시를 쓴다. `manualCheckError`를 getter/setter로 바꿔, 감지가 아닌 곳에서 오류를 쓰면 소유가 자동으로 풀린다. 감지 성공은 자신이 쓴 오류만 지운다. | `architecture.test.mjs`: 같은 잠금 오류 문구를 수집이 쓴 경우 감지 성공 뒤에도 남는다 |
| 이름이 UTF-8이 아닌 파일 하나 때문에 폴더 전체 목록이 빠짐 | vendor `list.rs`(직접 연결과 서버 경유 모두)가 해당 이름을 `ADBListItemType::InvalidName`(표시용 lossy 이름)으로 보고하고 DONE까지 계속 읽는다. `walker.rs`는 그 이름 하나만 오류로 남기고 형제 파일과 하위 폴더를 계속 열거한다. 그 파일은 받을 수 없으므로 항목은 Partial로 정직하게 남는다. `PATCHES.md`에 기록했다. | vendor `invalid_utf8_name_is_flagged_and_the_rest_of_the_folder_is_listed`(5바이트 분할, 모든 WRTE 승인). 앱 `invalid_utf8_name_is_one_error_and_siblings_are_still_listed`(형제·하위 파일 열거, 오류 1건) |

오프라인 검증 (2026-10-06, 이 수정 후):
- vendor: 40개, doctest 4개 통과
- backup:: 기본 feature: 133개 통과, 3개 ignored
- 기본 `cargo check` 성공
- `pnpm check`: 오류 0
- `pnpm test`: 133개 통과

아래 1절은 R1~R7의 원래 지적 기록이다. 그 안의 "낮음" 목록과 2~3절은 이번 회차에 다시 검사하지 않았다.

## 1. 원래 지적 기록 (R1~R7은 0절 기준 모두 수정됨)

### 높음

**R1. 끝이 잘린 격리 tar 세그먼트가 `PC_IO`로 분류되어 재개가 영구히 실패한다 — 확인함 (H2 수정의 회귀)**
- 위치: `backup/recovery.rs` `archive_error`(7~23행), 재작성 루프 156~177행
- 원인:
  - 격리 파일을 tar에 추가하는 도중 강제 종료되면 세그먼트가 본문 중간에서 끊긴다.
  - 재작성 루프가 그 항목을 건너뛰면 tar 0.4.46의 `skip()`이 `other("unexpected EOF during skip")`를 낸다(`archive.rs:567`). 이 오류는 kind `Other`이고 `raw_os_error`가 없다. 판정 문자열에도 없어 `PC_IO`로 분류된다.
  - GNU LongName(100바이트 초과 경로, Android/data에서 흔함)의 이름 블록 뒤에서 끊긴 경우도 같다. `"members found describing a future member"`가 나와 inspect 단계에서 이미 `PC_IO`가 된다(추정).
- 영향: 재개할 때마다 `repair_segments`가 Err를 내고(`runner.rs` 재개 시작), `omissions::clean`도 Err를 낸다. 이어받을 수 없다. 이전 코드는 모든 오류에서 `break`해서 이 경우를 처리했다.
- 권장:
  - tar 구조 오류 문구(`unexpected EOF during skip`, `members found describing`, `size overflow`, long name/sparse 관련)를 `CORRUPT`로 분류한다.
  - 또는 재작성 루프도 inspect와 같은 길이 검사를 써서 미리 끊는다.
  - 본문 중간에서 끊긴 세그먼트에 대한 회귀 테스트를 추가한다.

**R2. 받기에 실패했던 파일이 폰에서 사라지면 "손실 기록"이 영구히 붙어 항목이 완결되지 않는다 — 확인함 (M8 수정의 회귀, 빈도는 추정)**
- 위치: `backup/runner.rs` `previous_entries`(628~632행), 손실 기록 루프(736~750행)
- 원인: `previous_entries`에 sha256이 없는 오류 영수증(실패한 pull)까지 들어간다. 그 파일이 재개 전에 폰에서 지워지면, PC에 사본이 원래 없었는데도 "검증하지 못한 이전 백업 파일을 원본에서도 다시 수집하지 못했습니다"가 매 재개마다 붙는다.
- 재현: Android/data나 fs-rest에서 받는 도중 사라지거나 바뀐 캐시 파일이 있는 경우. 1회차는 오류 영수증이 남고, 2회차에는 원본이 없다.
- 변형: contacts의 remote에 연락처 수가 들어가 있어, 재수집할 때 수가 바뀌면 같은 경로로 손실 기록이 남는다(`contacts.rs:292`). 받지 못한 채 삭제된 앱의 APK도 같다.
- 영향: 새 백업이 `complete=true`에 도달하지 못할 수 있다.
- 권장: 손실 기록은 sha256이 있던, 즉 실제 PC 사본이 있던 영수증에만 남긴다.

### 중간

**R3. 내용은 같고 mtime만 바뀐 파일은 원본 속성 영수증이 영구 미완결로 남는다 — 확인함 (빈도는 추정)**
- 위치: `runner.rs` 765~768행(SHA가 같으면 예전 속성 복구), 116~120행(`old.backup_sha256.is_none()`이면 예전 속성과 사본의 일치를 확인하지 않고 유지)
- 재현:
  1. 1회차가 Partial로 끝난다. 파일 F는 속성 A1(mtime T1)로 연결된다.
  2. 재개 전에 F의 내용은 그대로이고 mtime만 T2로 바뀐다.
  3. 2회차: F를 다시 받아도 SHA가 같아서 A1이 복구된다. 그러면 `associate`에서 T1과 T2가 달라 불일치가 된다.
  4. 3회차 이후: 116행 조건이 다시 A1을 고르고 같은 불일치를 반복한다.
- 영향: `complete()`가 영구 false가 된다. 사본 내용은 정상이다.
- 권장: 두 조건 모두에 "예전 속성의 size·mtime이 해당 사본 영수증과 일치"를 추가한다.

**R4. 복사 후 변경 검사가 항목 전체 시간을 구간으로 잡는다 — 확인함 (수렴 여부는 추정)**
- 위치: `runner.rs` 170~216행
- 원인: 항목 시작 전 stat과 항목 전체를 받은 뒤의 재stat을 비교한다. 그래서 자기 사본을 받은 뒤에 바뀐 파일도 "복사 중 원본 파일이 변경됐습니다"로 잡혀 항목이 Partial이 된다.
- 영향: 몇 분마다 같은 크기로 다시 쓰이는 app-data 파일(SQLite, 로그)이 있으면 1회차는 거의 확실히 Partial이 된다.
  - 재개에서는 바뀐 파일만 다시 받으므로 구간이 짧아져 대개 수렴할 것이다.
  - 계속 쓰이는 파일이 있으면 Partial이 반복될 수 있다.
- 권장: 파일별 pull 직후 그 파일만 다시 stat하거나, "pull 시작 이후 변경"만 오류로 본다. 실제 Android/data 백업 결과를 보고 판단해도 된다.

**R5. 문자 앱 준비 중 [중단]이 확인 없이 실행되고, Rust 준비 작업은 계속 돈다 — 확인함**
- 위치: `RunProgressView.svelte` 16~18행 `requestAbort`, `wizard.svelte.ts:1468` `api.smsiePrepare`(`track`으로 감싸지 않아 `busy`에 포함되지 않음), `backup/mod.rs` smsie_prepare(전역 작업 잠금, 취소 수단 없음)
- 재현: 문자 내보내기 창에서 앱 준비(APK 다운로드·설치) 중 [중단] → 곧바로 [이어서].
- 영향:
  - 확인 창 없이 바로 중단된다.
  - 다시 시작한 `backup_run`이 잠금을 얻지 못해 "다른 기기 변경 작업이 진행 중입니다"로 실패한다.
  - 중단한 준비 작업이 뒤늦게 폰에서 내보내기 앱을 연다.
  - 다시 시도하면 회복된다.
- 권장: 준비 호출을 `track`으로 감싸 `busy`에 포함한다. 그러면 확인 창이 뜨고, 진행 중 표시와도 맞는다.

**R6. 보관한 `.damaged` 세그먼트에 나중에 제외된 앱 데이터가 남는다 — 확인함 (정책 위반)**
- 위치: `recovery.rs` 135행(그 시점의 `omitted_apps`만 확인), `omissions.rs`(`recovery/`를 정리하지 않음)
- 재현: 재개 시작 시 손상 세그먼트를 보관하고, 같은 실행 끝(또는 이후)에 그 세그먼트에 들어 있던 패키지가 제외된다.
- 영향: "제외 앱 데이터는 PC에서 삭제"라는 승인 정책을 지키지 못한다. 반대로 제외 앱이 하나라도 있으면, 관련 없는 손상 세그먼트도 보관하지 않는다.
- 권장: `omissions::clean`이 `recovery/*.damaged`도 검사해 해당 앱이 들어 있으면 지운다.

**R7. [초기화 전 필수] exec 스트림 하나가 실패하면 이후 복원 작업이 모두 실패한다 — 확인함 (adbd 동작은 추정)**
- 위치: `vendor/adb_client/.../commands/shell.rs` 108~112행(어떤 오류든 `sync_broken` 설정), `restore.rs`의 stage 정리와 `abandon_install`, `backup/mod.rs`(복원 전체를 `with_first_device` 하나에서 실행)
- 재현:
  - 기기 쪽 tar나 `pm install-write`가 입력 도중 먼저 끝나 CLSE를 보낸 경우.
  - 또는 PC 쪽 tar 생성이 실패해 기기가 입력을 기다리다 300초 시간 초과가 나는 경우.
- 영향:
  - 이후의 `rm -rf` stage 정리, `pm install-abandon`, 나머지 복원 항목이 모두 즉시 실패한다.
  - 폰에 임시 폴더와 설치 세션이 남는다.
  - 원래의 기기 출력(tar 오류 등)이 덮인다.
  - 백업에는 영향이 없다.
- 권장: 상대가 정상적으로 닫은 경우는 끊어진 연결로 표시하지 않는다. 또는 `run_restore` 안에서 재연결한 뒤 정리한다.

### 낮음

- **원본 속성** (`source_metadata.rs`, `runner.rs`)
  - 단일 경로 3회 조기 중단이 실행 전체의 cancel을 켠다(`source_metadata.rs:206-208`). 사용자에게는 "사용자가 작업을 취소했습니다"로 보여 실제 원인이 묻힌다. 사유를 따로 표시해야 한다(확인함).
  - 사용자 취소나 NO_SPACE이면 복사 후 변경 검사를 건너뛴다. 이번 회차 파일은 다음 재개에서 재사용되므로 끝내 검사되지 않는다(확인함).
  - 소수초·ctime·inode만 바뀐 경우 한 번 Partial이 되지만, 재사용 기준이 size+mtime 초 단위라 다시 받지 않고 Done이 된다(확인함).
  - 항목 끝 `publish(...)?`가 실패하면 실행 전체가 Err로 끝난다(`runner.rs:776`, 확인함).
  - 예전 sidecar가 정리되지 않는다(확인함).
  - enrich는 취소할 수 없다(`mod.rs:433`, 개발용, 확인함).
- **ADB와 walker** (`list.rs`, `adb_session.rs`, `shell.rs`, `install.rs`)
  - 이름 하나가 UTF-8이 아니면 그 폴더 전체 목록을 버린다(`list.rs:150`). 결정적이라 재개해도 그 항목이 계속 Partial이다. 지난 백업에서 같은 폰의 LIST는 이 오류 없이 통과했다. 권장: DONE까지 읽고 해당 이름만 개별 오류로 남긴다(확인함).
  - 중단 비우기 중 WRTE에 OKAY를 보내 stale CLSE가 하나 더 생긴다. OPEN 루프가 걸러 내므로 무해하다(추정).
  - `begin_sync_batch`가 실패해도 cancel을 설정하지 않는다(`walker.rs:79-87`, `puller.rs:148-155`, 확인함).
  - install 응답 처리가 ID 검사와 close를 생략한다. 호출부 Err로 연결을 버리므로 영향은 작다(확인함).
- **복구와 정리** (`omissions.rs`, `verify.rs`, `recovery.rs`, `quarantine.rs`)
  - M9의 대소문자 불일치를 만나면 폴더 삭제만 건너뛰면 되는데 Err를 반환해 `clean`이 매번 실패한다(`omissions.rs:188`, 드묾, 확인함).
  - 취소가 아닌 일시적 PC 읽기 오류로 재개 검증이 실패하면 여전히 Partial로 저장된다. 그러면 contacts/settings를 폰에서 다시 수집한다. [초기화 전 필수](확인함)
  - 복구 사후 검사와 omissions 경로의 `quarantine_hashes`는 새 `CancelFlag`를 써서 취소와 진행 표시가 없다(확인함).
  - `clean_temporaries`가 재개마다 트리 전체를 순회한다(확인함).
  - 세그먼트를 재작성한 뒤 manifest 저장 전에 크래시가 나면, 빠진 영수증이 선택하지 않은 항목에서 표시되지 않는다(확인함).
  - 512MB 회전으로 닫힌 세그먼트의 고아 항목이 남는다(확인함).
- **GUI** (`wizard.svelte.ts`, `architecture.test.mjs`)
  - 백그라운드 감지가 성공하면 `manualCheckError`를 지운다. 그래서 [확인하고 진행]의 수집 실패 사유가 5초 안에 사라질 수 있다(확인함).
  - 백업만 실행으로 들어와 일반 기록 재개가 거부되면 `opts.backupOnly=true`가 남은 채 warning 화면에 머문다. 안전 문제는 없다(확인함).
  - `architecture.test.mjs:880` 테스트는 두 번째 `ensureOptions`가 조기 반환해 `pendingJournal` 단독 차단을 실제로 검증하지 않는다(확인함).

---

## 2. 이전 지적 반영 상태

| 이전 지적 | 상태 |
|---|---|
| H1 enrich가 완결·복원 게이트를 막음 | **수정됨.** `required()`가 사후 보정 영수증을 완결·복원·요약에서 제외하고, 미완결 enrichment는 교체할 수 있다. CLI가 receipt `complete=false`를 실패로 기록한다 |
| H2 I/O 오류와 손상 구분 | **부분.** 실제 디스크 I/O(raw_os_error 있음)가 손상으로 잘못 분류되는 경로는 없다. 반대로 tar 구조 오류 일부가 PC_IO로 분류된다(R1). 손상 원본 보관, 사후 검사 오류의 영수증 오염 방지, scratch `create_new`는 수정됨. 보관본의 정책 문제는 R6 |
| M1 재사용 사본의 원래 속성 | **부분.** SHA 기준으로 복구한다. mtime만 바뀐 경우 R3 |
| M2 선저장 stat 보존 | **수정됨.** size·mtime·종류가 같으면 유지한다. 해시 없음 분기는 R3 |
| M3 sidecar 누락·손상 교착 | **수정됨.** 새 관측으로 회복하고 limitations에 기록한다. 제외 정리는 계속 진행한다 |
| M4 결정적 실패 이분 폭증 | **수정됨.** 단일 경로 3회에서 중단한다. 사유 표시는 낮음 항목 |
| M5 관측 중 소멸 | **수정됨.** 단일 stat 오류 문구가 `": No such file or directory"`로 끝날 때만 `SOURCE_GONE`이다. 권한 거부와 구분된다. toybox·bionic 문구는 번역되지 않고, shell v1에서 stderr가 섞여도 `ends_with`가 맞다 |
| M6 TOCTOU | **부분.** 새로 받은 파일만 size·mtime 원문·ctime 원문·inode를 다시 stat한다. 구간이 넓다(R4) |
| M7 취소 시 Partial 저장 | **수정됨.** 저장 전에 반환한다. 일시적 읽기 오류 잔여는 낮음 항목 |
| M8 손실 영수증 | **반영됐으나 회귀**(R2) |
| M9 대소문자 삭제 | **수정됨.** canonical의 마지막 이름을 대소문자까지 비교한다. 불일치 시 Err는 낮음 항목 |
| M10 백업만 실행의 [← 이전] | **수정됨**(기기 화면으로 감) |
| M11 Esc 후 기록 덮어쓰기 | **수정됨.** 기기/경고 화면으로 돌아가고, 다시 들어오면 기록을 재조회한다 |
| M12 일반 기록 재개 시 동의 | **수정됨** |
| M13 빈 계획 | **수정됨.** 실행을 막고 버튼을 비활성화한다 |
| M14 smsie prepare/probe 순서 | **수정됨.** 준비가 끝난 뒤 감지하고, 다시 준비할 때 진행 중인 감지를 기다리며, 감지 성공 시 오류를 지운다 |
| M15 문자 창 중단 | **부분.** 버튼과 z-index 60 확인 창이 있다. 준비 중에는 확인 없이 중단되고 Rust 작업이 계속된다(R5) |
| M16 LIST 중단 | **수정됨.** incomplete 유지, CLSE 중단, ACK 소비, 잘못된 UTF-8은 명시 오류. 폴더 전체 누락은 낮음 항목 |
| M17 다중 프레임 테스트 | **수정됨.** 160KiB를 32KiB×6 프레임으로 보내고, QUIT 0회와 남은 프레임 없음을 확인한다. close 1024, OPEN 16, peer close 테스트도 있다. LIST 기기 단위·UTF-8·exec 테스트는 없다 |
| M18 shell/stat/push 종료 | **수정됨.** 공통 종료 경로를 쓰고, shell 실패는 재연결로 표시한다. `ensure_usb`가 다시 연결한다 |
| M19 exec 동시 읽기 | **수정됨.** 리더가 하나이고 detached 스레드가 없다. 입력 중 출력 ACK로 교착이 없다. 실패 시 broken 처리는 R7 |
| USB 페이로드 상한 | **수정됨.** 할당 전에 16MiB로 제한한다 |
| `send_and_expect_okay` ID/CLSE | **수정됨** |
| GUI N1 백업만 실행 기록 거부 | **수정됨.** `backup-notice`를 허용하고, 실제 `buildPlan` 저장·재로드 테스트가 있다 |
| GUI N2 게이트 예외 | **수정됨.** `.catch`에서 `failStep`을 부르고, `runGen++`로 이후 판정을 차단하며, 구독을 해제한다. 예외 테스트가 있다 |
| GUI N3 `journalReadBlocked` 반응성 | **수정됨**(`$state`) |
| 실패 모달 반복 표시 | **수정됨**(`failureSequence`) |

## 3. 문제없음 확인 (확인함)

- **원본 속성**
  - `stat -L`은 루트(`/sdcard`)에만 실제로 작용한다. walker가 심볼릭 링크를 건너뛰므로 트리 안의 링크 대상이 링크로 기록되지 않는다.
  - sidecar를 먼저 원자 저장하고 manifest를 나중에 저장하며, 예전 sidecar를 지우지 않아 크래시에도 안전하다.
  - signed/unsigned epoch를 처리하고, 이분 탐색 깊이가 7 이하이며, 기기 데이터에서 panic이 나는 경로가 없다.
- **크래시 일관성**
  - 손상 보관본(`recovery/<seg>-<ns>.damaged`)은 검증·복원 대상(`quarantine/*.tar`)에 들어가지 않는다.
  - 복구 임시 파일은 `.xvolte-PID-N.tmp`라 `clean_temporaries`가 정리한다. `read_dir` 오류는 `continue`로 넘어간다.
  - 병렬 해시와 tar 해시의 256KiB 단위 취소(재개 검증 경로)가 있다.
- **ADB**
  - OPEN은 arg1이 다른 CLSE만 버린다.
  - RECV와 LIST 중단은 CLSE 후 ACK를 소비하고 상한이 있으며 실패하면 broken으로 표시한다.
  - 재연결은 `with_first_device`와 `ensure_usb`로 이어진다. 새 코드에 panic 경로가 없다.
- **GUI**
  - backupOnly 계획, 기록, 게이트, API 플래그 분리가 맞다. 기록 검사가 kind/wipe/manual/config까지 제한한다.
  - 새 필드(`Receipt.captureContext`)의 serde와 TS 타입이 일치한다.

## 4. 권장 순서 (새 백업 기준, 최종)

1. R1~R7은 수정됨. 새 백업을 시작해도 된다.
2. 3회 중단 사유 표시, 일시적 PC 읽기 오류 처리, 감지 오류 처리는 0-1절 기준 수정됨. 취소 사유 보존, 감지 오류 소유 판정, UTF-8 폴더 누락은 0-2절 기준 수정됨.
3. 나머지 1절 낮음 항목(트리 전체 순회, 예전 sidecar 미정리, 고아 tar 항목 등)은 초기화 전에 다시 판단한다.
