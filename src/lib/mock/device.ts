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
  sims: [
    { slot: 1, type: "physical", carrier: null, volteEnabled: false },
    { slot: 2, type: "esim", carrier: "SK Telecom", volteEnabled: false },
  ],
  usb: {
    topology: "루트 허브 직결",
    controller: "Intel 칩셋 xHCI",
    linkSpeed: "SuperSpeed (5Gbps)",
  },
};

export const mockEnvChecks: EnvCheckItem[] = [
  { id: "webview2", label: "런타임", state: "pass", detail: "정상", fixable: false },
  { id: "drivers", label: "USB 드라이버", state: "pass", detail: "설치됨", fixable: false },
  { id: "presets", label: "VoLTE 프로파일", state: "warn", detail: "미지정 — VoLTE 작업 전 설정 필요", fixable: false },
];
