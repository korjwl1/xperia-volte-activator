// 백엔드 facade — 데스크톱(Tauri) Rust 명령 우선, 브라우저 개발은 mock 폴백
// adb 질의는 전부 백엔드(src-tauri/src/adb.rs)에서 수행 — 프론트/미들웨어에는 adb 코드 없음
// 실쓰기(백업/플래싱/EFS)는 항상 mock — 실기기에는 영향 없음
// 컴포넌트에서 @tauri-apps/api 직접 import 금지.

import type { AdbStatus, AppItem, DeviceStatus, EnvCheckItem, SettingsOverview } from "$lib/types";
import { mockDeviceStatus, mockEnvChecks } from "$lib/mock/device";
import { classifyApp, SAMPLE_FLAGS, type AppFlag } from "$lib/data/appRules";

// ── Tauri 백엔드 경유 (데스크톱 빌드) ──
const inTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function invokeBackend<T>(cmd: string, args?: Record<string, unknown>): Promise<T | null> {
  if (!inTauri()) return null;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    return await invoke<T>(cmd, args ?? {});
  } catch {
    return null; // 명령 실패 — 호출부에서 실패로 처리 (mock으로 위장하지 않음)
  }
}

export interface Api {
  /** null = 조회 실패(일시적 오류 포함), [] = 연결된 기기 없음 */
  deviceList(): Promise<DeviceStatus[] | null>;
  deviceStatus(serial: string): Promise<DeviceStatus | null>;
  /** 백업 경로별 실측 용량(바이트), 측정 실패 시 null */
  storageSizes(serial?: string): Promise<Record<string, number> | null>;
  /** 설치된 3자 앱별 복구 가능성 (완전/불완전/불가), 조회 실패 시 null */
  appClasses(serial?: string): Promise<AppItem[] | null>;
  /** 설정 백업 개요(키 개수·자동 복원 대상 현재 값), 조회 실패 시 null */
  settingsOverview(serial?: string): Promise<SettingsOverview | null>;
  /** null = 백엔드 없음(브라우저 개발). available=false → 프론트에서 연결 재시도 팝업 */
  adbStatus(): Promise<AdbStatus | null>;
  openExternal(url: string): Promise<void>;
  /** 폴더 선택 다이얼로그 — 전체 경로 반환, 취소 시 null */
  pickFolder(): Promise<string | null>;
  /** 경로가 속한 PC 드라이브의 여유 공간(바이트), 조회 불가 시 null */
  diskFree(path: string): Promise<number | null>;
  envCheck(): Promise<EnvCheckItem[]>;
  envFix(id: string): Promise<{ ok: boolean; message: string }>;
}

const hybridApi: Api = {
  async deviceList() {
    if (inTauri()) {
      // 데스크톱: 백엔드 실측이 유일한 소스 — mock으로 위장하지 않음
      return await invokeBackend<DeviceStatus[]>("device_list");
    }
    // 브라우저 개발(백엔드 없음): mock
    return [mockDeviceStatus];
  },

  async deviceStatus(serial) {
    const devices = (await this.deviceList()) ?? [];
    return devices.find((d) => d.serial === serial) ?? null;
  },

  async pickFolder() {
    if (inTauri()) {
      try {
        const { open } = await import("@tauri-apps/plugin-dialog");
        const picked = await open({ directory: true, multiple: false, title: "백업 위치 선택" });
        return typeof picked === "string" ? picked : null;
      } catch {
        return null;
      }
    }
    // 브라우저 개발: 전체 경로를 알 수 없으므로 폴더 이름만
    const w = window as unknown as { showDirectoryPicker?: (opts: object) => Promise<FileSystemDirectoryHandle> };
    if (!w.showDirectoryPicker) return null;
    const handle = await w.showDirectoryPicker({ mode: "readwrite" }).catch(() => null);
    return handle?.name ?? null;
  },

  async diskFree(path) {
    if (!path.trim()) return null;
    return await invokeBackend<number>("disk_free", { path });
  },

  async storageSizes(serial) {
    // 브라우저 개발: 백엔드가 없으므로 측정 불가로 표시
    return await invokeBackend<Record<string, number>>("storage_sizes", { serial: serial ?? null });
  },

  async appClasses(serial) {
    if (!inTauri()) return SAMPLE_FLAGS.map(classifyApp); // 브라우저 개발: 샘플
    const flags = await invokeBackend<AppFlag[]>("app_flags", { serial: serial ?? null });
    return flags ? flags.map(classifyApp) : null;
  },

  async settingsOverview(serial) {
    return await invokeBackend<SettingsOverview>("settings_overview", { serial: serial ?? null });
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
