// 백엔드 facade — 개발 단계: 실기기 감지 우선 + mock 폴백
// 실쓰기(백업/플래싱/EFS)는 항상 mock — 실기기에는 영향 없음
// 컴포넌트에서 @tauri-apps/api 직접 import 금지.

import type { DeviceStatus, EnvCheckItem } from "$lib/types";
import { mockDeviceStatus, mockEnvChecks } from "$lib/mock/device";

export const USE_MOCK = true;

// ── 실기기 감지 (개발 서버 미들웨어 경유) ──
async function fetchRealDevices(): Promise<DeviceStatus[]> {
  try {
    const res = await fetch("/api/dev/devices", { signal: AbortSignal.timeout(5000) });
    if (!res.ok) return [];
    const data = await res.json();
    return Array.isArray(data) ? data : [];
  } catch {
    return []; // adb 없음, 서버 응답 없음 등 → mock 폴백
  }
}

// ── SIM 타입 정규화 (실기기 _plmn 기반) ──
function normalizeDevice(d: DeviceStatus): DeviceStatus {
  return {
    ...d,
    sims: d.sims.map((s) => ({
      ...s,
      type: s.slot === 1 ? "physical" : "esim", // XQ-DQ44 가정 — devices.json 확장 시 모델별 매핑
    })),
  };
}

export interface Api {
  deviceList(): Promise<DeviceStatus[]>;
  deviceStatus(serial: string): Promise<DeviceStatus | null>;
  storageSizes(): Promise<Record<string, number>>;
  envCheck(): Promise<EnvCheckItem[]>;
  envFix(id: string): Promise<{ ok: boolean; message: string }>;
}

const hybridApi: Api = {
  async deviceList() {
    const real = await fetchRealDevices();
    if (real.length > 0) return real.map(normalizeDevice);
    // 실기기 없음 → mock 반환 (개발용)
    return [mockDeviceStatus];
  },

  async deviceStatus(serial) {
    const devices = await this.deviceList();
    return devices.find((d) => d.serialMasked === serial || d.serial === serial) ?? null;
  },

  async storageSizes(): Promise<Record<string, number>> {
    try {
      const res = await fetch("/api/dev/devices?storage", { signal: AbortSignal.timeout(10000) });
      if (res.ok) {
        const data = await res.json();
        if (Object.keys(data).some((k) => data[k] > 0)) return data;
      }
    } catch {}
    return {};
  },

  async envCheck() {
    // 환경 체크는 로컬 상태라 실기기와 무관 — mock 유지
    return mockEnvChecks;
  },

  async envFix(id) {
    return { ok: true, message: `(mock) 수정: ${id}` };
  },
};

export const api: Api = hybridApi;
