// 외부 링크 — 원본 CLI Config.json "urls" 계승
export const LINKS = {
  /** Sony 부트로더 언락 코드 발급 */
  unlock: "https://developer.sony.com/open-source/aosp-on-xperia-open-devices/get-started/unlock-bootloader#toggleSelect-device",
} as const;

/** 민감 문자열 마스킹 — 앞 3자만 남김 (원본 CLI log.maskStr 계승) */
export function maskSecret(text: string): string {
  return text.length <= 3 ? text : text.slice(0, 3) + "*".repeat(text.length - 3);
}
