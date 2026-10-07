# adb_client 3.2.3 — 로컬 패치

2026-10-06 RECV FAIL 종료: 재개 백업에서 Lightroom 파일의 권한 거부 뒤 QUIT를 보내던 종료 처리 때문에 OKAY/CLSE 순서 오류가 나고 전체 배치가 중단됐다. AOSP daemon/file_sync_service.cpp의 recv_impl은 open/read 실패 시 FAIL을 보내고 SYNC 서비스를 종료한다. 명시적인 SYNC RECV FAIL에 한해 새 QUIT를 보내지 않고 같은 스트림 ID의 원격 CLSE를 소비·확인한다. 대기는 10초·최대5프레임으로 제한하고, 다른 스트림/무응답/손상/예상 밖 데이터는 기존 SYNC_BATCH_BROKEN으로 남겨 후속 전송을 막는다. 분할 FAIL·QUIT 미전송·다음 정상 파일 수신·잘못된/누락된 종료 응답 회귀 2개 포함 vendor USB 테스트 35개 통과. 권한 자체는 바꾸지 않으며 실패 파일은 계속 누락 기록으로 남는다. 수정 release로 같은 폰의 해당 디렉터리를 읽기 전용 샘플 조회한 결과 일반 Permission denied만 반환하고 SYNC_BATCH_BROKEN/종료 순서 오류는 없었다. 전체 재개 백업의 완료 판정은 별도다.

2026-10-06 읽기 SYNC 배치: ADBDeviceExt begin_sync_batch/end_sync_batch 기본 메서드(미지원 transport는 false)를 추가. 직접 USB/TCP는 LIST/RECV의 성공한 세션을 재사용하며 배치 종료 때만 QUIT/CLSE/종료 드레인을 수행한다. 다른 서비스 열기 전에 캐시 세션을 닫는다. RECV 헤더+경로를 같은 WRTE로 보내고 정상 실패는 세션을 폐기한다. 종료 실패는 SYNC_BATCH_BROKEN 마커로 후속 요청을 차단한다. 조각난/빈 파일·동일 세션의 연속 LIST/RECV·다른 서비스 전환·저장 실패·종료 실패를 가짜 transport로 확인했다. 실기기 동일 사진 샘플 8개 66,710,218바이트씩 ABBA 4회에서 PC 해시 일치, 세션별 30.10/30.63 MiB/s·재사용 34.77/34.22 MiB/s를 관찰했다. 같은 버퍼·동기화 조건에서 세션 재사용만 비교한 작은 표본이며 전체 백업 처리량이나 USB 대역폭 측정은 아니다.

2026-10-06 재연결 핸드셰이크: 기존 백업 프로세스를 DCIM manifest 저장 후 강제 종료한 직후, 새 프로세스의 첫 연결 응답이 CLSE여서 실패했다. 별도 재시도는 성공했다. 이전 스트림의 늦은 응답이라는 해석이 유력하지만 패킷 추적이 없어 정확한 생성 경로를 단정하지 않는다. 연결 전 10ms idle 드레인만으로는 CNXN 뒤 도착하는 오래된 프레임을 막지 못한다. AOSP docs/dev/protocol.md의 연결 전 스트림 메시지 무시 규칙에 따라 CNXN/AUTH/STLS 대기 중 OPEN/OKAY/WRTE/CLSE 전체 프레임을 승인·서비스 재실행 없이 버린다. 회당 최대64개·초기/서명 대기30초·공개 키 승인10초로 제한하고 손상 프레임·무응답·반복 초과는 오류로 유지한다. 정상 연결 응답을 받은 뒤 후속 프레임은 비우지 않는다. 늦은 종료·인증 서명/공개 키·TLS·시간 상한·프레임 반복·무결성 실패 5개 회귀 테스트 및 전체 vendor USB 단위 테스트 33개 통과. 수정 코드의 실기기 재연결 재현은 현재 백업 종료 후 별도 확인한다.

원본: crates.io `adb_client` 3.2.3 (https://github.com/cocool97/adb_client, MIT — 원본 패키지에 LICENSE 파일이 없어
Cargo.toml의 `license = "MIT"`와 authors로 LICENSE를 작성했다). `src/`·`Cargo.toml`·`README.md`는 원본 그대로 복사했고,
아래 변경만 더했다. 바뀐 곳은 모두 `[xvolte patch]` 주석으로 표시했다. `src-tauri/Cargo.toml`의 `[patch.crates-io]`로 연결한다.

목적: 원본은 USB 읽기 상한이 `u64::MAX`초, 서버(TCP 5037) 소켓은 읽기·쓰기 상한이 없어 기기가 응답 없이 멈추면
작업 스레드가 USB 연결 잠금과 기기 변경 실행권을 앱 재시작까지 잡았다.

| 파일 | 변경 |
|---|---|
| `src/message_devices/adb_message_transport.rs` | USB 기본 읽기 상한 `u64::MAX`초 → 300초. 기본 쓰기 상한 2초 → 30초(느린 저장소로 adbd가 USB 읽기를 잠시 멈출 때 스트림이 끊기지 않게) |
| `src/server/tcp_server_transport.rs` | 서버 접속 `connect_timeout` 5초, 접속마다 읽기·쓰기 상한 300초. 장시간 유휴가 정상인 읽기용 `clear_read_timeout()` 추가 |
| `src/server_device/adb_server_device_commands.rs` | 양방향 세션(exec/shell)은 stdin 전송 중 읽기 쪽이 유휴 상태이므로 읽기 상한 해제(쓰기 상한 유지) |
| `src/server/commands/wait_for_device.rs`, `src/server/commands/devices.rs` | wait-for·track-devices는 기다리는 것이 목적이므로 읽기 상한 해제 |

300초 근거: 출력 없이 오래 도는 명령(boot_patch.sh, 저장소 du 스크립트, 대용량 앱 `pm install-commit`의 dexopt)보다 길다.
USB exec stdin 전송 중에는 기기가 WRTE마다 OKAY를 보내 읽기 스레드가 유휴 상태가 되지 않는다.
시간 초과는 rusb `Timeout` / io `TimedOut`·`WouldBlock` 오류로 올라오며 EOF나 성공으로 바뀌지 않는다.

업그레이드 시: 원본 새 버전으로 교체한 뒤 위 변경을 다시 적용하고 이 표를 갱신한다.

### 추가 변경 (2026-10-05)

| 파일 | 변경 |
|---|---|
| `src/error.rs`, `src/lib.rs` | `UNAUTHORIZED_MARKER` 상수 추가·공개 |
| `src/message_devices/adb_message_device.rs` | 공개 키를 보낸 뒤 10초 안에 CNXN이 없으면(폰에 "USB 디버깅 허용" 창이 떠 있음) 시간 초과 오류를 `ADBRequestFailed(UNAUTHORIZED_MARKER)`로 바꿔 돌려준다 — 앱이 일반 연결 실패와 구분해 허용 안내를 보여 준다 |

### 추가 변경 (2026-10-06 백업 세션)

| 파일 | 변경 |
|---|---|
| `src/message_devices/commands/install.rs` | 설치 응답의 빈 OKAY를 결과로 판정하지 않고 WRTE 출력 조각을 모아 정확한 `Success\n`을 확인한다. WRTE마다 OKAY를 보내고 종료 CLSE까지 읽어 응답한다. 실패 출력도 종료까지 소비하며 출력 누락·연결 끊김은 성공으로 바꾸지 않는다. |

XQ-DQ44 / Android 15 / 67.2.A.3.178에서 SMS Import/Export 설치 후 빈 오류를 반환하고 다음 연결에 WRTE가 남는 문제를 관찰했다. 이후 설치 조회·권한 준비는 성공했다. 분할 성공 출력·빈 OKAY·실패·잘린 응답 모의 테스트 2개 통과. 수정한 설치 전송 코드 자체의 실기기 재검증은 대기 중이다.

알려진 한계: 서버 모드 양방향 세션(exec/shell)은 읽기 상한을 해제하므로, 복원(`backup/restore.rs`)이 기기 출력 대기
시간 초과(300초)로 포기한 뒤에도 그 세션의 읽기 스레드와 소켓은 기기가 멈춰 있으면 계속 남을 수 있다
(원본 크레이트와 같은 동작이며, 앱 실행권·USB 잠금은 잡지 않는다).

2026-10-06 USB bulk 전송: endpoint.max_packet_size는 USB 패킷 단위이며 libusb 요청 크기 제한이 아니다. 페이로드 읽기/쓰기 요청을 최대 64 KiB로 묶되 현재 ADB 프레임 끝을 넘지 않는다. 짧은 전송은 반복 처리하고 0바이트 읽기/쓰기는 오류로 종료한다. ZLP는 전체 쓰기 크기가 실제 max_packet_size의 배수일 때 유지한다. bulk_tests의 짧은 읽기·프레임 경계 및 0바이트 읽기 테스트 2개 통과.
2026-10-06 실기기 재개 확인 중 추가 수정: USB 연결을 새로 잡고 CNXN을 보내기 전에, 직전 프로세스 중단으로 남은 bulk 응답을 10ms idle 기준·최대64회로 비운다. 새 연결 응답을 받은 뒤에는 드레인하지 않는다. transport 종료에 stream ID 없는 CLSE(0,0)를 보내던 동작을 제거하고 인터페이스만 해제한다. 이것은 스트림별 CLSE와 구별된다. 실기기에서 연결 재개를 검증한다.
2026-10-06 스트림 응답: WRTE는 OKAY, CLSE는 CLSE로 승인하고 OKAY에는 추가 승인하지 않는다. push 마지막 SYNC OKAY를 승인·확인하고, sync QUIT 뒤 CLSE를 소비·승인한다. 스트림 ID/ACK 및 실패 push 테스트 포함. USB ZLP는 정상 전송 경계일 수 있어 최대4회까지 허용하되 반복 무진행은 유한 오류로 종료한다.

2026-10-06 extended stat: Android의 긴 UID/GID가 공백 없이 출력되는 형식도 허용한다. stat 경로는 POSIX 작은따옴표로 감싸 날짜 뒤 중복 번호·공백·작은따옴표가 들어간 파일을 한 경로로 조회한다. 4 GiB 초과 크기와 실제 mtime 회귀 테스트를 추가했다.

2026-10-06 LIST 수신: 마지막 WRTE까지 즉시 OKAY로 확인하고 QUIT 전에 DONE의 16바이트 본문을 끝까지 소비한다(AOSP file_sync_service.cpp sync_dent). 응답이 여러 패킷에 나뉘어도 반복 수신하며 파일명 길이·무진행 상한 및 알 수 없는 응답 거부를 적용한다. 마지막 패킷을 확인하지 않으면 다음 응답을 보내지 않는 가짜 transport로 1바이트 분할부터 단일 패킷까지·종료 응답·잘린 목록을 검증한다. 폰이 꺼져 있으므로 실기기 재개 결과는 아직 검증하지 않았다.

2026-10-06 SYNC STAT도 응답 WRTE를 OKAY로 확인한다. STAT 접두사 및 본문 크기를 확인해 잘린 응답이 슬라이스 패닉을 발생시키지 않도록 한다. 정상·짧은·실패 응답 회귀 테스트 포함.

2026-10-06 재연결 시 XQ-DQ44에서 수천 개 LIST와 QUIT/CLSE 승인 완료를 관찰했다. 개발 빌드에서만 XVA_ADB_TRACE 환경변수가 있을 때 경로·내용 없는 SYNC 진행 진단을 최대 10,000줄 출력한다. 전체 파일 복사와 최종 무결성 검증은 별도로 확인한다.
2026-10-06 앱 데이터 실기기 백업: 21,933개 폴더 열거 후 RECV의 짧은 페이로드에서 기존 recv_file의 len-8 슬라이스가 패닉을 일으켰다. DATA/DONE/FAIL의 8바이트 헤더를 ADB 페이로드 경계와 독립적으로 읽는 스트리밍 파서로 교체한다. 파일 내용의 DONE 바이트를 종료로 오인하지 않고 EOF·잘린 헤더·FAIL·반복 빈 패킷은 오류로 반환한다. STAT 응답 승인은 stat_with_explicit_ids에서 한 번만 처리하며 pull의 중복 OKAY를 제거한다. 1·3·7·8·17바이트 분할, 빈 파일·빈 WRTE, 종료 유사 바이너리 내용, 실패·잘린 응답 회귀 테스트 포함. 수정 후 실제 파일 복사 검증은 재개 실행에서 확인한다.
RECV 수정 후 실기기 재개: Documents/Music/Movies/Download의 71개 파일 565,557,635바이트를 정상 수신했고 별도 PC 크기·SHA256 대조가 모두 일치했다. 전체 백업 완료를 의미하지 않으며 Pictures/Android/data/DCIM/fs-rest는 진행 중이다.

2026-10-06 follow-up: bound OPEN/stream cleanup to 10 seconds; mark failed OPEN as reconnect-required; ignore bounded stale CLSE; remember peer close; abort RECV with ADB CLSE and drain/ACK pending DATA after a destination write error instead of injecting SYNC QUIT. Multi-frame >64KiB writer-failure, failed OPEN and stale CLSE regression tests.

2026-10-06 재검사: LIST incomplete를 RECV와 동일하게 CLSE로 중단. stat/push 오류 종료 공통화, shell 실패 reconnect. exec는 단일 reader로 WRTE/OKAY/CLSE 처리하여 detached output reader와 세션 Drop의 concurrent read 제거. Drop에서 임의 프레임을 소비하지 않는다. USB 할당 최대 16MiB. 실제 160KiB fragmented writer 실패, pending WRTE drain, peer close, 1024/16 응답 상한 회귀 검사.

2026-10-06 R7: exec/shell 오류 후 CLSE/abort 정리가 성공한 stream은 sync_broken으로 표시하지 않는다. 정리 실패 때만 reconnect가 필요하다. premature close에는 기기 마지막 출력이 포함된다. 100KB exec 입력을 두 ACK chunk로 전송하며 출력/CLSE를 처리하는 테스트, early close 및 PC stdin 오류 뒤 다음 shell 서비스 성공 테스트를 추가했다. vendor 39 unit + 4 doctest 통과.

2026-10-06 LIST UTF-8: UTF-8이 아닌 항목 이름은 오류로 LIST 전체를 중단하지 않고 `ADBListItemType::InvalidName`(표시용 lossy 이름, 경로로 사용 금지)으로 보고한 뒤 DONE까지 계속 읽는다. 직접 연결(message_devices)과 서버 경유(server_device) 양쪽에 적용했다. 앱 walker는 해당 이름 하나만 오류로 남기고 같은 폴더의 나머지를 계속 열거한다. 5바이트 분할 응답에서 잘못된 이름 + 정상 파일이 모두 반환되고 모든 WRTE가 승인되는 회귀 테스트 포함. vendor 40 unit + 4 doctest 통과.

2026-10-07 LIS2: 직접 연결(message_devices)은 기기 CNXN 배너의 features에 `ls_v2`가 있을 때 LIST 대신 LIS2를 쓴다. DNT2/DONE 뒤 72바이트 dent_v2(error, dev, ino, mode, nlink, uid, gid, size u64, atime/mtime/ctime i64, namelen)를 경계 독립으로 읽는다. `ADBListItem.size`는 u64로 넓혔고(v1과 서버 경유는 하위 32비트), mtime은 u32로 맞춘다. LIST v1이 4GiB를 넘는 크기를 잘라 보내서 큰 파일을 매번 다시 받던 문제를 해결했다. 23GB 크기·조각난 응답·배너 기능 판정 회귀 테스트 포함. vendor 42 unit + 4 doctest 통과. 실기기(XQ-DQ44, Android 15) 갱신 백업에서 DCIM 4GiB 초과 13개를 다시 받지 않음을 확인했다.
