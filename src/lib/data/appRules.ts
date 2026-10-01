// 앱 백업·복구 분류 규칙 — 어느 폰에서나 같은 기준 (AI·추정 없음)
//
// 1) 기기 실측 신호 (모든 앱에 적용)
//    - /sdcard/Android/data/<pkg> 존재 → 이 프로그램이 백업·복원하는 데이터가 있음 → "restored"
//    - 없음 → 앱(APK)만 다시 설치됨 → "relogin" (계정에 저장된 데이터는 다시 로그인하면 돌아옴)
//    - dumpsys backup의 구글 백업 기록(state bytes > 0) → 초기 설정 때 구글 백업 복원으로 일부 데이터가 돌아옴
//      (ALLOW_BACKUP 플래그는 '자격'일 뿐이라 안내에 쓰지 않음 — 실제 백업 기록만 사용)
// ※ 앱별 백업·복원 동작을 모아 둔 공개 데이터베이스는 없음 (2026-10 조사) — 그래서 실측 신호가 기본
// 2) 앱별 특수 규칙 (KNOWN_APPS) — 특정 폰 기준이 아니라 "이 앱이면 어느 폰에서나 해당"하는 사실만 기록
//    - lost: 계정에 저장되지 않아 앱에서 직접 옮기지 않으면 사라지는 데이터가 있음 → 실측 신호보다 우선
//    - note: 추가로 필요한 사전 조치
//    목록에 없는 앱은 1)만 적용되며, 이름 대신 패키지명으로 표시된다.

import type { AppItem } from "$lib/types";

export interface AppFlag {
  pkg: string;
  allowBackup: boolean;
  hasExternalData: boolean;
  googleBackedUp: boolean;
}

interface KnownApp {
  label: string;
  lost?: boolean;
  note?: string;
}

const CERT_NOTE = "공동인증서·금융인증서를 미리 내보내 두세요 — 초기화 후 기기 재등록이 필요합니다";

const OTP_NOTE = "OTP가 기기에만 저장돼 있을 수 있습니다 — 앱의 내보내기·계정 이전 기능으로 미리 옮겨 두세요";

export const KNOWN_APPS: Record<string, KnownApp> = {
  // 데이터가 사라질 수 있는 앱 (미리 직접 이전 필요) — OTP 앱은 어느 폰에서나 동일
  "com.google.android.apps.authenticator2": {
    label: "Google OTP (Authenticator)",
    lost: true,
    note: "OTP가 기기에만 저장돼 있을 수 있습니다 — 앱의 계정 이전 또는 Google 계정 동기화를 미리 확인하세요",
  },
  "com.azure.authenticator": { label: "Microsoft Authenticator", lost: true, note: OTP_NOTE },
  "com.authy.authy": { label: "Twilio Authy", lost: true, note: OTP_NOTE },
  "com.beemdevelopment.aegis": { label: "Aegis Authenticator", lost: true, note: OTP_NOTE },
  "com.twofasapp": { label: "2FAS Authenticator", lost: true, note: OTP_NOTE },
  "org.fedorahosted.freeotp": { label: "FreeOTP", lost: true, note: OTP_NOTE },
  "com.duosecurity.duomobile": { label: "Duo Mobile", lost: true, note: OTP_NOTE },
  "com.lastpass.authenticator": { label: "LastPass Authenticator", lost: true, note: OTP_NOTE },
  "jp.co.ponos.battlecatskr": {
    label: "냥코 대전쟁",
    lost: true,
    note: "게임 진행은 앱에서 기종 변경 코드를 미리 발급해야 이어집니다",
  },
  // 추가 조치가 필요한 앱
  // 메신저 — 채팅 기록은 앱 자체 백업이 필요
  "com.kakao.talk": { label: "카카오톡", note: "채팅 내용은 카카오톡 '채팅 백업'을 미리 해 둬야 복원됩니다" },
  "jp.naver.line.android": { label: "LINE", note: "대화 내용은 LINE '대화 백업'을 미리 해 둬야 복원됩니다" },
  "com.whatsapp": { label: "WhatsApp", note: "대화 내용은 WhatsApp '채팅 백업'(Google 드라이브)을 미리 해 둬야 복원됩니다" },
  "com.shinhan.sbanking": { label: "신한 SOL뱅크", note: CERT_NOTE },
  "com.wooribank.smart.npib": { label: "우리WON뱅킹", note: CERT_NOTE },
  "com.kakaobank.channel": { label: "카카오뱅크", note: "초기화 후 기기 재등록과 본인 인증이 필요합니다" },
  "viva.republica.toss": { label: "토스", note: "기기 변경 인증 후 간편 비밀번호를 다시 설정해야 합니다" },
  "kr.co.tmoney.tia": { label: "티머니GO", note: "잔액은 계정에 남아 있으며, 교통카드를 다시 등록해야 합니다" },
  // 이름 표시용
  "com.naverfin.payapp": { label: "네이버페이" },
  "com.nhn.android.band": { label: "밴드" },
  "com.nhn.android.navercafe": { label: "네이버 카페" },
  "com.nhn.android.nmap": { label: "네이버 지도" },
  "net.daum.android.map": { label: "카카오맵" },
  "com.skt.tmap.ku": { label: "TMAP" },
  "com.instagram.android": { label: "인스타그램" },
  "com.Slack": { label: "Slack" },
  "org.mozilla.firefox": { label: "Firefox" },
  "com.vivaldi.browser": { label: "Vivaldi" },
  "com.google.android.keep": { label: "Google Keep" },
  "com.google.android.apps.docs": { label: "Google 드라이브" },
};

export function classifyApp(f: AppFlag): AppItem {
  const k = KNOWN_APPS[f.pkg];
  const base = {
    pkg: f.pkg,
    label: k?.label ?? f.pkg,
    hasExternalData: f.hasExternalData,
    allowBackup: f.allowBackup,
    googleBackedUp: f.googleBackedUp,
  };
  if (k?.lost) return { ...base, recovery: "lost", note: k.note ?? "" };
  if (f.hasExternalData) {
    return { ...base, recovery: "restored", note: k?.note ?? "앱 재설치 + 앱 외부 데이터(Android/data) 복원 — 다시 로그인이 필요할 수 있습니다" };
  }
  return {
    ...base,
    recovery: "relogin",
    note:
      k?.note ??
      (f.googleBackedUp
        ? "앱만 다시 설치됩니다 — 구글 백업 기록이 있어, 초기 설정 때 같은 구글 계정으로 복원하면 일부 데이터가 돌아옵니다"
        : "앱만 다시 설치됩니다 — 기기에만 저장된 데이터는 복원되지 않습니다"),
  };
}

/** 브라우저 개발용 샘플 플래그 (데스크톱은 실측 app_flags만 사용) */
export const SAMPLE_FLAGS: AppFlag[] = [
  { pkg: "com.kakao.talk", allowBackup: false, hasExternalData: true, googleBackedUp: false },
  { pkg: "com.google.android.apps.authenticator2", allowBackup: false, hasExternalData: false, googleBackedUp: false },
  { pkg: "com.shinhan.sbanking", allowBackup: false, hasExternalData: false, googleBackedUp: false },
  { pkg: "com.naverfin.payapp", allowBackup: false, hasExternalData: false, googleBackedUp: false },
  { pkg: "com.instagram.android", allowBackup: true, hasExternalData: true, googleBackedUp: false },
  { pkg: "viva.republica.toss", allowBackup: true, hasExternalData: false, googleBackedUp: true },
];
