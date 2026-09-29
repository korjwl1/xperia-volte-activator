// 백업 항목 mock — 실측 시드(tasks/recovery.md 2-3, 2026-09-29 스캔 기반 대표 추출)
import type { AppItem, BackupGroup } from "$lib/types";

export const mockApps: AppItem[] = [
  // ❌ 앱 데이터 직접 복구 불가 (allowBackup=false 또는 기기 바인딩)
  { pkg: "com.kakao.talk", label: "카카오톡", cls: "none", note: "채팅 백업(구글 드라이브)+비밀번호 필요 · 받은 미디어는 숨은 영역 백업으로 보존" },
  { pkg: "com.shinhan.sbanking", label: "신한 쏠", cls: "none", note: "공동인증서/금융인증서 사전 내보내기 + 기기 재등록" },
  { pkg: "com.wooribank.smart.npib", label: "우리WON뱅킹", cls: "none", note: "인증서 내보내기 필요" },
  { pkg: "com.kakaobank.channel", label: "카카오뱅크", cls: "none", note: "재인증 필요" },
  { pkg: "com.google.android.apps.authenticator2", label: "Google Authenticator", cls: "none", note: "⚠ 계정 동기화 확인 필수 — 꺼져 있으면 OTP 소실" },
  { pkg: "kr.co.tmoney.tia", label: "티머니 GO", cls: "none", note: "잔액은 계정에 존재, 카드 재등록" },
  { pkg: "com.ahnlab.v3mobileplus", label: "V3 Mobile+", cls: "none", note: "재설치 후 재진단" },
  { pkg: "jp.co.ponos.battlecatskr", label: "통키(배틀캣츠)", cls: "none", note: "⚠ 인계코드/계정 연동 확인 — 미발급 시 진행도 소실" },
  { pkg: "com.naverfin.payapp", label: "네이버페이", cls: "none", note: "재로그인" },
  // ⚠️ 불완전 (플래그 OK지만 기기재등록/재페어링 또는 플래그 NO지만 계정 동기화 존재)
  { pkg: "viva.republica.toss", label: "토스", cls: "partial", note: "데이터 일부 복원되나 기기변경 보안 절차·간편비밀번호 재설정" },
  { pkg: "org.mozilla.firefox", label: "Firefox", cls: "partial", note: "Firefox 계정 동기화로 북마크·비밀번호 복구" },
  { pkg: "com.vivaldi.browser", label: "Vivaldi", cls: "partial", note: "계정 동기화" },
  { pkg: "com.adobe.lrmobile", label: "Lightroom", cls: "partial", note: "Adobe 클라우드 동기화 — 로컬 캐시 손실" },
  { pkg: "com.fitbit.FitbitMobile", label: "Fitbit", cls: "partial", note: "재로그인 + 기기 재페어링" },
  { pkg: "com.scee.psxandroid", label: "PS App", cls: "partial", note: "재로그인" },
  { pkg: "com.skt.tmap.ku", label: "T map", cls: "partial", note: "계정 즐겨찾기 복구, 오프라인 지도 재다운로드" },
  { pkg: "com.nhn.android.band", label: "밴드", cls: "partial", note: "네이버 계정 재로그인" },
  // ✅ 완전 (플래그 OK + 계정/로컬 연속성)
  { pkg: "com.instagram.android", label: "인스타그램", cls: "full", note: "재로그인만" },
  { pkg: "com.flyersoft.moonreader", label: "Moon+ Reader", cls: "full", note: "진행도·책 파일 위치 유지 시 연속" },
  { pkg: "com.rookiestudio.perfectviewer", label: "Perfect Viewer", cls: "full", note: "/sdcard/PerfectViewer 폴더 백업 시 거의 완전" },
  { pkg: "com.newin.nplayer.pro", label: "nPlayer", cls: "full", note: "" },
  { pkg: "com.voyagerx.scanner", label: "vFlat 스캐너", cls: "full", note: "" },
  { pkg: "com.friendscube.somoim", label: "소모임", cls: "full", note: "" },
  { pkg: "com.google.android.keep", label: "Google Keep", cls: "full", note: "구글 계정 동기화" },
  { pkg: "com.google.android.apps.docs", label: "Google Drive", cls: "full", note: "구글 계정 동기화" },
];

const GB = 1024 ** 3;

export const mockBackupGroups: BackupGroup[] = [
  {
    id: "settings",
    label: "시스템 설정",
    desc: "화면 설정, 알림 설정 등",
    items: [
      { id: "qs-tiles", label: "상단 타일 순서 (15개)", cls: "full", checked: true },
      { id: "display", label: "밝기·타임아웃·폰트 크기", cls: "full", checked: true },
      { id: "ime", label: "기본 키보드 (ESTMob)", cls: "full", checked: true },
      { id: "idle-whitelist", label: "배터리 최적화 예외 앱 목록", cls: "full", checked: true },
    ],
  },
  {
    id: "apps",
    label: "앱 (설치 목록 + 데이터)",
    desc: "B_OK 앱은 구글 백업으로 일부 복원 · 아래 목록의 데이터는 개별 채널",
    items: mockApps.map((a) => ({
      id: `app:${a.pkg}`,
      label: `${a.label} (${a.pkg})`,
      cls: a.cls,
      note: a.note,
      checked: a.cls !== "none",
    })),
  },
  {
    id: "storage",
    label: "본체 저장소 (/sdcard)",
    desc: "adb pull -a · 타임스탬프 보존",
    items: [
      { id: "dcim", label: "DCIM (사진/영상)", cls: "full", checked: true, bytes: 18.2 * GB },
      { id: "download", label: "Download", cls: "full", checked: true, bytes: 3.1 * GB },
      { id: "pictures", label: "Pictures (스크린샷 등)", cls: "full", checked: true, bytes: 1.4 * GB },
      { id: "perfectviewer", label: "PerfectViewer 라이브러리", cls: "full", checked: true, bytes: 620 * 1024 ** 2 },
      { id: "dxo", label: "DxO ONE", cls: "full", checked: true, bytes: 340 * 1024 ** 2 },
    ],
  },
  {
    id: "hidden",
    label: "숨은 영역 (Android/data)",
    desc: "MTP로는 보이지 않는 영역 — 카톡 받은 미디어 포함",
    items: [
      { id: "kakao-media", label: "카카오톡 받은 미디어 (Android/data/com.kakao.talk)", cls: "full", checked: true, bytes: 2.1 * GB },
      { id: "others", label: "기타 Android/data 선택 항목", cls: "partial", checked: true, bytes: 480 * 1024 ** 2 },
    ],
  },
  {
    id: "sms",
    label: "SMS / 통화기록",
    desc: "PC에 저장 후 복원합니다",
    items: [{ id: "sms", label: "문자 메시지 + 통화 기록", cls: "full", checked: true, bytes: 5 * 1024 ** 2 }],
  },
];

export const mockGoogleBackupAge = "3일 전";
export const mockDiskFree = 112.4 * GB;
