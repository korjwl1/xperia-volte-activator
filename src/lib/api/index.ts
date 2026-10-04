// 백엔드 facade — 데스크톱(Tauri) Rust 명령 우선, 브라우저 개발은 mock 폴백
// adb 질의는 전부 백엔드(src-tauri/src/adb.rs)에서 수행 — 프론트/미들웨어에는 adb 코드 없음
// 단계별 실행 플래그는 data/runMode.ts에서 관리. fastboot 쓰기/재부팅은 facade에서도 차단.
// 컴포넌트에서 @tauri-apps/api 직접 import 금지.

import type { MagiskPatchRequest, FirmwareDirInfo, AdbStatus, AppItem, BackupProgress, BackupSummary, DeviceStatus, EnvCheckItem, FastbootVars, FirmwareResult, FirmwareVersions, MagiskPrepared, PatchResult, RestoreOutcome, RelockGate, SettingsOverview, SmsIeOutcome, UnlockResult } from "$lib/types";
import { mockDeviceStatus, mockEnvChecks } from "$lib/mock/device";
import type { ApiResult } from "$lib/types";
import { REAL_STEPS } from "$lib/data/runMode";
import { classifyApp, SAMPLE_FLAGS, type AppFlag } from "$lib/data/appRules";
import { inDesktop as inTauri, transport } from "./transport";
export { inDesktop, observeDesktopWindow } from "./transport";

// ── Tauri 백엔드 경유 (데스크톱 빌드) ──
const invokeBackend = transport.optional;
const invokeResult = transport.result;

export interface Api {
  /** null = 조회 실패(일시적 오류 포함), [] = 연결된 기기 없음 */
  deviceList(): Promise<DeviceStatus[] | null>;
  deviceStatus(serial: string): Promise<DeviceStatus | null>;
  /** 백업 경로별 실측 용량(바이트), 측정 실패 시 null */
  storageSizes(serial?: string): Promise<Record<string, number> | null>;
  /** 설치된 3자 앱별 복구 가능성 (완전/불완전/불가), 조회 실패 시 null */
  appClasses(serial?: string): Promise<AppItem[] | null>;
  /** IMEI 1 (전체 값 — UI에는 마스킹, 복사 버튼에만 사용), 실패 시 null */
  readImei1(serial?: string): Promise<string | null>;
  /** 기기와 같은 순정 펌웨어에서 부트 이미지만 받아 저장 (기본 앱 데이터 폴더, dest 지정 시 그 폴더). 공간 부족 오류는 "NO_SPACE|" 접두어 */
  firmwareFetch(serial: string | undefined, partition: string, version?: string, dest?: string): Promise<ApiResult<FirmwareResult>>;
  /** 서버 펌웨어 버전 목록(설치된 버전 이상), 실패 시 null */
  firmwareVersions(serial?: string): Promise<FirmwareVersions | null>;
  /** 연결된 Sony 기기의 USB 모드 (android / fastboot / flashmode / other), 실패 시 null */
  usbModes(): Promise<{ mode: string; vendorId: number; productId: number }[] | null>;
  /** 폰에 설정 화면 띄우기 — developer: 개발자 옵션 / about: 휴대전화 정보 */
  openSettingsScreen(serial: string | undefined, screen: "developer" | "about"): Promise<boolean>;
  /** 클립보드 복사 */
  copyText(text: string): Promise<boolean>;
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
  /** 작업 진행 기록 저장 (기기별, 앱 데이터 폴더) */
  journalSave(key: string, data: string): Promise<boolean>;
  /** 끝나지 않은 진행 기록 (없거나 실패 시 null) */
  journalLoad(key: string): Promise<string | null>;
  /** 진행 기록 보관 — done(끝남) / discarded(새로 시작) */
  journalArchive(key: string, tag: "done" | "discarded"): Promise<boolean>;
  /** 루트 권한 승인 여부 (su -c id = uid=0), 조회 실패 시 null */
  rootCheck(serial?: string): Promise<boolean | null>;
  /** 직접 지정한 펌웨어 폴더 검사 — <partition>_*.sin 존재 + 부트 이미지 추출 가능 */
  firmwareDirCheck(dir: string, partition: string): Promise<ApiResult<FirmwareDirInfo>>;
  /** 작업 중 PC 보호 — 절전 방지 + Windows 종료 방지 (작업 중에만 켬) */
  runGuard(active: boolean, reason?: string): Promise<boolean>;
  /** 연락처 복원 확인 — 백업한 연락처 수와 지금 폰의 연락처 수, 조회 실패 시 null */
  contactsRestoreCheck(serial: string | undefined, dir: string): Promise<{ backedUp: number; onDevice: number } | null>;
  /** 백업 시작 — 지정 폴더 아래 시작 시각 기준 폴더 생성, 절대 경로 반환 */
  backupPrepare(serial: string | undefined, dest: string): Promise<ApiResult<string>>;
  /** 백업 실행(자동 항목) — 진행은 onBackupProgress로. 실패 시 error 문구 */
  backupRun(serial: string | undefined, items: string[], dest: string, runId: string, resumeDir?: string): Promise<ApiResult<BackupSummary>>;
  /** 백업 취소 요청 — runId가 있으면 그 실행만(시작 전이면 시작 즉시 멈춤), 없으면 지금 실행 중인 백업 */
  backupCancel(runId?: string): Promise<void>;
  /** 백업 폴더 삭제(완료 화면, 사용자 확인 후) — 이 앱이 만든 backup-* 폴더만. 목 모드는 아무것도 지우지 않음 */
  backupDelete(dir: string): Promise<ApiResult<null>>;
  /** 기존 백업 폴더 완결 검사(파괴 단계 게이트용) — 폴더가 없으면 null */
  backupManifestCheck(dir: string): Promise<BackupSummary | null>;
  /** SMS Import/Export 설치·권한·임시 폴더 준비 — 로그 문구 목록 반환 */
  smsiePrepare(serial: string | undefined, download: boolean): Promise<ApiResult<string[]>>;
  /** SMS Import/Export 산출물 수집 — ready=false면 앱에서 아직 내보내지 않음 */
  smsieCollect(serial: string | undefined, backupDir: string): Promise<SmsIeOutcome | null>;
  /** 복구 실행(APK·파일 tar 스트리밍·설정·연락처 전송) — 진행은 onRestoreProgress */
  restoreRun(serial: string | undefined, dir: string, items: string[]): Promise<ApiResult<RestoreOutcome>>;
  /** 문자·통화 기록 수동 복원 준비 — 파일 전송 + 기본 문자 앱 역할 (안내 문구 반환) */
  smsieRestoreStage(serial: string | undefined, dir: string, items: string[]): Promise<ApiResult<string>>;
  /** 문자·통화 기록 수동 복원 마무리 — 기본 문자 앱 원복·임시 정리 (안내 로그 반환) */
  smsieRestoreFinish(serial: string | undefined): Promise<ApiResult<string[]>>;
  /** 백엔드 이벤트 구독 — unlisten 반환 (데스크톱 전용, 브라우저 dev은 즉시 no-op) */
  onBackupProgress(cb: (p: BackupProgress) => void): Promise<() => void>;
  onRestoreProgress(cb: (p: BackupProgress) => void): Promise<() => void>;
  /** fastboot getvar:all — 읽기 전용 프로브 (unlocked·슬롯·헬스), 실패·미설치 시 null */
  fastbootGetvar(): Promise<FastbootVars | null>;
  /** 부트로더 언락 — oem unlock 0x{code} 후 getvar 이중 확인 (로그에 코드 마스킹) */
  fastbootUnlock(code: string, confirm: boolean, expectedSerial: string): Promise<ApiResult<UnlockResult>>;
  /** 리락 요청 — 순정 출처·AVB·전체 체인 검증 전까지 백엔드가 항상 거부 */
  fastbootLock(confirm: boolean, partition: string, stockPath: string, expectedSerial: string): Promise<ApiResult<UnlockResult>>;
  /** 리락 이력 진단(읽기 전용) — 진단 정상도 실제 리락을 허용하지 않음 */
  relockGateCheck(partition: string, stockPath: string, deviceKey?: string): Promise<ApiResult<RelockGate>>;
  /** fastboot 재부팅 — os | bootloader (OKAY 확인 시 성공, 실패 시 백엔드 오류 문구) */
  fastbootReboot(target: "os" | "bootloader", expectedSerial: string): Promise<ApiResult<null>>;
  /** fastboot 로그 이벤트 구독 (INFO 프레임·명령·민감값 마스킹) */
  onFastbootLog(cb: (line: string) => void): Promise<() => void>;
  /** fastboot 파티션 기록 — download → flash <partition>_a/_b (fastboot-write 게이트) */
  fastbootFlash(partition: string, path: string, confirm: boolean, expectedSerial: string, expectedSha256: string): Promise<ApiResult<null>>;
  /** 현재 기기의 펌웨어 지문·부트 이미지 검사 후 SHA-256 반환 */
  bootImageCheck(serial: string, path: string, fingerprint: string): Promise<ApiResult<string>>;
  /** Magisk 최신 APK 확보(GitHub·캐시) — 기기 무관 준비 단계 */
  magiskPrepare(): Promise<ApiResult<MagiskPrepared>>;
  /** 부트 패치 — 스테이징·boot_patch.sh·검증(ANDROID!·크기·해시)·pull·정리 (root-write 게이트). partition은 결과명에 사용 */
  magiskPatch(request: MagiskPatchRequest): Promise<ApiResult<PatchResult>>;
  /** Magisk 앱 설치 (root-write 게이트) */
  magiskInstall(serial: string | undefined, apkPath: string, apkSha256: string): Promise<ApiResult<null>>;
  /** adb 재부팅 — os | bootloader (root-write 또는 fastboot-write 게이트) */
  rootReboot(serial: string | undefined, target: "os" | "bootloader"): Promise<ApiResult<null>>;
  /** Magisk 패치 로그 이벤트 구독 */
  onMagiskLog(cb: (line: string) => void): Promise<() => void>;
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
    if (typeof window === "undefined") return null;
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

  async readImei1(serial) {
    return await invokeBackend<string>("read_imei1", { serial: serial ?? null });
  },

  async firmwareFetch(serial, partition, version, dest) {
    return await invokeResult<FirmwareResult>("firmware_fetch", { serial: serial ?? null, partition, version: version ?? null, dest: dest || null });
  },

  async firmwareVersions(serial) {
    return await invokeBackend<FirmwareVersions>("firmware_versions", { serial: serial ?? null });
  },

  async usbModes() {
    return await invokeBackend<{ mode: string; vendorId: number; productId: number }[]>("usb_modes");
  },

  async openSettingsScreen(serial, screen) {
    return (await invokeResult<null>("open_settings_screen", { serial: serial ?? null, screen })).ok;
  },

  async copyText(text) {
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch {
      return false;
    }
  },

  async settingsOverview(serial) {
    return await invokeBackend<SettingsOverview>("settings_overview", { serial: serial ?? null });
  },

  async adbStatus() {
    return await invokeBackend<AdbStatus>("adb_status");
  },

  async journalSave(key, data) {
    return (await invokeResult<null>("journal_save", { key, data })).ok;
  },

  async journalLoad(key) {
    return await invokeBackend<string>("journal_load", { key });
  },

  async journalArchive(key, tag) {
    return (await invokeResult<null>("journal_archive", { key, tag })).ok;
  },

  async rootCheck(serial) {
    return await invokeBackend<boolean>("root_check", { serial: serial ?? null });
  },

  async firmwareDirCheck(dir, partition) {
    return await invokeResult<FirmwareDirInfo>("firmware_dir_check", { dir, partition });
  },

  async runGuard(active, reason) {
    return (await invokeResult<null>("run_guard", { active, reason: reason ?? null })).ok;
  },

  async contactsRestoreCheck(serial, dir) {
    return await invokeBackend<{ backedUp: number; onDevice: number }>("contacts_restore_check", { serial: serial ?? null, dir });
  },

  async backupPrepare(serial, dest) {
    if (!REAL_STEPS.backup) return { ok: false, error: "실전 백업이 비활성화되어 있습니다" };
    return await invokeResult<string>("backup_prepare", { serial: serial ?? null, dest });
  },

  async backupRun(serial, items, dest, runId, resumeDir) {
    if (!REAL_STEPS.backup) return { ok: false, error: "실전 백업이 비활성화되어 있습니다" };
    return await invokeResult<BackupSummary>("backup_run", { serial: serial ?? null, items, dest, resumeDir: resumeDir || null, runId });
  },

  async backupCancel(runId) {
    await invokeBackend<null>("backup_cancel", { runId: runId ?? null });
  },

  async backupDelete(dir) {
    if (!REAL_STEPS.backup) return { ok: true, value: null };
    return await invokeResult<null>("backup_delete", { dir });
  },

  async backupManifestCheck(dir) {
    return await invokeBackend<BackupSummary>("backup_manifest_check", { dir });
  },

  async smsiePrepare(serial, download) {
    if (!REAL_STEPS.backup) return { ok: false, error: "실전 백업이 비활성화되어 있습니다" };
    return await invokeResult<string[]>("smsie_prepare", { serial: serial ?? null, download });
  },

  async smsieCollect(serial, backupDir) {
    if (!REAL_STEPS.backup) return null;
    return await invokeBackend<SmsIeOutcome>("smsie_collect", { serial: serial ?? null, backupDir });
  },

  async onBackupProgress(cb) {
    return transport.subscribe<BackupProgress>("backup:progress", cb);
  },

  async restoreRun(serial, dir, items) {
    if (!REAL_STEPS.restore) return { ok: false, error: "실전 복구가 비활성화되어 있습니다" };
    return await invokeResult<RestoreOutcome>("restore_run", { serial: serial ?? null, dir, items });
  },

  async smsieRestoreStage(serial, dir, items) {
    if (!REAL_STEPS.restore) return { ok: false, error: "실전 복구가 비활성화되어 있습니다" };
    return await invokeResult<string>("smsie_restore_stage", { serial: serial ?? null, dir, items });
  },

  async smsieRestoreFinish(serial) {
    if (!REAL_STEPS.restore) return { ok: false, error: "실전 복구가 비활성화되어 있습니다" };
    return await invokeResult<string[]>("smsie_restore_finish", { serial: serial ?? null });
  },

  async onRestoreProgress(cb) {
    return transport.subscribe<BackupProgress>("restore:progress", cb);
  },

  async fastbootGetvar() {
    return await invokeBackend<FastbootVars>("fastboot_getvar");
  },

  async fastbootUnlock(code, confirm, expectedSerial) {
    if (!REAL_STEPS.fastboot) return { ok: false, error: "fastboot 실전 실행이 비활성화되어 있습니다" };
    return await invokeResult<UnlockResult>("fastboot_unlock", { code, confirm, expectedSerial });
  },

  async fastbootLock(confirm, partition, stockPath, expectedSerial) {
    if (!REAL_STEPS.fastboot) return { ok: false, error: "fastboot 실전 실행이 비활성화되어 있습니다" };
    return await invokeResult<UnlockResult>("fastboot_lock", { confirm, partition, stockPath, expectedSerial });
  },

  async relockGateCheck(partition, stockPath, deviceKey) {
    return await invokeResult<RelockGate>("relock_gate_check", { partition, stockPath, deviceKey: deviceKey ?? null });
  },

  async fastbootReboot(target, expectedSerial) {
    if (!REAL_STEPS.fastboot) return { ok: false, error: "fastboot 실전 실행이 비활성화되어 있습니다" };
    return await invokeResult<null>("fastboot_reboot", { target, expectedSerial });
  },

  async onFastbootLog(cb) {
    return transport.subscribe<string>("fastboot:log", cb);
  },

  async fastbootFlash(partition, path, confirm, expectedSerial, expectedSha256) {
    if (!REAL_STEPS.fastboot) return { ok: false, error: "fastboot 실전 실행이 비활성화되어 있습니다" };
    return await invokeResult<null>("fastboot_flash", { partition, path, confirm, expectedSerial, expectedSha256 });
  },

  async bootImageCheck(serial, path, fingerprint) {
    return await invokeResult<string>("boot_image_check", { serial, path, fingerprint });
  },

  async magiskPrepare() {
    return await invokeResult<MagiskPrepared>("magisk_prepare");
  },

  async magiskPatch(request) {
    if (!REAL_STEPS.root) return { ok: false, error: "루팅 실전 실행이 비활성화되어 있습니다" };
    return await invokeResult<PatchResult>("magisk_patch", { request });
  },

  async magiskInstall(serial, apkPath, apkSha256) {
    if (!REAL_STEPS.root) return { ok: false, error: "루팅 실전 실행이 비활성화되어 있습니다" };
    return await invokeResult<null>("magisk_install", { serial: serial ?? null, apkPath, apkSha256 });
  },

  async rootReboot(serial, target) {
    // 언락·리락(fastboot) 흐름도 부트로더 진입에 adb 재부팅을 쓴다 — 두 엔진 중 하나만 켜져도 허용
    if (!REAL_STEPS.root && !REAL_STEPS.fastboot) {
      return { ok: false, error: "루팅/fastboot 실전 실행이 비활성화되어 있습니다" };
    }
    return await invokeResult<null>("root_reboot", { serial: serial ?? null, target });
  },

  async onMagiskLog(cb) {
    return transport.subscribe<string>("magisk:log", cb);
  },

  async openExternal(url) {
    if (inTauri()) {
      try {
        const { openUrl } = await import("@tauri-apps/plugin-opener");
        await openUrl(url);
        return;
      } catch {}
    }
    if (typeof window !== "undefined") window.open(url, "_blank", "noopener");
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
