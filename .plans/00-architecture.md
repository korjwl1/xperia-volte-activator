# 00 — 런타임 아키텍처 요약

status: implemented (읽기 전용 실측 + 쓰기 작업 mock)

## 계층

```
[src/lib/views/*]      화면 — 순수 표시/인터랙션, 직접 I/O 없음
                       DeviceStatus · Warning · VolteConfig · PlanReview · RunProgress · Finish
[src/lib/components/]  공통 위젯 — OptionCard / OptionCategory / AppClassList / Sidebar
[src/lib/stores/]      wizard.svelte.ts — 위자드 상태 머신 (단계/선택/실측 결과/실행 러너)
[src/lib/domain/]      plan.ts(순수 실행 계획) · journal.ts(디스크 입력 검증) · asyncQueue.ts(저장 순서)
[src/lib/api/]         index.ts(명령 facade) · transport.ts(IPC/이벤트/창 lifecycle, 주입 가능한 BackendPort)
[src/lib/mock/]        apps.ts(백업 항목·앱 큐레이션) · device.ts(브라우저 dev용)
[src/lib/data/]        omd.ts(OMD 안내, 임시값) · links.ts(외부 링크·마스킹) · devices.ts(기종별 파티션)
[src-tauri/src]        adb.rs(연결 선택·질의) · device_io.rs(셸 결과 해석) · host.rs(PC 질의)
                       app_paths.rs(앱 경로) · storage.rs(원자 저장) · tasks.rs(블로킹 I/O·제한된 질의 스레드)
                       backup/(수집·검증·복원) · fastboot/(프로토콜·USB 전송·쓰기 게이트)
```

## 앱 셸 (routes/+page.svelte)

- device(1페이지): 풀스크린, 헤더/사이드바/하단 바 없음
- warning: 헤더 + 콘텐츠 + 하단 액션 바 (사이드바 없음 — 1~4단계 시작 전)
- step1~4: 헤더(앱 이름·버전) + 좌측 단계 사이드바(전진 금지, 3단계 진행 중 하위 단계 최대 3개) + 콘텐츠
  - 하단 액션 바: warning·step1은 셸 공통, step2는 뷰 내부(이전/실행), step3·4는 뷰 내부 버튼
- 테마: 시스템 prefers-color-scheme 자동 (강제 없음)
- 기본 창 크기: 1481x902 (논리 픽셀)

## 흐름 (사용자 지시 반영)

```
device(1페이지) → warning(OMD 확인·초기화 경고·책임 동의)
→ step1 사전 옵션 선택(슬롯별 통신사/패치 안 함 + 펌웨어 버전)
→ step2 작업 옵션 선택(백업 및 복구 / 루팅 탭 + 실행 순서) → [실행] → (초기화 단계 시) 확인 모달
→ step3 VoLTE 패치 진행 → step4 점검 및 마무리
```

## 실행 계획 (단일 공급원)

`domain/plan.ts buildPlan(device, volteConfig, opts, hasBackup)` 하나만 사용한다.
step2의 "실행 순서" 미리보기(`wizard.plan`)와 실제 실행(`wizard.launch()`)이 같은 결과를 쓴다.
순서: 사전 준비(언락 조건 확인·언락 코드·펌웨어 준비 — 필요한 것만) → (업데이트) 펌웨어 다운로드 → 백업 → (업데이트) 펌웨어 업데이트·업데이트 확인
→ (잠김+패치) 언락 → 기본 설정 → (필요 시) 루팅 → (패치) 연결 안정성 검사 → VoLTE 적용
→ 적용 확인 → VoLTE 활성화 설정(재부팅) → (선택) 언루팅 → (선택) 리락 → 최종 확인 → (초기화가 있을 때만) 복구

## 조건별 생략·포함 (buildPlan)
- VoLTE 패치 없음 + 업데이트만: 언락·EFS·언루팅·리락 생략, 루팅된 기기는 "루팅 다시 적용"(새 버전 이미지)
- 이미 언락: 언락·언락 조건 확인·언락 코드 생략 / 업데이트는 .ta를 제외하므로 언락 상태 유지
- 업데이트 선택: 루팅이 풀리므로 루팅 단계 포함(사전 준비의 부트 이미지는 새 버전), 사용자 데이터 유지(초기화 없음)
  - VoLTE 패치를 같이 하면 모뎀 포함 업데이트(이후 VoLTE 재적용), 업데이트만이면 모뎀 제외(기존 VoLTE 유지)

## 핵심 도메인 규칙 (UI가 반드시 반영)

- 의존성: 리락 ⟹ 언루팅 (자동 포함)
- 초기화 단계(언락/리락): 실행 순서에 ⚠ + 툴팁, 실행 전 확인 모달 필수, 백업 0개면 이중 확인
- 복구는 초기화가 실제로 일어나는 계획에서만 (덮어쓰기 방지)
- 판별 불가 값은 "확인 불가"로 표기 — 추측 금지 (plan §3-2)
- QPST: 미설치 = 정상 상태, 폴백 트리거는 EFS 실패 시점뿐 (§7.5)
- persist 프롭: 리락 후 소실 → VoLTE 판정은 IMS 등록 상태 기준, 최종 확인 단계 존재 (§4)

## 계층·코딩 규칙 (2026-10-04 리뷰)

- 뷰/스토어는 API facade만 호출한다. `@tauri-apps/*` import는 `src/lib/api/` 안에 둔다.
- domain은 공통 타입만 의존하고 Tauri·Svelte 상태·mock·파일 시스템에 의존하지 않는다.
- Rust 명령은 입력과 이벤트를 처리하고, 장시간 I/O는 `tasks::blocking`/`guarded`로 위임한다. 백업 엔진은 주입한 `ADBDeviceExt`와 진행 콜백으로 실행한다.
- 질의 실패는 `null`, 작업 실패는 공통 `ApiResult<T>`로 구분한다. 백업 진행 이벤트의 `file`은 `string | null`이다.
- UTF-8·LF·공백 들여쓰기(`.editorconfig`, `.gitattributes`), Rust는 rustfmt, TS/Svelte는 2칸·이중 따옴표를 기본으로 한다.
- `pnpm test`는 실제 store/domain과 가짜 IPC로 검증하며 뷰·domain의 IPC 경계 위반도 검사한다.
- Wizard는 실행 상태를 조정하는 계층으로 유지한다. 추가 엔진을 연결할 때 순수 판단과 독립 서비스부터 추출한다.

전체 리뷰의 수정 내역과 실기기 미검증 목록: [code-review.md](04-engine/code-review.md).

병합 후 엔진 통합 점검: [integrated-review.md](04-engine/integrated-review.md).

- `boot_image`는 기본 입력 검사·해시·현재 펌웨어 지문 대조를 공통 제공한다. 순정 인증·AVB 검증을 대신하지 않는다.
- `storage::hash_reader`는 스트리밍 파일 해시, `device_io::WriteOperation`은 백업/복구·Magisk·fastboot의 변경 작업 실행권을 제공한다. 실행권은 I/O 종료까지 유지한다.
- `domain/waitUntil`은 제한 시간·취소와 단일 질의를 관리한다. 이미 실행된 기기 명령의 강제 취소를 보장하지 않는다.
- 루팅·언루팅은 root/fastboot 모두 실전 활성화 필요. 리락은 현재 펌웨어·순정 추출 출처·동일 기기·양 슬롯 최신 완료 이력을 재검사한 뒤 조건부 실행한다. AVB·전체 체인 인증과 실기기 검증은 별도이며 구현 조건은 04-engine/unroot-relock.md를 따른다.## ?? (2026-10-08)

?? ? mode-select (?? | ?? | ????) ? ??? ??/?? ? ?? ??/?? ? ?? ? ??. ?? ?? ??? ?? ?? ????. USIM ??? ???? ????. ? ?? ??/??? VoLTE ?? ??? ?? update ??? ????. ??/?? ?? ???? ?? ????.

??? domain/plan.ts ??? ???? opts.mode/manualTask? ????. ???? ??? ??/??/??? ???? ??? ?? ??? ????. ??? ?? ??/??? ??? ???? ???/?? ???? ???? ????. ??? ???? ???? ?? ??, ???? ??? ?? ???? ?? ????.

? ??? ?? ?? ? ??? ??? ?? ???? ?? ??? ?? ?? ??? ????. ???? ??/??/??? ????? ?? ??? ??? ?? ????.


