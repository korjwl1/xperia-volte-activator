// @ts-nocheck
import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";
import tailwindcss from "@tailwindcss/vite";
// @ts-expect-error type error without @types/node package
import process from "node:process";
// @ts-expect-error type error without @types/node package
import { exec as _exec } from "node:child_process";
// @ts-expect-error type error without @types/node package
import { promisify } from "node:util";
const exec = promisify(_exec);
const host = process.env.TAURI_DEV_HOST;

// ── 개발용 기기 조회 미들웨어 ──
// adb를 통해 실제 연결된 기기 정보를 읽기 전용으로 제공
// (실쓰기는 불가 — 프론트 mock 단계에서 실기기 감지만 제공)
async function getProp(serial, prop) {
  try {
    const { stdout } = await exec(`adb -s ${serial} shell getprop ${prop}`, { timeout: 3000 });
    return stdout.trim();
  } catch { return ""; }
}

async function getConnectedDevices() {
  try {
    const { stdout } = await exec("adb devices", { timeout: 3000 });
    const lines = stdout.trim().split("\n").slice(1);
    const devices = [];
    for (const line of lines) {
      const [serial, state] = line.trim().split("\t");
      if (state !== "device") continue;
      const [model, productName, fw, android, locked, simState, sim1Alpha, sim2Alpha, sim1Num, sim2Num, volteAvail] =
        await Promise.all([
          getProp(serial, "ro.product.model"),
          getProp(serial, "ro.product.vendor.device"),
          getProp(serial, "ro.build.display.id"),
          getProp(serial, "ro.build.version.release"),
          getProp(serial, "ro.boot.flash.locked"),
          getProp(serial, "gsm.sim.state"),
          getProp(serial, "gsm.sim.operator.alpha"),
          getProp(serial, "gsm.sim.operator.alpha.2"),
          getProp(serial, "gsm.sim.operator.numeric"),
          getProp(serial, "gsm.sim.operator.numeric.2"),
          getProp(serial, "persist.dbg.volte_avail_ovr"),
        ]);
      // SIM 파싱
      const states = simState.split(",");
      const alphas = [sim1Alpha, sim2Alpha];
      const numerics = [sim1Num, sim2Num];
      const sims = [1, 2].map((slot) => {
        const idx = slot - 1;
        const loaded = states[idx]?.trim() === "LOADED";
        const carrier = loaded ? alphas[idx] || "" : null;
        const plmn = numerics[idx] || "";
        return {
          slot,
          type: slot === 1 ? "physical" : "esim", // XQ-DQ44 가정 — devices.json에서 모델별 매핑
          carrier,
          volteEnabled: volteAvail === "1" && loaded,
          _plmn: plmn, // 내부 판별용
        };
      });

      // 루팅 체크
      let rooted = false;
      try {
        const { stdout: su } = await exec(`adb -s ${serial} shell which su`, { timeout: 2000 });
        rooted = su.trim().length > 0;
      } catch {}

      // 시리얼 마스킹
      const serialMasked = serial.length > 4 ? serial.slice(0, 4) + "****" : serial;

      // 제품명 매핑 (임시 — devices.json으로 이전 예정)
      const nameMap = {
        "XQ-DQ44": "Xperia 1 V", "XQ-DQ72": "Xperia 1 V", "XQ-DQ54": "Xperia 1 V",
        "XQ-DE44": "Xperia 5 V", "XQ-DE54": "Xperia 5 V",
        "XQ-DX72": "Xperia 1 VI", "XQ-EC72": "Xperia 10 VI",
      };

      devices.push({
        serial,
        serialMasked,
        model,
        productName: nameMap[model] || model,
        firmware: fw,
        android,
        mode: "android",
        bootloader: locked === "1" ? "locked" : locked === "0" ? "unlocked" : "unknown",
        rooted,
        sims,
        usb: { topology: "", controller: "", linkSpeed: "" }, // 실구현에서 SetupAPI로 취득
      });
    }
    return devices;
  } catch (e) {
    return [];
  }
}

function devDeviceApi() {
  return {
    name: "dev-device-api",
    configureServer(server) {
      server.middlewares.use("/api/dev/devices", async (req, res) => {
        try {
          const devices = await getConnectedDevices();
      // 백업 경로별 실제 용량 쿼리
      if (req.url.includes("/storage")) {
        const serial = devices[0]?.serial ?? "";
        const paths = [
          { id: "dcim", path: "/sdcard/DCIM" },
          { id: "download", path: "/sdcard/Download" },
          { id: "pictures", path: "/sdcard/Pictures" },
          { id: "movies", path: "/sdcard/Movies" },
          { id: "music", path: "/sdcard/Music" },
          { id: "documents", path: "/sdcard/Documents" },
          { id: "perfectviewer", path: "/sdcard/PerfectViewer" },
          { id: "dxo", path: "/sdcard/DxO ONE" },
          { id: "kakao-media", path: "/sdcard/Android/data/com.kakao.talk" },
          { id: "android-data", path: "/sdcard/Android/data" },
          { id: "sdcard-total", path: "/sdcard" },
        ];
        const sizes = {};
        for (const p of paths) {
          try {
            const { stdout } = await exec(`adb -s ${serial} shell du -sk "${p.path}"`, { timeout: 5000 });
            const kb = parseInt(stdout.trim().split("\t")[0]) || 0;
            sizes[p.id] = kb * 1024;
          } catch { sizes[p.id] = 0; }
        }

        // 앱 APK 용량 측정
        try {
          const { stdout: pkgList } = await exec(`adb -s ${serial} shell pm list packages -3`, { timeout: 5000 });
          const pkgs = pkgList.trim().split("\n").map((l) => l.replace("package:", "").trim()).filter(Boolean);
          let totalApkBytes = 0;
          const apkSizes = {};
          for (const pkg of pkgs.slice(0, 30)) { // 상위 30개만 (전체는 오래 걸림)
            try {
              const { stdout: pathOut } = await exec(`adb -s ${serial} shell pm path ${pkg}`, { timeout: 3000 });
              const apkPath = pathOut.trim().split("\n")[0]?.replace("package:", "").trim();
              if (apkPath) {
                const { stdout: sizeOut } = await exec(`adb -s ${serial} shell stat -c %s "${apkPath}"`, { timeout: 3000 });
                const bytes = parseInt(sizeOut.trim()) || 0;
                apkSizes[pkg] = bytes;
                totalApkBytes += bytes;
              }
            } catch {}
          }
          sizes["apk-total"] = totalApkBytes;
          sizes["apk-count"] = pkgs.length;
          sizes["apk-sampled"] = Math.min(30, pkgs.length);
        } catch { sizes["apk-total"] = 0; }

        res.setHeader("Content-Type", "application/json");
        res.end(JSON.stringify(sizes));
        return;
      }

      res.setHeader("Content-Type", "application/json");
          res.setHeader("Access-Control-Allow-Origin", "*");
          res.end(JSON.stringify(devices));
        } catch (e) {
          res.statusCode = 500;
          res.end(JSON.stringify({ error: e.message }));
        }
      });
    },
  };
}

// https://vite.dev/config/
export default defineConfig(() => ({
  plugins: [tailwindcss(), sveltekit(), devDeviceApi()],

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
