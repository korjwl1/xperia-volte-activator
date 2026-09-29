// 백엔드 facade — mock↔실전 전환 지점 (AGENTS.md 규칙 2)
// 실전 구현은 .plans/02-contracts/tauri-commands.md 계약을 따른다 (M2+).
// 컴포넌트에서 @tauri-apps/api 직접 import 금지.

import type { DeviceStatus, EnvCheckItem } from "$lib/types";
import { mockDeviceStatus, mockEnvChecks } from "$lib/mock/device";

export const USE_MOCK = true;

export interface Api {
  deviceList(): Promise<unknown[]>;
  deviceStatus(serial: string): Promise<DeviceStatus>;
  envCheck(): Promise<EnvCheckItem[]>;
  envFix(id: string): Promise<{ ok: boolean; message: string }>;
}

const mockApi: Api = {
  async deviceList() {
    return [{ serial: mockDeviceStatus.serialMasked, mode: mockDeviceStatus.mode }];
  },
  async deviceStatus() {
    // 실전: 프롭/프로브 통합 질의 (§4) — mock은 실측 스냅샷 반환
    await new Promise((r) => setTimeout(r, 250));
    return mockDeviceStatus;
  },
  async envCheck() {
    await new Promise((r) => setTimeout(r, 200));
    return mockEnvChecks;
  },
  async envFix(id: string) {
    // 실전: WebView2 부트스트래퍼 / PNPUTIL 상승 등 (§12.6)
    return { ok: true, message: `(mock) 자동 수리 실행: ${id}` };
  },
};

// 실전 구현 자리 (M2+):
// const realApi: Api = { ... invoke('device_list') ... };
// 단, USE_MOCK=true 동안은 @tauri-apps/api를 로드하지 않는다 (브라우저 dev 호환).

export const api: Api = USE_MOCK ? mockApi : (undefined as unknown as Api);
