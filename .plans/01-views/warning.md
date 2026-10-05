# view: Warning (경고 및 동의 — 1페이지 [VoLTE 작업 시작] 직후)

status: implemented (정적 안내, 백엔드 호출 없음)

2026-10-05 과도한 안내 검토: SKT/KT 슬롯 이동 안내는 카페의 물리 USIM 절차임을 명시한다. eSIM에도 슬롯 이동이 필요하다는 의미로 읽히지 않도록 고객센터 안내를 따르도록 정리했다. 근거 없는 KT 옛 OMD 코드 변경 설명은 기본 안내에서 제거했다. OMD 확인은 자동 등록·SIM 이동 또는 EFS 기록 조건이 아니다. 기존 카드·레이아웃·컴포넌트는 유지했다.

2026-10-05: OMD는 EFS 기록 자체의 필수 조건이 아니다. SKT는 필요한 전산 변경·SIM 이동을 패치 전에 정리, KT LTE는 기본 OMD로 VoLTE 가능(앨리자 617140의 엘렌나 댓글), KT 5G는 미패치 상태의 3G 전환 가능성을 안내하고 패치 완료와 코드·APN 전환을 맞춘다. LG U+는 보통 변경 생략, LTE 단말 전산 등록으로 5G가 제한된 경우만 예외. 비주얼은 기존 MD3 카드·pane 스크롤 유지.

## 목적/진입
1페이지(DeviceStatus)의 [VoLTE 작업 시작] → 이 페이지 → 동의 후 1단계(SIM 및 통신사 선택).
원본 CLI의 시작 동의 화면(agreement)을 계승 + OMD 사전 등록 확인을 추가 (사용자 지시 2026-10-02).

## 표시 내용
1. **OMD 등록 확인** — 상단 공통 안내(114에 OMD 등록 요청) → 3사(SKT/KT/LG U+) 카드: 5G/LTE별 OMD 코드만
   → 카드 아래 `*` 각주로 통신사 전용 안내(SKT 세컨폰, LG U+ 자동 등록)
   - 데이터: `src/lib/data/omd.ts` (⚠ 임시값, `OMD_VERIFIED=false` — 사용자 검증 후 이 파일만 수정)
   - 체크: "사용할 SIM의 OMD 등록을 완료했습니다" → `wizard.omdAck`
2. **데이터 초기화 경고** — 언락/리락 시 초기화, 백업해도 복구 안 되는 항목(인증서·OTP 등), 백업 권장
3. **책임 고지** — 기기·데이터 손상 등 모든 책임은 사용자에게 있음
   - 체크: "위 내용을 이해했으며 동의합니다" → `wizard.riskAck`

## 인터랙션 → 계약 매핑
| 요소 | 동작 | 계약 |
|---|---|---|
| 체크박스 2개 | 스토어 갱신 | (프론트) |
| [다음] | 두 체크 모두 시에만 활성 → step1 | — |
| [이전] | device 복귀 | — |

## 이탈
동의 상태는 스토어 유지(step1에서 이전으로 돌아와도 체크 유지). [처음으로](restart) 시 초기화.

## 비주얼
step 페이지 셸(헤더 + 중앙 카드 + 하단 액션 바), 사이드바 없음(1~4단계 시작 전).
카드 3장: OMD(primary 아이콘) / 초기화 경고(warning) / 책임 고지(destructive 테두리). 체크 시 primary 톤.


## OMD 안내 갱신 (카페 조사 반영, 2026-10-03 — tasks/research-cafe-omd-volte.md)

- 데이터: src/lib/data/omd.ts — 코드마다 망(5G/LTE)·SIM 종류(물리 SIM/eSIM/공통)·구분(기본/대안/예외)·적용 조건·다른 표기·출처 URL, 확인일 OMD_CHECKED.
  전역 OMD_VERIFIED 플래그 제거, 대신 "카페 안내 기준 · 통신사 공식 보증 아님 · 고객센터 확인" 문구를 항상 표시
- SKT: OMD DEFAULT 5G(물리, 다른 표기 OMD-DEFAULT_5G) / OMD DEFAULT 5G ESIM / 대안 OMD DEFAULT 5G DUAL USIM / LTE는 OMD SONY LTE핸드셋_VOLTE, 범용 OMD 기타 LTE핸드셋_VOLTE.
  등록 시 SIM 위치: 등록할 IMEI 쪽 슬롯은 비우고 반대 슬롯에 SIM (예전 "세컨폰 필요" 일반화 제거)
- KT: LTE·패치 전 SONY-XPR-TAC / 5G 물리 PTA-TYPE5G(패치 후, APN 확인) / 5G eSIM PTA-DS-5G. 등록 시 SIM은 등록할 IMEI의 슬롯.
  기본 OMD로도 LTE VoLTE가 막히지 않는다는 안내 → 등록 "완료"가 아닌 "조건 확인"으로. 예전 PTA-VoLTE는 바뀜(무효 여부 미확인)
- LG U+: 보통 별도 등록 없음, 예외로 LTE 단말 등록 때문에 5G 제한 시 OMD-STDPHONE (OM-Phone 강제 안내 제거)
- 체크박스 문구: "사용할 망의 OMD 등록 조건을 확인했습니다"
- 카페의 "듀얼 SIM을 단일 SIM이라고 설명" 같은 우회 문구는 옮기지 않음 — 실제 SIM 구성과 IMEI를 알리도록 안내
- 레이아웃: 통신사별 블록(요약 + 코드 타일 — 페이지 최대 폭 max-w-5xl, 카드 폭 기준 1/2/3열(넓으면 한 줄 3개: SKT 5G 코드 3개가 같은 줄) + SIM 위치 + 각주), 출처는 외부 브라우저로 열기
