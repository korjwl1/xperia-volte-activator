// 백엔드 facade — 데스크톱(Tauri) Rust 명령 우선, 브라우저 개발은 mock 폴백
// adb 질의는 전부 백엔드(src-tauri/src/adb.rs)에서 수행 — 프론트/미들웨어에는 adb 코드 없음
// 단계별 실행 플래그는 data/runMode.ts에서 관리. fastboot 쓰기/재부팅은 facade에서도 차단.
// 컴포넌트에서 @tauri-apps/api 직접 import 금지.

import type { MagiskPatchRequest, FirmwareDirInfo, AdbStatus, AppItem, BackupProgress, BackupSummary, PreparedBackup, DeviceStatus, EnvCheckItem, FastbootVars, FirmwareResult, FirmwareVersions, MagiskPrepared, PatchResult, RestoreOutcome, RelockGate, SettingsOverview, SmsIeOutcome, UnlockResult } from "$lib/types";
import { mockDeviceStatus, mockEnvChecks } from "$lib/mock/device";
import type { ApiResult, FirmwarePackageReport } from "$lib/types";
import { REAL_STEPS } from "$lib/data/runMode";
import { canReboot, type EngineCapabilities } from "$lib/domain/execution";
import { classifyApp, SAMPLE_FLAGS, type AppFlag } from "$lib/data/appRules";
import { efsApi, type EfsApi } from "./efs";
import { rootToolsApi, type RootToolsApi } from "./rootTools";
import { inDesktop as inTauri, transport } from "./transport";
export { inDesktop, observeDesktopWindow } from "./transport";

// ── Tauri 백엔드 경유 (데스크톱 빌드) ──
const invokeBackend = transport.optional;
const invokeResult = transport.result;

export interface Api extends EfsApi, RootToolsApi {
  /** PC-only inspection of a local full firmware package. Never returns write authorization. */
  firmwarePackageInspect(dir: string, targetFingerprint: string): Promise<ApiResult<FirmwarePackageReport>>;
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
  /** 부트로더 모드 폰에 Windows fastboot 드라이버가 없으면 Sony 공식 드라이버를 받아 연결(UAC 한 번) — fastboot 실전에서만 */
  fastbootDriverEnsure(productName: string): Promise<ApiResult<string>>;
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
  /** PC 환경 점검(읽기 전용) — 조회 실패 시 null(이전 결과를 유지하고 단정하지 않음). 자동 수정(설치)은 하지 않는다 */
  envCheck(): Promise<EnvCheckItem[] | null>;
  /** 작업 진행 기록 저장 (기기별, 앱 데이터 폴더) */
  journalSave(key: string, data: string): Promise<boolean>;
  /** 기록 없음은 ok(null), 읽기 실패는 error — 실패를 덮어쓰지 않는다. */
  journalLoad(key: string): Promise<ApiResult<string | null>>;
  /** 진행 기록 보관 — done(끝남) / discarded(새로 시작) */
  journalArchive(key: string, tag: "done" | "discarded"): Promise<boolean>;
  flashHistoryArchive(confirm: boolean): Promise<ApiResult<string>>;
  /** 루트 권한 승인 여부 (su -c id = uid=0), 조회 실패 시 null */
  rootCheck(serial?: string): Promise<boolean | null>;
  /** 화면 켜짐·잠금 상태 (읽기 전용) */
  screenState(serial?: string): Promise<ApiResult<{ awake: boolean; locked: boolean }>>;
  /** 화면 켜기(WAKEUP 키 — 끄지 않음, 잠금은 풀지 않음) — 루팅·복원 실전에서만 */
  screenWake(serial?: string): Promise<ApiResult<null>>;
  /** 직접 지정한 펌웨어 폴더 검사 — <partition>_*.sin 존재 + 부트 이미지 추출 가능 */
  firmwareDirCheck(dir: string, partition: string): Promise<ApiResult<FirmwareDirInfo>>;
  /** 작업 중 PC 보호 — 절전 방지 + Windows 종료 방지 (작업 중에만 켬) */
  runGuard(active: boolean, reason?: string): Promise<boolean>;
  /** 연락처 복원 확인 — 백업한 연락처 수와 지금 폰의 연락처 수, 조회 실패 시 null */
  contactsRestoreCheck(serial: string | undefined, dir: string): Promise<{ backedUp: number; onDevice: number } | null>;
  contactsRestoreFinish(serial: string | undefined, dir: string): Promise<ApiResult<null>>;
  engineCapabilities(): Promise<ApiResult<EngineCapabilities>>;
  /** 백업 시작 — 지정 폴더 아래 시작 시각 기준 폴더 생성, 절대 경로 반환 */
  /** 저장 위치의 xva-<모델>-backup 폴더 준비 — existing이면 같은 폰의 기존 백업(바뀐 파일만 갱신) */
  backupPrepare(serial: string | undefined, dest: string, backupOnly?: boolean): Promise<ApiResult<PreparedBackup>>;
  /** 백업 실행(자동 항목) — 진행은 onBackupProgress로. 실패 시 error 문구 */
  backupRun(serial: string | undefined, items: string[], dest: string, runId: string, resumeDir?: string, backupOnly?: boolean): Promise<ApiResult<BackupSummary>>;
  /** 백업 취소 요청 — runId가 있으면 그 실행만(시작 전이면 시작 즉시 멈춤), 없으면 지금 실행 중인 백업 */
  backupCancel(runId?: string): Promise<void>;
  /** 백업 폴더 삭제(완료 화면, 사용자 확인 후) — 이 앱이 만든 backup-* 폴더만. 목 모드는 아무것도 지우지 않음 */
  backupDelete(dir: string, backupOnly?: boolean): Promise<ApiResult<null>>;
  /** 기존 백업 폴더 완결 검사(파괴 단계 게이트용) — 폴더가 없으면 null */
  backupManifestCheck(dir: string, runId?: string): Promise<BackupSummary | null>;
  /** SMS Import/Export 설치·권한·임시 폴더 준비 — 로그 문구 목록 반환 */
  smsiePrepare(serial: string | undefined, download: boolean, backupOnly?: boolean): Promise<ApiResult<string[]>>;
  /** SMS Import/Export 산출물 수집 — ready=false면 앱에서 아직 내보내지 않음 */
  smsieCollect(serial: string | undefined, backupDir: string, confirmComplete?: boolean, backupOnly?: boolean): Promise<ApiResult<SmsIeOutcome>>;
  smsieProbe(serial: string | undefined, backupDir: string, backupOnly?: boolean): Promise<ApiResult<boolean>>;
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
  /** 리락 요청 — 순정 출처·현재 펌웨어·동일 기기 및 양 슬롯 이력 조건 검사 */
  fastbootLock(confirm: boolean, partition: string, stockPath: string, expectedSerial: string): Promise<ApiResult<UnlockResult>>;
  /** 리락 이력 진단(읽기 전용) — 실제 명령은 별도 확인·기기 조건을 재검사 */
  relockGateCheck(partition: string, stockPath: string, deviceKey?: string): Promise<ApiResult<RelockGate>>;
  /** fastboot 재부팅 — os | bootloader (OKAY 확인 시 성공, 실패 시 백엔드 오류 문구) */
  fastbootReboot(target: "os" | "bootloader" | "fastboot", expectedSerial: string): Promise<ApiResult<null>>;
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
  /** Magisk 앱 삭제(언루팅 뒤) — 지웠으면 true, 설치돼 있지 않으면 false */
  magiskUninstall(serial: string | undefined): Promise<ApiResult<boolean>>;
  /** adb 재부팅 — os | bootloader (root-write 또는 fastboot-write 게이트) */
  rootReboot(serial: string | undefined, target: "os" | "bootloader" | "fastboot"): Promise<ApiResult<null>>;
  /** Magisk 패치 로그 이벤트 구독 */
  onMagiskLog(cb: (line: string) => void): Promise<() => void>;
}

const hybridApi: Api = {
  async firmwarePackageInspect(dir, targetFingerprint) {
    return invokeResult<FirmwarePackageReport>("firmware_package_inspect", { dir, targetFingerprint });
  },
  async engineCapabilities() { return invokeResult<EngineCapabilities>("engine_capabilities", {}); },
  ...efsApi,
  ...rootToolsApi,
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

  async fastbootDriverEnsure(productName) {
    if (!REAL_STEPS.fastboot) return { ok: false, error: "fastboot 실전이 비활성화되어 있습니다" };
    return await invokeResult<string>("fastboot_driver_ensure", { productName });
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
    if (!inTauri()) return { ok: true, value: null };
    return await invokeResult<string | null>("journal_load", { key });
  },

  async journalArchive(key, tag) {
    return (await invokeResult<null>("journal_archive", { key, tag })).ok;
  },

  async flashHistoryArchive(confirm) {
    return invokeResult<string>("flash_history_archive", { confirm });
  },

  async rootCheck(serial) {
    return await invokeBackend<boolean>("root_check", { serial: serial ?? null });
  },

  async screenState(serial) {
    return await invokeResult<{ awake: boolean; locked: boolean }>("screen_state", { serial: serial ?? null });
  },

  async screenWake(serial) {
    // 루트 승인·문자 복원처럼 폰 화면 조작이 필요한 실전 단계에서만
    if (!REAL_STEPS.root && !REAL_STEPS.restore) return { ok: false, error: "실전 실행이 비활성화되어 있습니다" };
    return await invokeResult<null>("screen_wake", { serial: serial ?? null });
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
  async contactsRestoreFinish(serial, dir) {
    if (!REAL_STEPS.restore) return { ok: false, error: "실전 복원이 비활성화되어 있습니다" };
    return invokeResult<null>("contacts_restore_finish", { serial: serial ?? null, dir });
  },

  async backupPrepare(serial, dest, backupOnly = false) {
    if (!REAL_STEPS.backup && !backupOnly) return { ok: false, error: "실전 백업이 비활성화되어 있습니다" };
    return await invokeResult<PreparedBackup>("backup_prepare", { serial: serial ?? null, dest });
  },

  async backupRun(serial, items, dest, runId, resumeDir, backupOnly = false) {
    if (!REAL_STEPS.backup && !backupOnly) return { ok: false, error: "실전 백업이 비활성화되어 있습니다" };
    return await invokeResult<BackupSummary>("backup_run", { serial: serial ?? null, items, dest, resumeDir: resumeDir || null, runId });
  },

  async backupCancel(runId) {
    await invokeBackend<null>("backup_cancel", { runId: runId ?? null });
  },

  async backupDelete(dir, backupOnly = false) {
    if (!REAL_STEPS.backup && !backupOnly) return { ok: true, value: null };
    return await invokeResult<null>("backup_delete", { dir });
  },

  async backupManifestCheck(dir,runId) {
    const result=await invokeResult<BackupSummary>("backup_manifest_check", { dir, runId: runId ?? null });
    if(!result.ok) throw new Error(result.error);
    return result.value;
  },

  async smsiePrepare(serial, download, backupOnly = false) {
    if (!REAL_STEPS.backup && !backupOnly) return { ok: false, error: "실전 백업이 비활성화되어 있습니다" };
    return await invokeResult<string[]>("smsie_prepare", { serial: serial ?? null, download });
  },

  async smsieCollect(serial, backupDir, confirmComplete = false, backupOnly = false) {
    if (!REAL_STEPS.backup && !backupOnly) return { ok: false, error: "실전 백업이 비활성화되어 있습니다" };
    return await invokeResult<SmsIeOutcome>("smsie_collect", { serial: serial ?? null, backupDir, confirmComplete });
  },

  async smsieProbe(serial, backupDir, backupOnly = false) {
    if (!REAL_STEPS.backup && !backupOnly) return { ok: false, error: "실전 백업이 비활성화되어 있습니다" };
    return await invokeResult<boolean>("smsie_probe", { serial: serial ?? null, backupDir });
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
    if (!REAL_STEPS.fastboot || !REAL_STEPS.relock) return { ok: false, error: "리락 실전 실행이 비활성화되어 있습니다" };
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

  async magiskUninstall(serial) {
    if (!REAL_STEPS.root) return { ok: false, error: "루팅 실전 실행이 비활성화되어 있습니다" };
    return await invokeResult<boolean>("magisk_uninstall", { serial: serial ?? null });
  },

  async magiskInstall(serial, apkPath, apkSha256) {
    if (!REAL_STEPS.root) return { ok: false, error: "루팅 실전 실행이 비활성화되어 있습니다" };
    return await invokeResult<null>("magisk_install", { serial: serial ?? null, apkPath, apkSha256 });
  },

  async rootReboot(serial, target) {
    // 언락·리락(부트로더)·루팅·언루팅(fastbootd) 진입·복귀는 root/fastboot, 최종 확인(verify)은 OS 재부팅만 허용
    if (!canReboot(REAL_STEPS, target)) {
      return { ok: false, error: "기기 재부팅 실전 실행이 비활성화되어 있습니다" };
    }
    return await invokeResult<null>("root_reboot", { serial: serial ?? null, target });
  },

  async onMagiskLog(cb) {
    return transport.subscribe<string>("magisk:log", cb);
  },

  async openExternal(url) {
    if (inTauri()) {
      // 동적 import 없이 기존 통신 경로로 opener 명령을 부른다 — 개발 모드 새로고침 뒤 모듈 로딩이 깨져
      // 조용히 아무 일도 안 하던 문제를 피한다. 앱 창에서는 window.open이 열리지 않으므로 대체하지 않는다.
      const r = await invokeResult<null>("plugin:opener|open_url", { url });
      if (!r.ok) console.warn(`[외부 열기 실패] ${url} — ${r.error}`);
      return;
    }
    if (typeof window !== "undefined") window.open(url, "_blank", "noopener");
  },

  async envCheck() {
    // 데스크톱: PC 실측(드라이버 등, 읽기 전용). 브라우저 개발은 mock
    if (inTauri()) return await invokeBackend<EnvCheckItem[]>("env_check");
    return mockEnvChecks;
  },
};

export const api: Api = hybridApi;
