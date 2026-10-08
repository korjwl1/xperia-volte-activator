// @ts-nocheck
import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";
import tailwindcss from "@tailwindcss/vite";
// @ts-expect-error type error without @types/node package
import process from "node:process";
const host = process.env.TAURI_DEV_HOST;

// adb 질의는 전부 Tauri 백엔드(src-tauri/src/adb.rs)에서 수행한다.
// dev 미들웨어에는 adb 코드를 두지 않는다 — 브라우저 개발은 mock 폴백으로 동작한다.

// https://vite.dev/config/
export default defineConfig(() => ({
  plugins: [tailwindcss(), sveltekit()],

  clearScreen: false,
  // Tauri 모듈은 facade에서 동적 import한다. 개발 중 처음 import될 때 Vite가 의존성을 다시 묶으면 이미 열린 창의
  // 모듈 해시가 낡아 "Failed to fetch dynamically imported module"로 폴더 선택 창 등이 안 뜬다 — 시작할 때 미리 묶는다
  optimizeDeps: {
    include: ["@tauri-apps/api/core", "@tauri-apps/api/event", "@tauri-apps/api/window", "@tauri-apps/plugin-dialog", "@tauri-apps/plugin-notification"],
  },
  server: {
    port: 1420,
    strictPort: true,
    host: host || "127.0.0.1",
    hmr: host
      ? { protocol: "ws", host, port: 1421 }
      : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
}));
