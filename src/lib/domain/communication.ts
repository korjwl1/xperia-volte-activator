import type { CallCheck, ImsDiagnostic, SimInfo } from "$lib/types";

export const simTypeLabel = (type: SimInfo["type"]): string =>
  type === "physical" ? "물리" : type === "esim" ? "eSIM" : "유형 미확인";

export const CALL_ITEMS = [
  { id: "outgoing", label: "발신 연결" },
  { id: "incoming", label: "다른 전화에서 수신" },
  { id: "audio", label: "양쪽 목소리 전달" },
  { id: "afterReboot", label: "재부팅 후 통화 유지" },
  { id: "afterIdle", label: "잠시 대기 후 수신 유지" },
] as const;
export type CallItem = typeof CALL_ITEMS[number]["id"];
export const newCallCheck = (slot: 1 | 2): CallCheck => ({ slot, outgoing: false, incoming: false, audio: false, afterReboot: false, afterIdle: false });
export const callsComplete = (checks: CallCheck[], slots: (1 | 2)[]) => slots.length > 0 && slots.every(slot => checks.some(c => c.slot === slot && c.outgoing && c.incoming && c.audio));

const STATUS_LABEL: Record<ImsDiagnostic["status"], string> = {
  "no-sim": "SIM 미삽입", "sim-not-ready": "SIM 준비·잠금 상태 확인 필요", "query-failed": "IMS 조회 실패",
  "unsupported-format": "IMS 출력 형식 확인 불가", "conflicting-evidence": "IMS 상태가 서로 달라 재조회 필요",
  "not-registered": "IMS 미등록", registering: "IMS 등록 중", "voice-unavailable": "IMS 음성 사용 불가",
  registered: "셀룰러 IMS 음성 준비", "wifi-only": "Wi-Fi 통화 등록", "cross-sim": "다른 SIM 데이터 경유 등록",
  "other-network": "LTE·NR 외 IMS 등록", "transport-unknown": "IMS 등록됨 · 접속 방식 미확인",
};
export function imsLabel(sim: SimInfo): string {
  if (sim.ims) return STATUS_LABEL[sim.ims.status] ?? "IMS 상태 확인 불가";
  if (sim.state === "ABSENT") return STATUS_LABEL["no-sim"];
  return sim.volte === "on" ? STATUS_LABEL.registered : sim.volte === "wifi" ? STATUS_LABEL["wifi-only"] : sim.volte === "off" ? "IMS 음성 미등록·사용 불가" : "IMS 상태 확인 불가";
}
export function imsDetail(sim: SimInfo): string {
  const v = sim.ims;
  if (!v) return "상세 상태 미제공 — 통화는 별도 확인";
  const registration = { registered: "등록됨", registering: "등록 중", "not-registered": "미등록", unknown: "미확인" }[v.registration];
  const capability = (value: boolean | null) => value === true ? "가능" : value === false ? "불가" : "미확인";
  const transport = { cellular: "셀룰러", wifi: "Wi-Fi", other: "기타 망", unknown: "미확인" }[v.transport];
  const tech = { lte: "LTE", nr: "NR", iwlan: "IWLAN", "cross-sim": "다른 SIM 경유", "3g": "3G", unknown: "기술 미확인" }[v.technology];
  return `IMS ${registration} · 음성 ${capability(v.voice)} · SMS ${capability(v.sms)} · ${transport} / ${tech}`;
}

/** An enriched diagnostic result takes precedence over the legacy summary flag. */
export function cellularReady(sim: SimInfo | undefined): boolean {
  if (!sim) return false;
  return sim.ims ? sim.ims.status === "registered" && sim.ims.registration === "registered" && sim.ims.voice === true && sim.ims.transport === "cellular" : sim.volte === "on";
}
