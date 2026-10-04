// 실행 단계 판정 규칙(순수 함수) — 스토어는 기기 조회·대기만 하고 판정은 여기서 한다.

/** 지문 앞부분(브랜드/제품/기기) — "Sony/XQ-DQ44/XQ-DQ44:15/67.2.A.3.178/…" → "Sony/XQ-DQ44/XQ-DQ44" */
function fingerprintDevice(fp: string): string {
  return fp.split(":")[0] ?? "";
}

/**
 * 펌웨어 업데이트 확인 — 문제가 없으면 빈 배열.
 * 버전(ro.build.id)이 대상과 같고, 지문이 대상 버전을 담고 있어야 한다.
 * - expected: 받아 둔 대상 펌웨어의 지문(기기·지역 대조를 이미 거친 값) — 있으면 정확히 같아야 한다
 * - before: 업데이트 전 기기 지문 — expected가 없을 때 기기·지역(지문 앞부분) 대조에 쓴다
 * 지문 자체를 못 읽으면 확인 실패로 본다(추측하지 않음).
 */
export function firmwareUpdateProblems(
  target: string,
  after: { firmware: string; fingerprint?: string },
  ref: { before?: string; expected?: string } = {},
): string[] {
  const problems: string[] = [];
  if (after.firmware.trim() !== target) {
    problems.push(`설치된 버전(${after.firmware || "알 수 없음"})이 업데이트 대상(${target})과 다릅니다`);
  }
  const fp = after.fingerprint?.trim() ?? "";
  if (!fp) {
    problems.push("기기 지문을 읽지 못해 펌웨어를 대조할 수 없습니다");
    return problems;
  }
  const expected = ref.expected?.trim();
  if (expected) {
    if (fp !== expected) problems.push("기기 지문이 받아 둔 업데이트 펌웨어의 지문과 다릅니다");
    return problems;
  }
  if (!fp.includes(`/${target}/`)) {
    problems.push("기기 지문에 업데이트 대상 버전이 없습니다");
  }
  const before = ref.before?.trim();
  if (before && fingerprintDevice(before) !== fingerprintDevice(fp)) {
    problems.push("업데이트 전과 기기·지역 정보가 다릅니다 — 다른 지역 펌웨어가 기록됐을 수 있습니다");
  }
  return problems;
}
