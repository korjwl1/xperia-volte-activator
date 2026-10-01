// 통신사별 외산폰 VoLTE OMD 등록 안내 — 경고 페이지 표시용
// ⚠ 검증 필요: 임시값 (출처: 사용자 수동 가이드(7모바일, SKT망), 나무위키 VoLTE 문서, itnmobile.net/131 — 2026-10 조사)
//   사용자가 실제 값을 확인해 알려주면 이 파일만 수정한다.

export interface OmdInfo {
  carrier: "SKT" | "KT" | "LGU";
  label: string;
  /** 망별 OMD 코드 */
  codes: { net: "5G" | "LTE"; code: string }[];
  /** 해당 통신사 전용 안내 — 카드 아래 * 각주로 표시 */
  footnote?: string;
}

export const OMD_VERIFIED = false;

/** 공통 안내 — 카드 위 설명 */
export const omdCommonGuide =
  "통신사 고객센터(114)에 전화해 외산폰 VoLTE OMD 등록을 요청하고, 아래 코드로 변경해 달라고 안내하세요.";

export const omdInfo: OmdInfo[] = [
  {
    carrier: "SKT",
    label: "SKT",
    codes: [
      { net: "5G", code: "OMD-DEFAULT_5G" },
      { net: "LTE", code: "OMD기타VoLTE핸드셋_VOLTE" },
    ],
    footnote: "SKT: 등록 시 다른 휴대폰에 USIM이 꽂혀 있어야 할 수 있습니다 (세컨폰 필요)",
  },
  {
    carrier: "KT",
    label: "KT",
    codes: [
      { net: "5G", code: "PTA-TYPE5G" },
      { net: "LTE", code: "PTA-VoLTE" },
    ],
  },
  {
    carrier: "LGU",
    label: "LG U+",
    codes: [
      { net: "5G", code: "OM-Phone" },
      { net: "LTE", code: "OM-Phone" },
    ],
    footnote: "LG U+: 대부분 USIM 장착만으로 자동 등록되므로, 통화가 안 되는 경우에만 등록을 요청하세요",
  },
];
