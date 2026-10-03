# 전체 코드 리뷰 — 2026-10-04

status: implemented (정적 리뷰·가짜 I/O 검증, 이번 작업에서 실기기 테스트 없음)

대상: feat/fastboot-unlock. main의 db516b7 백업 수정은 이 브랜치에 병합했다. main 작업 트리는 수정하지 않았다. 이전 fastboot 리뷰는 [fastboot.md](fastboot.md)에 있다.

## 범위와 수정

Tauri 명령·ADB 연결·fastboot·펌웨어 파서·백업/복원 엔진, 공통 타입/API, Wizard 상태 전이, 각 화면과 공통 위젯의 I/O·lifecycle 경계를 검토했다.

| 발견한 문제 | 수정한 동작 |
|---|---|
| 실제 계획 생성기가 mock 폴더에 있고 창 이벤트가 화면에서 직접 IPC를 사용 | 순수 domain/plan과 API transport로 이동. IPC·이벤트·창 관찰을 facade 안에서 처리 |
| IPC 오류 결과 타입·이벤트 구독 구현 중복 | ApiResult와 transport의 result/optional/subscribe로 공통화, BackendPort 주입 가능 |
| 파일/작업 공통 기능이 ADB 모듈에 결합 | app_paths·storage·tasks·device_io 분리 |
| ADB 연결 오류 시 함수 전체 재실행 | 쓰기 콜백은 한 번만 실행하고 실패한 연결을 버림. 서버 경로도 Sony 제조사 확인 후 실행 |
| 셸 명령의 실패 종료 코드가 빈 출력 성공으로 해석 | 공통 device_io::shell에서 종료 코드·오류 스트림 검사. 설정·연락처·SMS·ADB 조회에 적용 |
| 시간 초과된 질의마다 새 스레드가 계속 남음 | 동시에 살아 있는 질의 최대 4개. 종료 시 RAII로 자원 반환 |
| 백업/복원/SMS 명령 동시 실행으로 취소·파일/역할 상태 충돌 | 변이 작업 하나만 허용, 중복 호출은 오류. 실행권 획득 후 취소 플래그 초기화 |
| 빈 매니페스트·첫 항목 전 취소가 완결로 판정 | 선택 항목을 먼저 Pending으로 저장, 선택 0개와 오류가 있는 Done은 완결 불허 |
| 동일 초에 만든 백업 폴더 덮어쓰기 | create_dir와 충돌 시 번호 추가 |
| 완료 표시만 믿고 재개, 손상된 manifest를 새 기록으로 대체 | 유효한 manifest와 원본 모델/마스킹 시리얼 확인. 완료 항목도 다시 검증해 손상 시 재수집 |
| 일반 파일 처음 3개만 해시 검사 | 기록된 파일 전부를 64KiB 버퍼로 SHA-256 검사. 격리 파일도 해시/크기 검사 |
| 신규 설정 덤프에 해시 기록 없음 | FileEntry와 해시를 추가하고 원자 저장. 예전 artifact-only 기록은 존재 여부만 검사 |
| 로컬 경로 탈출·symlink, 잘못된 항목 id | 읽기·쓰기·삭제 전에 루트 경계 검사, 절대/부모/잘못된 경로 거부. 미지정·중복·알 수 없는 선택 거부 |
| 폴더의 추가 파일과 선택하지 않은 격리 파일까지 복원 | 선택한 매니페스트 파일만 복원. 격리 tar도 선택 파일의 일치하는 버전만 새 tar에 담아 전송 |
| 수정시각을 PC 파일에서 가져옴, 저장 실패 무시 | 기록된 원본 mtime 사용. 파일·격리 tar 디스크 저장 및 일반 파일 mtime 실패를 오류로 처리 |
| 실패한 APK 설치 세션 잔류 | commit 실패도 abandon 시도. 잘못된 session id는 쓰기 전 거부 |
| Rust 복원 failures를 로그만 남기고 단계 완료 | 하나라도 실패하면 단계 실패, 마무리 역할 원복 오류도 실패 |
| 마지막 파일 카운터만으로 백업 항목 완료 표시 | 항목 저장 후 done/partial/pending 이벤트와 결과 상태 사용. 이전 실패 항목을 다음 Done으로 완료시키지 않음 |
| SMS 준비 실패 중에도 완료 가능 | 준비 loading/failed에서 확인 버튼 차단, 오류 표시 및 준비 재시도 버튼 |
| 문자 백업 수집이 미선택 항목까지 추가 | 원래 선택된 manifest 항목만 병합. 알 수 없는 산출물은 Partial, 실패한 수집은 기기 산출물 유지 |
| 문자 복원 폴더의 임의 파일을 모두 전송 | 선택한 SmsIe 기록을 해시 검증하고 그 파일만 전송 |
| 문자 앱 원복 실패 시 원래 앱 정보를 잊음 | 실패는 Err로 반환하며 이전 역할 정보를 남겨 같은 프로세스에서 재시도 가능 |
| APK 캐시·릴리스 태그·다운로드 무제한 | 태그 경로 검증, APK 128MiB 상한·최소 ZIP 형식 확인·원자 저장, 캐시 형식 확인. 해시는 스트리밍 계산 |
| 펌웨어 HTTP Range 무시·잘못된 크기·압축 폭증 | 206와 정확한 Content-Range/본문 길이 요구. 읽기/압축 해제 크기 제한, 합계·오프셋 오버플로 및 ZIP64 확장 필드 경계 검사 |
| 비동기 onMount의 cleanup 미등록 | 동기 onMount에서 정리 함수 반환. 늦은 이벤트 등록/기기 조회도 해제 또는 무시 |
| 중단·기기 변경 후 늦은 응답이 새 상태를 덮음 | run generation, watcher generation, 펌웨어 요청 번호로 오래된 응답 배제 |
| 이벤트 등록 중 중단해도 실제 작업 시작 | 구독 직후 generation 재검사하고 리스너 해제 |
| 진행 기록 저장·보관 순서 역전, 임시 파일명 충돌 | snapshot을 직렬 큐로 저장/보관, 고유 임시 파일로 sync 후 교체. 디스크 JSON 구조 검사 후 재개 |
| 메인 스레드에서 진행 기록 디스크 I/O | 블로킹 작업으로 분리, 저장 입력은 8MiB 제한 |
| Windows 종료 보호 예약만 하고 적용 성공 반환 | 메인 창 스레드에서 적용·상태 확인 결과까지 기다림. 실패 시 절전 보호/ACTIVE 해제 |
| 경로를 지워도 여유 공간 loading 유지, 중복 TS alias | loading 초기화, SvelteKit 제공 alias 사용 |
| 스타일·개행 규칙 부재 | editorconfig/gitattributes 추가, Rust 전체 rustfmt, 런타임 Clippy 경고 제거 |

Windows 종료 보호의 창 생성 스레드 요구 및 NULL 버퍼 길이 조회 동작은 [Microsoft 공식 API 문서](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-shutdownblockreasonquery)를 확인했다.

## 검증

- Rust 단위 테스트: 일반 실행만 사용. live_* 및 ignored 테스트는 실행하지 않음.
- 프런트엔드: 실제 Svelte store/domain을 Vite로 컴파일하고 IPC를 메모리 가짜로 주입.
- HTTP Range: localhost 가짜 서버로 정상/200 응답/잘못된 범위/짧거나 긴 본문 검사. Sony 서버 다운로드는 실행하지 않음.
- 경로/해시/복원: 임시 디렉터리·FakeADBDevice로 네 번째 파일 손상, 누락된 격리 파일, 매니페스트 밖 파일, 선택하지 않은 항목/이전 tar 버전, 원본 mtime, 취소·재개 등을 검사.
- `pnpm test`: 21개 통과. SvelteKit 생성 파일을 공유하므로 테스트 파일은 순차 실행한다.
- `cargo test --lib`, `cargo test --lib --all-features`: 각각 98개 통과, live_* 5개 제외.
- `pnpm check`: 오류 0·경고 0. `pnpm build`: 정적 프로덕션 빌드 통과.
- `cargo fmt --all -- --check`, `cargo clippy --lib --all-features -- -D warnings`: 통과.

## 이번에 실제 테스트하지 않은 부분

이전 main 문서의 2026-10-03 실측은 그때의 코드에 대한 기록이다. 아래 변경의 실기기 검증을 대신하지 않는다.

1. Sony USB 재연결·복수 장치·ADB 서버 공존, shell exit code/권한 오류의 실제 기기 출력.
2. 백업/복원 중 연결 해제·저장 공간 부족·취소·재시도, 대용량 파일/NTFS 링크·긴 경로의 실제 동작 및 처리 시간.
3. Android toybox tar 호환, 원본 mtime·격리 이름 복원, split APK 설치 실패 후 abandon, 연락처 가져오기.
4. SMS Import/Export 설치·권한·내보내기·가져오기, 기본 문자 앱 전환 및 원복 실패 후 재시도. 실제 문자·MMS·통화 기록의 내용/개수는 가짜 I/O 테스트가 보증하지 않음.
5. Sony 서버/CDN의 Range 응답, 실제 SIN/ZIP64·현재 펌웨어 지문과 추출 이미지의 부팅 호환.
6. fastboot USB claim/전송·언락 후 초기화/재부팅·슬롯별 이미지 쓰기. 기존 fastboot 문서의 미검증 항목도 계속 적용.
7. 실제 Tauri 창 닫기/화면 이탈과 Windows 로그아웃·재시작 중 보호 적용·진행 기록 보존. 종료/재시작 실험은 하지 않음.

## 현재 남아 있는 한계

- 순정 부트 체인×슬롯 증거와 AVB 검증이 없으므로 실제 리락은 차단한다.
- REAL_STEPS 기본값은 모두 false, fastboot 쓰기는 Cargo fastboot-write 기능도 필요하다. 루팅·EFS·전체 펌웨어 기록 등 나머지 실행 단계는 아직 시뮬레이션이다.
- 질의 timeout은 하위 USB/HTTP I/O를 강제 취소하지 않는다. 살아 있는 스레드를 제한하며, 뒤늦은 응답은 화면에 적용하지 않는다. 단일 파일 전송/복원 도중 즉시 취소도 아직 보장하지 않는다.
- 이전 SMS 기본 앱 정보는 프로세스 메모리에 있다. 앱 강제 종료나 다른 기기로 전환하면 원복을 자동 보장할 수 없다. 기기별 영속 상태는 별도 구현이 필요하다.
- 신규 덤프는 해시가 있지만 예전 artifact-only 덤프는 해시가 없다. 기존 파일 내용의 동일성을 사후에 증명할 수 없다.
- 백업 재개의 기기 대조는 기존 모델·마스킹 시리얼을 사용한다. 같은 접두사의 기기까지 암호학적으로 구분하지는 못한다.
- SHA-256은 백업 파일 손상을 검사하며 매니페스트의 진위를 인증하지 않는다. APK PK 헤더 확인도 서명 인증서 검증을 대체하지 않는다.
- Wizard는 화면·실행 상태 조정 역할을 함께 가진다. 순수 정책/공통 I/O는 분리했지만 모든 단계의 별도 실행 서비스 추출까지 완료한 구조는 아니다.
