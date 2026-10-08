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

## 흐름 (2026-10-08)

기기 → mode-select (자동 | 수동 | 업데이트) → 경로별 안내/입력 → 공통 계획/백업 → 실행 → 결과. USIM은 통신사만 선택한다. 새 버전은 VoLTE 인식 기기의 전용 Newflasher 경로만, 자동/수동 부트 이미지는 현재 버전이다. 수동 10개 작업은 선택한 준비/검증만 포함하고 매니저/모듈은 별도 root-tools section으로 이동한다. 자동 루팅 탭은 루팅 유지 시 세트를 제공하며 B→A 의존 선택, 언루팅/리락/언락 초기화 시 자동 해제·비활성화한다.

계획은 buildPlan/opts.mode/manualTask/modules를 사용한다. 백업 뒤 일반 승인만 유지하고 나머지는 엔진 finally 후 연결한다. 전체 펌웨어 실행은 차단한다. 모듈은 순차 설치/재부팅/적용 및 폰 설정 확인을 사용한다. 기본 쓰기 기능은 그대로 꺼져 있다.

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
- 루팅·언루팅은 root/fastboot 모두 실전 활성화 필요. 리락은 현재 펌웨어·순정 추출 출처·동일 기기·양 슬롯 최신 완료 이력을 재검사한 뒤 조건부 실행한다. AVB·전체 체인 인증과 실기기 검증은 별도이며 구현 조건은 04-engine/unroot-relock.md를 따른다.
