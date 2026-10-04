# adb_client 3.2.3 — 로컬 패치

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
