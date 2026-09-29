// 백엔드 facade — 데스크톱(Tauri) Rust 명령 우선, 브라우저 개발은 mock 폴백
// adb 질의는 전부 백엔드(src-tauri/src/adb.rs)에서 수행 — 프론트/미들웨어에는 adb 코드 없음
// 실쓰기(백업/플래싱/EFS)는 항상 mock — 실기기에는 영향 없음
// 컴포넌트에서 @tauri-apps/api 직접 import 금지.

import type { AdbStatus, DeviceStatus, EnvCheckItem } from "$lib/types";
import { mockDeviceStatus, mockEnvChecks } from "$lib/mock/device";

export const USE_MOCK = true;

// ── Tauri 백엔드 경유 (데스크톱 빌드) ──
const inTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function invokeBackend<T>(cmd: string, args?: Record<string, unknown>): Promise<T | null> {
  if (!inTauri()) return null;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    return await invoke<T>(cmd, args ?? {});
  } catch {
    return null; // 명령 실패 → mock 폴백
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
  /** null = 백엔드 없음(브라우저 개발). available=false → 프론트에서 연결 재시도 팝업 */
  adbStatus(): Promise<AdbStatus | null>;
  openExternal(url: string): Promise<void>;
  envCheck(): Promise<EnvCheckItem[]>;
  envFix(id: string): Promise<{ ok: boolean; message: string }>;
}

const hybridApi: Api = {
  async deviceList() {
    if (inTauri()) {
      // 데스크톱: 백엔드 실측이 유일한 소스 — 실패/미연결 시 빈 목록 (mock으로 위장하지 않음)
      const rust = await invokeBackend<DeviceStatus[]>("device_list");
      return (rust ?? []).map(normalizeDevice);
    }
    // 브라우저 개발(백엔드 없음): mock
    return [mockDeviceStatus];
  },

  async deviceStatus(serial) {
    const devices = await this.deviceList();
    return devices.find((d) => d.serialMasked === serial || d.serial === serial) ?? null;
  },

  async storageSizes(): Promise<Record<string, number>> {
    const rust = await invokeBackend<Record<string, number>>("storage_sizes", { serial: "" });
    if (rust) return rust;
    return {};
  },

  async adbStatus() {
    return await invokeBackend<AdbStatus>("adb_status");
  },

  async openExternal(url) {
    if (inTauri()) {
      try {
        const { openUrl } = await import("@tauri-apps/plugin-opener");
        await openUrl(url);
        return;
      } catch {}
    }
    window.open(url, "_blank", "noopener");
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
