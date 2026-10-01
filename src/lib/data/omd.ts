// 통신사별 외산폰 VoLTE OMD 등록 안내 — 경고 페이지 표시용
// ⚠ 검증 필요: 임시값 (출처: 사용자 수동 가이드(7모바일, SKT망), 나무위키 VoLTE 문서, itnmobile.net/131 — 2026-10 조사)
//   사용자가 실제 값을 확인해 알려주면 이 파일만 수정한다.

export interface OmdInfo {
  carrier: "SKT" | "KT" | "LGU";
  label: string;
  /** 고객센터에 요청할 OMD 코드 (용도: 코드) */
  codes: { use: string; code: string }[];
  /** 등록 방법 */
  how: string;
  /** 주의사항 */
  note?: string;
}

export const OMD_VERIFIED = false;

export const omdInfo: OmdInfo[] = [
  {
    carrier: "SKT",
    label: "SKT (알뜰폰 포함)",
    codes: [
      { use: "5G", code: "OMD-DEFAULT_5G" },
      { use: "LTE", code: "OMD기타VoLTE핸드셋_VOLTE" },
    ],
    how: "고객센터(114) → 통화 품질 상담 → 외산폰 VoLTE OMD 등록 요청",
    note: "등록 시 다른 휴대폰에 USIM이 꽂혀 있어야 할 수 있습니다 (세컨폰 필요)",
  },
  {
    carrier: "KT",
    label: "KT (알뜰폰 포함)",
    codes: [
      { use: "LTE", code: "PTA-VoLTE" },
      { use: "5G", code: "PTA-TYPE5G" },
    ],
    how: "고객센터(114) → 외산폰 VoLTE OMD 등록 요청",
  },
  {
    carrier: "LGU",
    label: "LG U+ (알뜰폰 포함)",
    codes: [{ use: "미등록 시", code: "OM-Phone" }],
    how: "대부분 USIM 장착만으로 자동 등록됩니다. 통화가 안 되면 고객센터(114)에 등록 요청",
  },
];
