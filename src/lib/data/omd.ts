// 통신사별 외산폰 VoLTE OMD 등록 안내 — 경고 페이지 표시용
// 출처: 소니 사용자모임 카페(cafe.naver.com/x1smart) 안내·해결 사례 조사 (tasks/research-cafe-omd-volte.md, 2026-10-03)
// ⚠ 통신사 공식 보증이 아니며 회선·전산 상태에 따라 다를 수 있다 — 코드마다 출처·확인일·적용 조건을 둔다.
//   카페의 "듀얼 SIM 기기를 단일 SIM이라고 설명" 같은 우회 문구는 옮기지 않는다.

export type OmdSim = "물리 SIM" | "eSIM" | "공통";
export type OmdRole = "기본" | "대안" | "예외";

export interface OmdCode {
  code: string;
  net: "5G" | "LTE";
  sim: OmdSim;
  role: OmdRole;
  /** 적용 조건 (망·SIM 종류 외에 더할 것이 있을 때만) */
  when?: string;
  /** 같은 코드의 다른 표기 (고객센터 표기가 다를 수 있음) */
  aliases?: string[];
  source: string;
}

export interface OmdCarrier {
  carrier: "SKT" | "KT" | "LGU";
  label: string;
  /** 한 줄 요약 */
  summary: string;
  codes: OmdCode[];
  /** 등록할 때 SIM을 어느 슬롯에 둘지 */
  simPlacement?: string;
  notes: { text: string; source?: string }[];
}

export const OMD_CHECKED = "2026-10-03";

const CAFE = (id: number) => `https://cafe.naver.com/x1smart/${id}`;

/** 공통 안내 — 카드 위 설명 */
export const omdCommonGuide =
  "통신사 고객센터(114)에 외산폰 VoLTE(OMD) 등록을 요청하세요. 실제 SIM 구성(물리 SIM·eSIM, 슬롯)과 등록할 IMEI를 그대로 알려 주면 됩니다.";

export const omdDisclaimer = `아래 코드는 소니 사용자모임 카페의 안내·해결 사례 기준(${OMD_CHECKED} 확인)이며 통신사 공식 보증이 아닙니다. 회선·전산 상태에 따라 다를 수 있으니 고객센터에서 확인해 주세요.`;

export const omdInfo: OmdCarrier[] = [
  {
    carrier: "SKT",
    label: "SKT",
    summary: "요금제와 관계없이 OMD DEFAULT 5G 계열 권장",
    codes: [
      { code: "OMD DEFAULT 5G", net: "5G", sim: "물리 SIM", role: "기본", aliases: ["OMD-DEFAULT_5G"], source: CAFE(617140) },
      { code: "OMD DEFAULT 5G ESIM", net: "5G", sim: "eSIM", role: "기본", source: CAFE(617140) },
      {
        code: "OMD DEFAULT 5G DUAL USIM",
        net: "5G",
        sim: "물리 SIM",
        role: "대안",
        when: "물리 듀얼 SIM에서 기본 코드로 5G가 잡히지 않을 때 — 고객센터 확인 필요",
        source: CAFE(611952),
      },
      { code: "OMD SONY LTE핸드셋_VOLTE", net: "LTE", sim: "공통", role: "기본", when: "LTE만 사용할 때 (Sony 전용)", source: CAFE(617140) },
      {
        code: "OMD 기타 LTE핸드셋_VOLTE",
        net: "LTE",
        sim: "공통",
        role: "대안",
        when: "LTE만 사용할 때 (범용 코드)",
        aliases: ["OMD기타VoLTE핸드셋_VOLTE"],
        source: CAFE(617140),
      },
    ],
    simPlacement: "등록할 IMEI 쪽 슬롯은 비우고, 반대 슬롯에 SIM을 둔 채 등록합니다 (다른 폰을 써도 됩니다)",
    notes: [{ text: "SIM을 다른 슬롯이나 폰으로 옮기면 기존 VoLTE 패치가 풀릴 수 있습니다", source: CAFE(609728) }],
  },
  {
    carrier: "KT",
    label: "KT",
    summary: "LTE는 SONY-XPR-TAC, 5G는 물리 SIM·eSIM 코드가 다름",
    codes: [
      { code: "SONY-XPR-TAC", net: "LTE", sim: "공통", role: "기본", when: "LTE 사용 또는 아직 패치하기 전", source: CAFE(617140) },
      { code: "PTA-TYPE5G", net: "5G", sim: "물리 SIM", role: "기본", when: "패치 완료 후 (APN도 함께 확인)", source: CAFE(617140) },
      { code: "PTA-DS-5G", net: "5G", sim: "eSIM", role: "기본", when: "물리 SIM 코드와 바꿔 쓰지 않음", source: CAFE(617140) },
    ],
    simPlacement: "등록할 IMEI의 슬롯에 SIM을 둔 채 등록합니다",
    notes: [
      {
        text: "기본 OMD 상태에서도 LTE VoLTE는 막히지 않고 5G를 쓸 때 등록이 필요하다는 안내와 사례가 있습니다 — 회선마다 다를 수 있어 고객센터 확인",
        source: CAFE(601876),
      },
      { text: "예전 안내의 PTA-VoLTE는 최신 안내에서 SONY-XPR-TAC로 바뀌었습니다 (예전 코드의 무효 여부는 미확인)" },
    ],
  },
  {
    carrier: "LGU",
    label: "LG U+",
    summary: "보통 별도 등록 없이 사용 — 네트워크 유형(5G/LTE) 설정 확인",
    codes: [
      {
        code: "OMD-STDPHONE",
        net: "5G",
        sim: "공통",
        role: "예외",
        when: "LTE 단말로 등록되어 5G가 제한된 경우에만",
        aliases: ["OMD_STDPHONE"],
        source: CAFE(614300),
      },
    ],
    notes: [{ text: "통화·5G가 안 될 때 전산 등록 상태를 고객센터에 확인해 주세요", source: CAFE(612332) }],
  },
];
