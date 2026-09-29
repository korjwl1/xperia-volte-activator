// 도메인 타입 — .plans/03-data/mock-schema.md 참조
export type DeviceMode = "android" | "bootloader-fastboot" | "fastbootd" | "flashmode";

export type TriState = boolean | "unknown";

export interface SimInfo {
  slot: 1 | 2;
  carrier: string;
  plmn: string; // 예: 45005 (SKT)
}

export interface UsbInfo {
  topology: string; // "루트 허브 직결" | "외부 허브 경유(1단계)" ...
  controller: string; // "Intel 칩셋(xHCI)" | "ASMedia ..." — §10-5
  linkSpeed: string; // "SuperSpeed (5Gbps)" | "High-Speed (480Mbps)"
}

export interface DeviceStatus {
  serialMasked: string;
  model: string;
  productName: string;
  firmware: string;
  android: string;
  mode: DeviceMode;
  bootloader: "locked" | "unlocked" | "unknown";
  rooted: TriState;
  volte: { enabled: boolean; ims: "registered" | "none" | "unknown"; reason?: string };
  sims: SimInfo[];
  usb: UsbInfo;
}

export interface EnvCheckItem {
  id: string;
  label: string;
  state: "pass" | "warn" | "fail" | "info";
  detail: string;
  fixable: boolean;
}

export type Profile = "clean-return" | "keep-root" | "unroot-only";

export type StepKind =
  | "backup" | "unlock" | "setup" | "root" | "efs-preflight" | "efs" | "verify"
  | "unroot" | "backup2" | "relock" | "final-verify" | "restore" | "dexopt";

export type ManualId =
  | "usb-debug" | "su-grant" | "magisk-patch" | "oem-toggle" | "mode-wait" | "ims-check";

export interface PlanStep {
  id: string;
  kind: StepKind;
  title: string;
  desc: string;
  optional: boolean;
  enabled: boolean; // 선택 단계의 토글 상태
  risk: "safe" | "warn" | "danger";
  wipe: boolean; // 데이터 초기화 발생
  estSec: number;
  manual?: ManualId;
}

export type BackupClass = "full" | "partial" | "none";

export interface BackupItem {
  id: string;
  label: string;
  cls: BackupClass;
  note?: string;
  checked: boolean;
  bytes?: number;
}

export interface BackupGroup {
  id: string;
  label: string;
  desc: string;
  items: BackupItem[];
}

export interface AppItem {
  pkg: string;
  label: string;
  cls: BackupClass;
  note: string;
}

export type RunStatus =
  | "pending" | "running" | "done" | "failed" | "skipped" | "manual-wait";

export interface RunStep {
  id: string;
  title: string;
  status: RunStatus;
  progress: number; // 0..1
  logs: string[];
}

export const RISK_LABEL: Record<PlanStep["risk"], string> = {
  safe: "안전",
  warn: "주의",
  danger: "위험",
};

export const CLS_LABEL: Record<BackupClass, string> = {
  full: "완전 복구",
  partial: "불완전 (재로그인 등 필요)",
  none: "앱 데이터 직접 복구 불가",
};
