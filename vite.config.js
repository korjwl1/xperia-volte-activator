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
