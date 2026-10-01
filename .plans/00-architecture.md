# 00 — 런타임 아키텍처 요약

status: implemented (읽기 전용 실측 + 쓰기 작업 mock)

## 계층

```
[src/lib/views/*]      화면 — 순수 표시/인터랙션, 직접 I/O 없음
                       DeviceStatus · Warning · VolteConfig · PlanReview · RunProgress · Finish
[src/lib/components/]  공통 위젯 — OptionCard / OptionCategory / AppClassList / Sidebar
[src/lib/stores/]      wizard.svelte.ts — 위자드 상태 머신 (단계/선택/실측 결과/실행 러너)
[src/lib/api/]         facade — 데스크톱은 Rust 명령(읽기 전용 실측), 브라우저 dev는 mock
[src/lib/mock/]        plan.ts(실행 계획 단일 생성기) · apps.ts(백업 항목·앱 큐레이션) · device.ts(브라우저 dev용)
[src/lib/data/]        omd.ts(OMD 안내, 임시값) · links.ts(외부 링크·마스킹) · devices.ts(기종별 파티션)
[src-tauri/src]        adb.rs(기기 읽기 전용 질의) · host.rs(PC 읽기 전용 질의) — 계약은 02-contracts
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
→ step1 SIM 및 통신사 선택(슬롯별 통신사/패치 안 함)
→ step2 작업 옵션 선택(백업 및 복구 / 루팅 탭 + 실행 순서) → [실행] → (초기화 단계 시) 확인 모달
→ step3 VoLTE 패치 진행 → step4 점검 및 마무리
```

## 실행 계획 (단일 공급원)

`mock/plan.ts buildPlan(device, volteConfig, opts, hasBackup)` 하나만 사용한다.
step2의 "실행 순서" 미리보기(`wizard.plan`)와 실제 실행(`wizard.launch()`)이 같은 결과를 쓴다.
순서: 백업 → (잠김) 개발자 옵션 준비 → 언락 → 기본 설정 → (비루팅) 루팅 → 연결 안정성 검사 → VoLTE 적용
→ 적용 확인 → VoLTE 활성화 설정(재부팅) → (선택) 언루팅 → (선택) 리락 → 최종 확인 → (초기화가 있을 때만) 복구

## 핵심 도메인 규칙 (UI가 반드시 반영)

- 의존성: 리락 ⟹ 언루팅 (자동 포함)
- 초기화 단계(언락/리락): 실행 순서에 ⚠ + 툴팁, 실행 전 확인 모달 필수, 백업 0개면 이중 확인
- 복구는 초기화가 실제로 일어나는 계획에서만 (덮어쓰기 방지)
- 판별 불가 값은 "확인 불가"로 표기 — 추측 금지 (plan §3-2)
- QPST: 미설치 = 정상 상태, 폴백 트리거는 EFS 실패 시점뿐 (§7.5)
- persist 프롭: 리락 후 소실 → VoLTE 판정은 IMS 등록 상태 기준, 최종 확인 단계 존재 (§4)
