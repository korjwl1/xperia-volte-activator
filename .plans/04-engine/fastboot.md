# fastboot 엔진 (M2) — 언락/리락·플래시 프로토콜 구현 설계

status: draft (사용자 승인 2026-10-03 — 실전 코드 작성, 실기기 테스트 금지, REAL_STEPS.fastboot 기본 꺼짐)

- 상위 정책: `tasks/plan.md` §2(기술 스택)·§3-3(리락 게이트)·§9-3(유한 처리)·§10-2(fastboot 게이트), M2 마일스톤
- 근거 조사: `tasks/research-unlock-firmware.md` A절 (언락 코드는 공식 페이지 수동 발급 — 앱은 `oem unlock 0x<code>` 실행만)
- 원본 CLI 계승: `fastboot oem unlock 0x{code}` · `oem lock` · `flash <partition>_a`+`_b` · `reboot` (src/adb.py)

## 범위

**구현 (In)**
1. `src-tauri/src/fastboot/` 모듈 — rusb 트랜스포트 + AOSP fastboot 프로토콜 상태머신(OKAY/FAIL/INFO/DATA)
2. 명령: getvar(읽기 전용) · `oem unlock 0x<code>` · `oem lock` · flash(download+DATA) · reboot — **전부 REAL_STEPS.fastboot 게이트 뒤**
3. FakeTransport(cfg(test)) 시나리오 테스트 — 실기기 없이 프로토콜 전 분기 검증
4. facade + wizard 언락/리락 단계 실전 연결(플래그 기본 꺼짐)
5. `.plans` 문서 갱신 (02-contracts fastboot 절 정밀화)

**제외 (Out)**
- 실기기 테스트 (rusb open 자체는 컴파일만 — 프로토콜 로직은 전부 Fake 검증)
- 리락 게이트 완전판(§3-3 ①순정 이미지 해시 ②플래시 이력 판정) — 플래시 이력 기록 구조만 깔고 판정은 M4(루팅) 후 완성. 이번엔 getvar 실측(unlocked·슬롯) 기반 최소 게이트
- 펌웨어 전체 플래시(fw-flash, M6 newflasher)·EFS·루팅 단계
- `flashing get_unlock_ability` **강제**하지 않음 — getvar에 포함해 정보만 수집(Sony는 OEM 토글이 설정 앱 연동, 실측 후 판정)

## 프로토콜 명세 (AOSP fastboot — 표준)

- 명령: 호스트→기기 ASCII bulk OUT(≤64B). 응답: bulk IN 4글자 status + 페이로드
- `OKAY`+value / `FAIL`+reason / `INFO`+text(종결 응답 전까지 복수 프레임 수집) / `DATA`+8자리 hex 크기
- DATA 흐름: `download:%08x` → DATA수신 → 정확히 그 크기 bulk OUT → 최종 OKAY/FAIL. 그 후 `flash:<part>`
- 주요 변수: `unlocked`(yes/no) · `current-slot` · `slot-successful:_a/_b` · `slot-retry-count:_a/_b` · `max-download-size` · `(bootloader) version-bootloader`
- 유한 처리(§9-3): INFO 무한 루프 방지(최대 256프레임), 응답 타임아웃(기본 10초, DATA 전송 중 제외), 다운로드 크기 상한(max-download-size와 1GiB 중 작은 값)

## 모듈 구조

```
src-tauri/src/fastboot/
├─ mod.rs          — tauri 명령(fastboot_getvar/unlock/lock/flash/reboot)·이벤트·REAL 게이트 문서화
├─ transport.rs    — FastbootTransport 트레이트 + RusbTransport(FF/42/03 클레임·bulk IN/OUT)
├─ protocol.rs     — 상태머신·명령 API(getvar/unlock/lock/flash/reboot)·응답 파싱
└─ (테스트) fake — FakeTransport: 시나리오(OKAY·FAIL·INFO 복수·DATA·타임아웃·바이러us 크기)
```

- RusbTransport: usbmode.rs 디스크립터 탐색 재사용 — Sony VID 0x0FCE + FF/42/03 인터페이스, bulk IN/OUT 엔드포인트 페어, 커널 드라이버 분리(detach) 시도 후 claim
- 장치 선택: fastboot 모드 장치가 **정확히 1대**일 때만 open(§9-3 다중 기기 거부 — usb_modes와 같은 규칙)

## 계약 (02-contracts fastboot 절 교체)

```ts
invoke('fastboot_getvar', { }) → Record<string,string>            // ✅ 읽기 전용 — getvar:all 파싱
invoke('fastboot_unlock', { code: string, confirm: boolean }) → { unlocked: boolean }
//   16자리 hex 검증 후 "oem unlock 0x{code}" — 로그·이벤트에 코드 마스킹(§12.5)
invoke('fastboot_lock', { confirm: boolean }) → { unlocked: boolean }   // "oem lock"
invoke('fastboot_flash', { partition, path, confirm }) → void     // download → flash <part>_a/_b + 이력 기록
invoke('fastboot_reboot', { target: 'os'|'bootloader' }) → void
// 이벤트 'fastboot:log': { line: string }   — INFO 프레임·진행 상황(코드 마스킹)
```

- 파괴 명령(unlock/lock/flash)은 confirm=true 필수(계약 기존 방침) + wizard의 위험 배지·확인 게이트(기존 UI)
- 언락/리락 후 getvar로 결과 확인(unlocked yes/no) — 성공 판정은 응답+상태 이중 확인(§8 원칙 계승)

## wizard 연결

- `data/runMode.ts`: `REAL_STEPS = { backup, restore, fastboot }`(기본 전부 false)
- 언락 단계: mode-wait(fastboot 진입, 이미 실측 감지) → [실전] fastboot_getvar 헬스 → `fastboot_unlock(code)` → getvar 재확인 → 완료/실패(failStep — 초기화는 이미 발생했으므로 실패 시 재시도 안내)
- 리락 단계: 최소 게이트 = getvar unlocked=yes + (언루팅 단계 done) → `fastboot_lock` → 확인
- 코드 마스킹: 로그·journal에 `0x1234********` 형태만

## 테스트 전략 (실기기 없이)

1. FakeTransport 시나리오: 단순 OKAY / FAIL(reason 파싱) / INFO 3프레임+OKAY / DATA 정상 / DATA 용량 불일치 / 타임아웃 / getvar:all 파싱(키 벨리에이션)
2. getvar:all 파싱 — 표준 출력 형식(`(bootloader) key: value`) 유닛 테스트
3. unlock: 코드 검증(16진 16자리)·명령 조립(`0x` 접두 중복 방지 — 기존 normalizedUnlockCode 규칙과 동일)
4. flash: download 협상 → 청크 전송 → flash:<part>_a/_b 순서·이력 기록 검증
5. 상태머신: INFO 루프 상한, 타임아웃, 응답 불일치(알 수 없는 status)

## 리스크·검증 대기

| 항목 | 상태 | 대응 |
|---|---|---|
| rusb 실제 open·클레임(드라이버 바인딩) | 미실측 | 기존 android_winusb 팩 가이드 유지·실측 후 조정 |
| Sony fastboot 응답 변형(INFO 프레임 포함 여부) | 미실측 | INFO는 로그로 남기고 종결 응답 기다림(유한) |
| `flashing get_unlock_ability` 필요 여부 | 미실측 | getvar에 포함해 정보 수집만 — 판단은 실측 후 |
| 슬롯 헬스(slot-retry-count 0 등) 게이트 | 설계 | getvar 값 그대로 반환 — 프론트 표시·판정은 실측 후 |

## 구현 순서 (커밋 단위)

1. `docs(plans)`: 이 문서 + 02-contracts fastboot 절 정정
2. `feat(fastboot)`: transport(트레이트+rusb) + 프로토콜 상태머신 + Fake 테스트
3. `feat(fastboot)`: getvar/unlock/lock/flash/reboot 명령·이벤트 + facade
4. `feat(front)`: wizard 언락/리락 실전 연결(REAL_STEPS.fastboot 기본 꺼짐) + 최소 리락 게이트
5. `docs(plans)`: 상태 배지 갱신
