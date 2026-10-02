// 외부 링크 — 원본 CLI Config.json "urls" 계승
export const LINKS = {
  /** Sony 부트로더 언락 코드 발급 (구 developer.sony.com 주소는 여기로 리다이렉트 — 2026-10 확인) */
  unlock: "https://opendevices.sony.net/aosp-on-xperia-open-devices/get-started/unlock-bootloader",
} as const;

/** IMEI 마스킹 — 앞 2자리·뒤 4자리만 */
export function maskImei(imei: string): string {
  return imei.length < 8 ? "****" : imei.slice(0, 2) + "*".repeat(imei.length - 6) + imei.slice(-4);
}

/** 민감 문자열 마스킹 — 앞 3자만 남김 (원본 CLI log.maskStr 계승) */
export function maskSecret(text: string): string {
  return text.length <= 3 ? text : text.slice(0, 3) + "*".repeat(text.length - 3);
}
