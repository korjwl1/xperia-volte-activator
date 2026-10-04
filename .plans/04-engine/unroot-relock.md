# 언루팅·리락 게이트 (M4 후속) — 구현 설계

status: implemented (실기기 검증 대기 — 게이트 단위 테스트 9종 포함 133통과, fastboot-lock 게이트는 fastboot-write 뒤)

- 상위 정책: `tasks/plan.md` §3-3(리락 게이트 의존성 규칙)·§5(초기화 루트)
- 원본 CLI 계승: `unRoot()`(src/cliInterface.py 461) — 순정 IMG 양 슬롯 기록 후 "Magisk 앱 직접 삭제" 안내(앱 삭제 자동화 안 함)
- 차단 해제 대상: `fastboot::ensure_relock_verified()` 하드 차단(Codex 0908266) → 실제 게이트 판정으로 교체

## 리락 게이트 규칙 (§3-3 "엔진 강제")

> 리락 게이트 = ①검증된 순정 이미지 해시(우리가 SIN에서 직접 추출) ②부트 체인 파티션×슬롯 플래시 이력. 미충족 시 리락 차단.

구현 판정:
1. **순정 증거**: 호출자가 지정한 순정 이미지 경로(firmware_fetch 결과)를 해시 — ANDROID! 매직 확인(부트 이미지 형식)
2. **이력 판정**: `flash-history.jsonl`(deviceKey 필터)에서 boot·init_boot 양 슬롯(_a/_b) 각각의
   **마지막 done 항목**이 존재하고 그 sha256 == 순정 이미지 sha256이어야 통과
   - 마지막 done이 패치 이미지 해시 → 차단(언루팅 안 됨)
   - done 없음(전부 started/failed) → 차단(미완료 기록 — 한 슬롯만 성공한 경우 오인 방지, Codex 기록 방침 계승)
   - **우리 이력에 없는 파티션 = 미조작으로 간주(순정 유지)** — 공장 상태에서 온 파티션은 이력이 없음
3. **이중 적용**: 사전 점검 명령(안내 표시용) + `fastboot_lock` 내부 강제(서버 측 최종 방어)

한계(문서화): 다른 도구로 플래시한 이력은 알 수 없다 — 수동 오버라이드("AVB 위험 감수")는 §3-3이 허용하나
v1에서는 제공하지 않고 차단 메시지로 안내만(언루팅→리락 표준 경로로 해결).

## 언루팅 절차 (원본 unRoot 계승 + 자동화)

```
[자동] 순정 이미지 확인(firmware_fetch 결과·ANDROID! 매직) — patchedImage(패치 이미지)와 해시 다름 확인
[자동] adb reboot bootloader(root_reboot) → fastboot 감지 대기(waitFor)
[자동] fastboot_flash(partition, 순정이미지) — 양 슬롯 기록 + 이력 자동 기록(게이트 재료)
[자동] fastboot reboot os → adb 복귀 대기
[안내] Magisk 앱 실행해 언루팅 확인 → 사용자가 직접 앱 삭제(원본과 동일 — 자동화 안 함)
[안내] Play 프로텍트 미인증 시 Play 스토어 앱 정보에서 앱 데이터 삭제(원본 문구 계승)
```

## 계약 (02-contracts 정정)

```ts
invoke('relock_gate_check', { partition, stockPath, deviceKey? }) → RelockGate
//   RelockGate = { ok: boolean, reasons: string[], checked: { partition, slot, ok, detail }[] }
//   읽기 전용 사전 점검 — fastboot 모드 아님(adb 연결 중)에도 호출 가능. deviceKey 생략 시 전체 이력 대상(안내용)
//   권한 판정은 fastboot_lock이 내부에서 다시 수행한다(기기 fastboot serial 기준 — 최종 방어)
invoke('fastboot_lock', { confirm }) → { unlocked }   // 내부: 리락 게이트 통과 필수로 교체(하드 차단 제거)
// 언루팅 절차는 기존 명령 조합(root_reboot + fastboot_flash + fastboot_reboot) — 신규 명령 없음
```

- 게이트: `fastboot_lock`은 Cargo feature `fastboot-write` + 프론트 REAL_STEPS.fastboot 이중.
  `relock_gate_check`는 읽기 전용(파일 판정만)이라 게이트 밖 — 사전 점검 안내용, 최종 판정은 fastboot_lock이 기기 fastboot serial 기준으로 재수행
- wizard 언루팅 러너 게이트: `REAL_STEPS.root && REAL_STEPS.fastboot`(adb 재부팅 + 기록 모두 사용).
  mode-wait 수동 개입에서 앱이 부트로더 재부팅을 자동 수행(안내 문구와 실제 동작 일치 — 기존 갭 해소)

## 구현

```
src-tauri/src/fastboot/relock.rs — 게이트 순수 로직(이력 파싱·판정·사유 조립) + 단위 테스트
src-tauri/src/fastboot/mod.rs  — ensure_relock_verified → verify_relock(...) 실제 판정, relock_gate_check 명령
wizard — runRealUnroot(위 절차) + runRealRelock에 사전 게이트 점검(사유 로그·미충족 시 failStep)
```

- 순정 이미지 해시는 게이트 시점에 파일에서 다시 계산(전송 버퍼 해시와 동일 알고리즘 — fastboot_flash와 정합)
- deviceKey: fastboot serialno의 SHA-256 hex(fastboot_flash 기록 방식과 동일). 사전 점검 시 프론트는
  adb serial로 같은 알고리즘 계산(일반적으로 동일값 — 불일치 시 최종 fastboot_lock이 걸러냄)

## 테스트 전략 (실기기 없이)

1. 이력 파싱: jsonl(빈 줄·끊긴 줄 포함) → 항목 무시·정상 항목만 판정
2. 판정 매트릭스: 양 슬롯 순정 done → 통과 / 마지막이 패치 → 차단 / 한 슬롯만 done → 차단 /
   done 없음 → 차단 / 이력 없는 파티션 → 통과(미조작 간주) / 다른 기기 이력 → 제외
3. 순정 이미지 검증: ANDROID! 아닌 파일 → 차단 사유 / patchedImage와 해시 동일 → 순정이 아니라고 차단
4. 시나리오: 루팅(patch 해시 기록) → 언루팅(순정 해시 기록) → 게이트 통과 (이력 순서 보존 판정)

## 구현 순서 (커밋 단위)

1. `docs(plans)`: 이 문서 + 02-contracts 정정 + AGENTS 예외 갱신
2. `feat(fastboot)`: relock.rs 게이트 로직 + 단위 테스트 + relock_gate_check 명령 + fastboot_lock 교체
3. `feat(front)`: wizard 언루팅 러너 + 리락 사전 게이트 점검 연결
4. `docs(plans)`: 상태 배지 갱신
