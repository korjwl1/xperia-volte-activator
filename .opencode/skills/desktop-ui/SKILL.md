---
name: desktop-ui
description: xperia-volte-activator 데스크탑 앱 UI/UX 가이드. 뷰/컴포넌트 작성·수정 시 반드시 적용. MD3 톤 기반 컬러, pane 스크롤 구조, 도구류(flasher/installer) UI 패턴 규칙.
---

# 데스크탑 UI/UX 가이드 (xperia-volte-activator)

웹페이지가 아니라 **데스크탑 프로그램**을 만든다. 모든 화면·컴포넌트 작성 시 아래 규칙을 적용한다.

## 1. 스크롤 구조 (가장 중요)

- `html/body`는 **절대 스크롤되지 않는다** (app.css에서 `overflow:hidden` 고정)
- 뷰포트 = 고정 프레임. 스크롤은 **내부 pane 단위로만** 발생:
  - 콘텐츠 영역(메인) = 자체 스크롤
  - 목록이 길면(백업 항목, 로그) 그 pane이 자체 스크롤 + sticky 헤더
  - 실행 화면 = 좌(단계)/우(로그) 분할, 각각 독립 스크롤
- 타이틀 바·사이드바·액션 바는 항상 고정 — 스크롤에 절대 함께 움직이지 않음
- 스크롤바는 얇고 반투명하게 (app.css 커스텀)

## 2. 색상 시스템 (Material 3 톤 기반)

- 토큰은 `src/app.css`의 CSS 변수가 단일 공급원 (`--primary`, `--success`, `--warning`, `--info`, tonal surface들)
- **의미색을 적극 사용**: 성공/통과=success 그린, 경고=warning 앰버, 위험/초기화=destructive 레드, 정보=info 블루, 대기/불명=muted
- 상태 표기는 텍스트만이 아니라 **색 채운 아이콘 + 토널 배경 칩**으로 (✅⚠️❌ 이모지 대신 lucide 아이콘)
- 표면 위 표면: 카드는 border 대신 **미세 그림자(elev-1/2) + tonal container**로 구분 — MD3 elevation 참고
- 히어로/섹션 헤더에 **gradient** 적극 사용 (`grad-hero` 유틸) — 단, 본문 텍스트 가독성 우선

## 3. 레이아웃 밀도와 구성

- 기본 폰트 12~13px, 행 높이 조밀하게 — 데스크탑 밀도 (모바일 웹 아님)
- 최소 해상도 1280×720 가정 — 한 화면에 핵심 정보가 접히지 않게
- 마법사 단계는 사이드바(전진 금지, 뒤로만) + 하단 액션 바 — 설치 마법사 관례
- 위험 단계(언락/리락/초기화)는 항상 **컬러 배지 + 아이콘**으로 시각적 경각

## 4. 도구류(flasher/installer) UI 관례 — 레퍼런스

- Etcher/RPi Imager: 큰 터치 목표, 한 화면 = 한 의사결정, 진행 상태가 화면의 이벤트
- Odin 계열: 파티션/단계 상태가 **항상 보이는 고정 패널**, 로그는 하단/측면 별도 박스
- ours: 실행 화면 = 단계 타임라인(좌) + 콘솔 로그(우, 모노스페이스·다크) — Odin+Imager 혼합
- 대기/수동 단계는 모달이 배경을 확실히 덮고 **다음 행동을 번호 스텝으로** 안내

## 5. 아이콘·일러스트

- 아이콘: `@lucide/svelte` 필수 사용 (import X from "@lucide/svelte/icons/x") — 이모지 금지
- 기기 일러스트: `DeviceHero` SVG 컴포넌트 (폰 슬라베 + 그라디언트 스크린)
- 상태 아이콘 조합: CheckCircle2(성공) / TriangleAlert(경고) / OctagonX(위험·실패) / Info(정보) / Loader2(실행중, animate-spin)

## 6. 금지

- 전체 페이지 스크롤, 브라우저 감성의 초대형 여백/중앙정렬 1단
- 텍스트만 나열한 상태 표기, 회색 온니 UI
- 이모지로 상태 표기, 컬러 하드코딩 (반드시 토큰 변수 사용)
- 인라인 style 색상 — 유틸리티/토큰 클래스로만

## 7. 변경 시 의무

- 토큰/유틸 변경은 app.css에서만, 뷰 문서(.plans/01-views/*)의 "비주얼" 라인 갱신
