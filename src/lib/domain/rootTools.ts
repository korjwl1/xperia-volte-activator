import type { ApiResult, RootImportedImage, RootImagePort } from "$lib/types";

export function requireResult<T>(result: ApiResult<T>): T {
  if (!result.ok) throw new Error(result.error);
  return result.value;
}
/** Sequential read polling: never race a pending native call against a timer. */
export async function waitForFastboot(api: RootImagePort, timeoutMs = 90000): Promise<void> {
  const until = Date.now() + timeoutMs;
  do {
    const vars = await api.fastbootGetvar();
    if (vars && vars["is-userspace"] === "yes") return;
    await new Promise(resolve => setTimeout(resolve, 1000));
  } while (Date.now() < until);
  throw new Error("fastbootd 연결을 확인하지 못했습니다. 기기 모드를 확인한 뒤 수동으로 다시 진행하세요");
}
/** Caller enters fastbootd explicitly. This function never unlocks, relocks or retries a write. */
export async function flashRootImage(api: RootImagePort, serial: string, image: RootImportedImage): Promise<void> {
  const vars = await api.fastbootGetvar();
  if (!vars || vars["is-userspace"] !== "yes" || vars.unlocked !== "yes") throw new Error("언락된 fastbootd 기기가 필요합니다");
  requireResult(await api.fastbootFlash(image.partition, image.path, true, serial, image.sha256));
  requireResult(await api.fastbootReboot("os", serial));
}
export const ROOT_MODULE_CHOICES = [
  { id: "overlayfs", name: "OverlayFS MetaModule", note: "KernelSU 계열의 첫 단계 · 공식 GitHub" },
  { id: "neozygisk", name: "NeoZygisk", note: "Zygisk 구현체 · ReZygisk/Next와 택일" },
  { id: "rezygisk", name: "ReZygisk", note: "Zygisk 구현체 · NeoZygisk/Next와 택일" },
  { id: "zygisk-next", name: "Zygisk Next", note: "Zygisk 구현체 · Shamiko 조합" },
  { id: "play-integrity-fork", name: "PlayIntegrityFork", note: "Integrity Box와 택일" },
  { id: "integrity-box", name: "Integrity Box", note: "PlayIntegrityFork와 택일" },
  { id: "tricky-store", name: "TrickyStore", note: "TrickyAddon보다 먼저 설치" },
  { id: "tricky-addon", name: "TrickyAddon", note: "설치 후 WebUI에서 직접 설정" },
  { id: "hma", name: "HMA-OSS Zygisk", note: "설치 후 동봉 카페 프리셋 그대로 가져오기" },
  { id: "bootloop-protector", name: "AshReXcue", note: "부트루프 보호 · 한국어 빌드 동봉" },
  { id: "play-store-fix", name: "PlayStoreFix", note: "선택 항목 · 사용자모임 빌드 동봉" },
  { id: "zygisk-assistant", name: "Zygisk Assistant", note: "선택 항목" },
  { id: "shamiko", name: "Shamiko", note: "Magisk + Zygisk Next 전용" },
] as const;
