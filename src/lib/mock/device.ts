// 기기/환경 mock — 실측 시드(tasks/recovery.md 1-3, 2026-09-29, XQ-DQ44)
import type { DeviceStatus, EnvCheckItem } from "$lib/types";

export const mockDeviceStatus: DeviceStatus = {
  serialMasked: "AB1234****",
  model: "XQ-DQ44",
  productName: "Xperia 1 V",
  firmware: "67.2.A.3.178",
  android: "15",
  mode: "android",
  bootloader: "locked",
  rooted: false,
  volte: {
    enabled: false,
    ims: "none",
    reason: "프레임워크가 SKT(450/05)를 IMS 미지원으로 처리 중 · EFS 주입 흔적 없음",
  },
  sims: [
    { slot: 1, carrier: "(비어 있음)", plmn: "-" },
    { slot: 2, carrier: "SK Telecom", plmn: "450/05" },
  ],
  usb: {
    topology: "루트 허브 직결",
    controller: "Intel 칩셋 xHCI",
    linkSpeed: "SuperSpeed (5Gbps)",
  },
};

export const mockEnvChecks: EnvCheckItem[] = [
  { id: "webview2", label: "WebView2 런타임", state: "pass", detail: "126.0.2592.61", fixable: false },
  { id: "drivers", label: "USB 드라이버 (adb/bootloader/DIAG)", state: "pass", detail: "드라이버 스토어 확인 완료 — PNPUTIL 전역 설치됨", fixable: false },
  { id: "adb", label: "adb 사이드카", state: "pass", detail: "번들 해시 일치 · 기존 adb 서버(5037) 충돌 없음", fixable: false },
  { id: "bundle", label: "libusb DLL / newflasher", state: "pass", detail: "번들 자산 해시 일치", fixable: false },
  { id: "magisk", label: "Magisk APK", state: "pass", detail: "30.7 (30700) 번들", fixable: false },
  { id: "presets", label: "EFS 프리셋 경로", state: "warn", detail: "미설정 — EFS 단계 전에 지정 필요 (로컬 경로 방식)", fixable: false },
  { id: "disk", label: "백업 대상 디스크 여유", state: "pass", detail: "112.4 GB 가용 (예상 25.9 GB)", fixable: false },
  { id: "qpst", label: "QPST", state: "info", detail: "미설치 (정상 — 폴백 트리거 전까지 설치 안 됨)", fixable: false },
];
