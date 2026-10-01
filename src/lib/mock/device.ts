// 기기/환경 mock — 실측 시드(tasks/recovery.md 1-3, 2026-09-29, XQ-DQ44)
import type { DeviceStatus, EnvCheckItem } from "$lib/types";

export const mockDeviceStatus: DeviceStatus = {
  state: "device",
  serialMasked: "AB1234****",
  model: "XQ-DQ44",
  productName: "Xperia 1 V",
  firmware: "67.2.A.3.178",
  android: "15",
  mode: "android",
  bootloader: "locked",
  rooted: false,
  sims: [
    { slot: 1, type: "physical", carrier: null, state: "ABSENT", volte: "unknown" },
    { slot: 2, type: "esim", carrier: "SK Telecom", state: "LOADED", volte: "off" },
  ],
  usb: {
    topology: "루트 허브 직결",
    controller: "Intel 칩셋 xHCI",
    linkSpeed: "SuperSpeed (5Gbps)",
  },
};

export const mockEnvChecks: EnvCheckItem[] = [];
