# Backup metadata and review follow-up — 2026-10-06

Repo: C:\Users\korjw\Downloads\sony-volte_v0.1-beta11_20260821\xperia-volte-activator

사용자가 기존 Claude 리뷰의 실제 결함 전부 수정과, 소유자/권한/파일 및 폴더/시간 속성 보존 알고리즘까지 재검사를 요청했다. 코드 작성 권한 있음. 리뷰어는 수정/기기 통신/194GB 재해시 없이 읽기 전용 리뷰 및 오프라인 테스트만 수행한다. git 작업 트리에는 이전 실기기 개선도 섞여 있으므로 HEAD diff만 보지 말고 현재 코드를 직접 읽어 연결을 추적한다.

## 원본 속성 알고리즘

src-tauri/src/backup/source_metadata.rs 신규. 신규 Files 항목(외부 앱 데이터/APK/사진/기타 파일)은 원본 열거 → 파일+빈 디렉터리 포함 stat batch → 내용 해시 기반 이름의 JSON sidecar + manifest 선행 저장 → 기존 전송 중 hash/mtime 보존 → 원본 size/mtime와 내용 hash 연계 → 최종 영수증 저장이다. snapshot 자체 해시도 manifest에 있고 PC verify/restore gate가 검사한다. 강제 종료 전에 원본 속성을 먼저 내구 저장하며 재사용하는 정상 기존 파일의 원본 속성은 덮어쓰지 않는다. 배치는 128경로/24KiB 이내이며 filename을 stat 출력에 넣지 않아 따옴표/탭/개행 때문에 대응이 틀어지지 않는다. batch exit=0와 줄 수 일치 때만 대응시키며 실패 배치는 이분해서 개별 오류를 남긴다.

저장: 전체 st_mode, UID/GID와 이름, size, inode, hard-links, blocks, atime/mtime/ctime, 실제 제공될 때 birth time, SELinux context, 시각 원시 문자열(소수초/시간대), 내용 hash 연계. ctime은 생성 시각이 아니다. Toybox %W/%w가 미지원이면 null. ACL/임의 xattr/심볼릭 링크 대상은 지원하지 않는다고 기록한다. /sdcard FUSE 합성값 한계와 비원자적 snapshot 한계도 저장한다. 이전 UID/SELinux를 초기화 후 무조건 재적용하는 구현은 넣지 않았다. 원본 속성 기록 보존과 속성 재적용/전체 앱 복원 보장은 구분해야 한다. 현재 restore.rs는 기존 안전한 내용 복원/mtime 방식을 유지한다. 빈 폴더는 메타데이터에 보존되지만 기존 내용 복원 루틴에서 생성하는 기능과는 구분된다.

기존 백업 보정용 개발 CLI backup_metadata_enrich(serial,dir)는 같은 기기 해시를 확인하고 읽기 전용 stat을 추가한다. after-copy-enrichment로 표시하며 과거 접근 시각/바뀐 소유자/삭제된 빈 폴더를 소급 복구했다고 주장하지 않는다. 현재 작업 중 기기 통신으로 이 보정을 실행하지 않았고 현재 D:194GB 사본이 새 메타데이터 형식으로 변환됐다고 보고하지 않는다.

## 기존 피드백 대응

1. GUI: 명시적 '백업만 실행' → backupOnly 저장 옵션, backup 하나만 실행, 독립 backupLive. REAL_STEPS=false 및 Cargo default=[] 유지. 일반 시작은 backupOnly 해제. patch/root/restore/SIM 설정 숨김, 같은 폰의 journal을 먼저 조회. backupOnly journal의 추가 엔진은 disabled여도 거부. 완료 화면 실제/목업 구분도 반영.
2-1. smsie_probe 명령은 목록만 읽고 collect/ZIP 수신하지 않음. GUI 확인은 진행 중 probe를 기다린 뒤 collect. ApiResult로 잠금/USB 등 실제 오류 표시. 사용자 내보내기 성공 확인은 유지.
2-2. 재개 해시는 4개 bounded worker, 파일 안에서도 1MiB마다 취소/byte progress. UI callback은 원래 호출자 스레드. 파괴 전 backup_manifest_check도 동일 progress/runId 기반 cancel. 파괴 전 강한 전수 hash를 삭제하지 않았으며 일반 존재/mtime만으로 대체하지 않음.
2-3. 본 백업 실패/메타데이터 실패일 때 export 먼저 안 열고, 이미 Done이면 재내보내기 없음. export Esc/backdrop 닫기 차단. 수동 확인 문구 수정.
3-1. omissions::clean은 affected-empty를 삭제용 루트 검사보다 먼저 반환.
3-2. OPEN: 10초/16응답 stale CLSE handling; 실패 후 sync_broken으로 연쇄 OPEN 차단.
3-3. PC writer 실패시 recv_incomplete 유지, ADB CLSE abort/남은 WRTE ACK-drain 후 새 stream. DATA 중간에 SYNC QUIT을 보내지 않음. >64KiB multi-frame 테스트.
3-4. peer_closed 기록, close_sync 10초/1024프레임 제한. 원래 I/O 오류 유지.
3-5. resume 중 APK 설치 경로 변화 검사. local filename에 설치 경로 hash를 포함해 새 base/split cohort를 충돌 없이 받음. 성공한 cohort로 활성 manifest 교체. 실패하면 이전 정상 사본 유지/Partial. base.apk 부재도 실패. 복원은 활성 manifest만 사용.
3-6a. recovery::repair_segments: 손상 헤더 탐지/extent 검사 → durable manifest hash에 맞는 정상 prefix payload만 원자 재작성 → 잃은 receipt는 Partial/re-pull. 다른 segment 정상 사본 유지. omissions의 영구 parse 오류 회복.
3-6b. clean_temporaries: 백업 manifest 확인, exact .xvolte-PID-SEQ.tmp만 제거, symlink/reparse 불추적, 명시적 manifest 이름 보존.
3-6c. SMS cleanup은 검증/확인/manifest 저장 후 해당 선택 export 경로만 rm -f, 바뀐 size/mtime는 보존. 다른 파일/폴더/contacts JSON 유지. restore_finish도 전체 rm-rf 없앰; restore import 파일은 사용자가 지울 수 있게 남김.
3-6d. failure modal key의 log length 제거, 기존 Next/omissions acknowledgement 유지.

정책: 권한 오류 앱들의 앱 데이터 전체 PC 삭제 및 이 백업의 영구 제외는 사용자가 명시적으로 선택한 동작이다. APK/폰 원본 유지. 여기에 다시 승인을 요구할 필요 없다.

## 오프라인 검증

pnpm check: 오류 0/경고 0. pnpm test: 126개. pnpm build 통과.
Rust cargo test --features dev-cli: lib 284 passed/9 ignored(전체 293), integration 3 passed, 실패 0.
vendor adb_client: 34 unit + 4 doctests passed.
추가 회귀: original ctime/birth/nanoseconds/>4GiB; path quote/newline/missing stat batch; metadata tamper/empty-dir/enrichment no-pull; cancellable large-file hash; updated APK cohort; corrupt tar salvage/lost receipt; exact temp cleanup; standalone plan/journal isolation; SMS probe/confirm race/real backend errors/partial main/no repeat export.

현재 일부 최종 수정(journal 초기 조회/UI 뒤로 경로)에 대한 짧은 재검증을 진행할 수 있다. 발견사항은 파일/라인/재현 조건/위험도 순으로 보고하고, '확인함'은 현재 코드로 직접 확인한 것만 표시해 달라. 특히 metadata capture와 payload association, resume/omission/corrupt segment crash consistency, GUI mode/journal/API 연결, ADB OPEN/RECV/CLSE 종료 경로를 우선 검사해 달라. 검사가 끝나면 .plans/04-engine/claude-backup-re-review.md에 결과를 저장하고 이 탭에도 보고해 달라. 코드 수정이나 기기 통신은 하지 말 것.
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

## R1–R7 최종 수정 (마지막 직접 재검토 요청)

- R1: tar의 unexpected EOF during skip와 GNU longname의 미래 멤버 구조 오류를 구조 손상으로 분류한다. 잘린 payload/longname 재개 회귀 검증. 실제 PC I/O 오류는 계속 구분한다.
- R2: 사라진 원본의 손실 영수증은 Files/SmsIe에서 SHA가 있던 실제 이전 사본에만 적용한다. 처음부터 받지 못한 파일과 연락처 합성 경로는 영구 미완결을 만들지 않는다.
- R3: 이전 속성을 재사용할 때 SHA 외에 size/mtime도 원래 영수증과 대조한다. 내용은 같고 mtime만 바뀐 파일의 재개가 수렴하는 회귀 검증.
- R4: 항목 전체 후속 재-stat 제거. 위 M6 최종 정책을 따른다.
- R5: 문자 앱 준비도 track/busy로 감싸 준비가 끝날 때까지 재개를 막는다. 안내 중 중단은 확인 팝업을 거친다. 이미 보내진 준비 명령의 즉시 중단을 주장하지 않는다. 준비 중 중단/재개 회귀 테스트.
- R6: 이후 제외한 앱이 들어 있는 generated recovery/*.damaged 진단 보관본도 정리한다. 구조 손상으로 포함 여부를 판단할 수 없는 진단은 제외 정책 적용 때 정리한다. 다른 정상 진단 및 검증된 활성 복원 본문은 보존한다. PC I/O 오류는 삭제로 처리하지 않는다.
- R7: 실패한 exec/shell도 CLSE/abort 정리가 성공하면 연결을 손상으로 표시하지 않는다. 정리가 실패할 때만 reconnect가 필요하다. 조기 종료 오류에 마지막 기기 출력을 포함한다. 실제 100KB 입력/ACK/출력/CLSE와 조기 종료·PC 입력 오류 후 다음 작업 성공 회귀 2개.

최종 오프라인 검증: frontend 133 pass / svelte-check 0 errors·0 warnings / pnpm build 성공. Rust 전체 dev-cli lib 295 pass+9 ignored, integration 3 pass, doc 0, 실패 0. 기본 feature cargo check 성공. backup 부분집합 134 pass+3 ignored. vendor 39 unit+4 doctests pass. 기존 PC 백업은 삭제 완료이며 새 실기기 백업/복원은 이 수정 검증으로 대체됐다고 주장하지 않는다.

마지막 낮음 결함 보완: CancelFlag의 오류 사유를 사용자 취소와 구분하여 stat 형식/기기 통신 실패를 그대로 표시한다. PC 파일 읽기/공유 잠금 오류는 NotFound·실제 hash 불일치와 구분하여 재개 검증을 중단하고 manifest 저장/폰 재수집 전에 반환한다. Windows share_mode(0) 실제 잠금 회귀는 manifest 동일/no pull/잠금 해제 후 complete를 검증한다. tar 실제 I/O와 sidecar 읽기 오류도 중단한다. 성공한 SMS probe는 probe 자기 오류만 지우며 이전 collect 오류를 지우지 않는다. pendingJournal 회귀 테스트는 별도 wizard의 유효 plan으로 수정했다. 기기 통신은 수행하지 않았다.
