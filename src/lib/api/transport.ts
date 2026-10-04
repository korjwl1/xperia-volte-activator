import type { ApiResult, Unsubscribe } from "$lib/types";

export const inDesktop = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

interface BackendPort {
  available(): boolean;
  invoke<T>(command: string, args: Record<string, unknown>): Promise<T>;
  listen<T>(event: string, callback: (payload: T) => void): Promise<Unsubscribe>;
}

const desktopPort: BackendPort = {
  available: inDesktop,
  async invoke<T>(command: string, args: Record<string, unknown>) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<T>(command, args);
  },
  async listen<T>(event: string, callback: (payload: T) => void) {
    const { listen } = await import("@tauri-apps/api/event");
    return listen<T>(event, (e) => callback(e.payload));
  },
};

/** IPC는 여기에서만 처리한다. 질의 실패는 null, 작업 실패는 명시적인 결과로 반환한다. */
export function createTransport(port: BackendPort) {
  async function result<T>(command: string, args: Record<string, unknown> = {}): Promise<ApiResult<T>> {
    if (!port.available()) return { ok: false, error: "데스크톱 앱에서만 사용할 수 있습니다" };
    try {
      return { ok: true, value: await port.invoke<T>(command, args) };
    } catch (error) {
      return { ok: false, error: error instanceof Error ? error.message : String(error) };
    }
  }
  return {
    result,
    /** 구조화된 네이티브 오류를 해석하는 facade는 원래 오류 객체를 받는다. */
    async invoke<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
      if (!port.available()) throw new Error("데스크톱 앱에서만 사용할 수 있습니다");
      return port.invoke<T>(command, args);
    },
    async optional<T>(command: string, args?: Record<string, unknown>): Promise<T | null> {
      const response = await result<T>(command, args);
      return response.ok ? response.value : null;
    },
    async subscribe<T>(event: string, callback: (payload: T) => void): Promise<Unsubscribe> {
      if (!port.available()) return () => {};
      try {
        return await port.listen<T>(event, callback);
      } catch {
        return () => {};
      }
    },
  };
}

export const transport = createTransport(desktopPort);

/** async 등록 중 화면이 사라져도 이미 등록한/늦게 등록된 이벤트를 모두 해제한다. */
export async function observeDesktopWindow(
  onClose: () => boolean,
  onSessionEnd: (kind: string) => void,
): Promise<{ close(): Promise<void>; dispose: Unsubscribe } | null> {
  if (!inDesktop()) return null;
  const disposers: Unsubscribe[] = [];
  try {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    const win = getCurrentWindow();
    disposers.push(await transport.subscribe<string>("run-guard", onSessionEnd));
    disposers.push(await win.onCloseRequested((event) => {
      if (onClose()) event.preventDefault();
    }));
    return { close: () => win.destroy(), dispose: () => disposers.splice(0).forEach((off) => off()) };
  } catch {
    disposers.forEach((off) => off());
    return null;
  }
}
