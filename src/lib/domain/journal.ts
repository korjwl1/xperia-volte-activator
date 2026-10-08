import { MANUAL_IDS, MANUAL_TASK_IDS, STEP_KINDS, type RunJournal } from "$lib/types";
import { CALL_ITEMS } from "$lib/domain/communication";

const object = (value: unknown): value is Record<string, unknown> => !!value && typeof value === "object" && !Array.isArray(value);
const text = (value: unknown): value is string => typeof value === "string";
const texts = (value: unknown): value is string[] => Array.isArray(value) && value.every(text);
const integer = (value: unknown): value is number => typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
const option = (value: unknown) => object(value) && ["unroot", "relock", "restore"].every(key => typeof value[key] === "boolean")
  && (value.backupOnly === undefined || typeof value.backupOnly === "boolean")
  && (value.mode === undefined || ["automatic", "manual", "update"].includes(value.mode as string))
  && (value.manualTask === undefined || (value.mode === "manual" && MANUAL_TASK_IDS.includes(value.manualTask as never)))
  && (value.mode !== "manual" || value.manualTask !== undefined)
  && (value.backupOnly !== true || (!value.unroot && !value.relock && !value.restore));
const manualIds = new Set<string>(MANUAL_IDS);
const kinds = new Set<string>(STEP_KINDS);
const statuses = new Set(["pending", "running", "done", "failed", "skipped", "manual-wait"]);
const carriers = new Set(["SKT", "KT", "LGU", "LGU_V"]);

function diagnostic(value: unknown): boolean {
  if (!object(value)) return false;
  return ["no-sim", "sim-not-ready", "query-failed", "unsupported-format", "conflicting-evidence", "not-registered", "registering", "voice-unavailable", "registered", "wifi-only", "cross-sim", "other-network", "transport-unknown"].includes(value.status as string)
    && ["registered", "registering", "not-registered", "unknown"].includes(value.registration as string)
    && ["voice", "sms"].every(key => value[key] === null || typeof value[key] === "boolean")
    && ["cellular", "wifi", "other", "unknown"].includes(value.transport as string)
    && ["lte", "nr", "iwlan", "cross-sim", "3g", "unknown"].includes(value.technology as string);
}

function snapshot(value: unknown): boolean {
  if (value === null) return true;
  if (!object(value) || !["checkedAt", "model", "firmware", "fingerprint", "android", "baseband"].every(key => text(value[key]) && (value[key] as string).length <= 1024)
      || !Number.isFinite(Date.parse(value.checkedAt as string))
      || !["observed", "query-failed", "disconnected"].includes(value.outcome as string)
      || !Array.isArray(value.sims) || value.sims.length > 2 || !Array.isArray(value.presets) || value.presets.length > 2) return false;
  const sims = value.sims;
  const presets = value.presets;
  return sims.every(sim => object(sim) && [1, 2].includes(sim.slot as number)
      && ["physical", "esim", "unknown"].includes(sim.type as string) && (sim.carrier === null || text(sim.carrier)) && text(sim.state)
      && ["on", "off", "wifi", "unknown"].includes(sim.volte as string) && (sim.ims === undefined || diagnostic(sim.ims)))
    && new Set(sims.map(s => s.slot)).size === sims.length
    && presets.every(p => object(p) && [1, 2].includes(p.slot as number) && carriers.has(p.carrier as string)
      && text(p.version) && text(p.sha256) && /^[a-f0-9]{64}$/.test(p.sha256))
    && new Set(presets.map(p => p.slot)).size === presets.length;
}

function communication(value: unknown): boolean {
  if (value === undefined) return true; // old journals reopen final confirmation
  if (!object(value) || !snapshot(value.before) || !snapshot(value.latest) || !Array.isArray(value.calls) || value.calls.length > 2) return false;
  return value.calls.every(c => object(c) && [1, 2].includes(c.slot as number) && CALL_ITEMS.every(item => typeof c[item.id] === "boolean"))
    && new Set(value.calls.map(c => c.slot)).size === value.calls.length;
}

/** 디스크 JSON은 외부 입력이다. resume가 읽는 구조와 인덱스를 검사한다. */
export function decodeJournal(raw: string): RunJournal | null {
  try {
    const value: unknown = JSON.parse(raw);
    if (!object(value) || value.version !== 1
        || !["model", "productName", "serialMasked", "startedAt", "updatedAt", "backupPath", "firmwareDir"].every(key => text(value[key]))
        || !option(value.opts) || !texts(value.backupItems) || !integer(value.cursor)
        || !Array.isArray(value.steps) || !Array.isArray(value.runSteps) || value.runSteps.length === 0 || value.steps.length > 100
        || value.cursor > value.runSteps.length || (value.backupDir !== undefined && !text(value.backupDir))
        || (value.patchedImage !== undefined && !text(value.patchedImage))
        || ["imsUnverified", "imsVerified", "callVerified"].some(key => value[key] !== undefined && typeof value[key] !== "boolean")
        || !communication(value.communication)) return null;
    const config = value.config;
    if (!object(config) || !Array.isArray(config.sims) || config.sims.length !== 2
        || !(config.firmware === null || text(config.firmware))
        || ![null, "unlock", "relock"].includes(config.bootloaderAction as null | "unlock" | "relock")) return null;
    if (!config.sims.every(sim => object(sim) && [1, 2].includes(sim.slot as number)
        && (sim.carrier === null || carriers.has(sim.carrier as string)))
        || new Set(config.sims.map(sim => sim.slot)).size !== 2) return null;
    const definitions = new Map<string, Record<string, unknown>>();
    for (const step of value.steps) {
      if (!object(step) || !text(step.id) || definitions.has(step.id)
          || !text(step.kind) || !kinds.has(step.kind) || !text(step.title) || !text(step.desc)
          || !["safe", "warn", "danger"].includes(step.risk as string)
          || !["enabled", "optional", "wipe"].every(key => typeof step[key] === "boolean")
          || !integer(step.estSec) || (step.manual !== undefined && (!texts(step.manual) || !step.manual.every(id => manualIds.has(id))))) return null;
      definitions.set(step.id, step);
    }
    if ((value.opts as Record<string,unknown>).backupOnly === true && (value.steps.length!==1 || value.backupItems.length===0 || value.steps.some(step => step.id !== "backup" || step.kind!=="backup" || step.wipe || (step.manual ?? []).some((id: unknown)=>!["backup-notice", "usb-debug", "smsie-export"].includes(id as string))) || config.bootloaderAction!==null || config.firmware!==null || config.sims.some(sim=>sim.carrier!==null))) return null;
    const opts = value.opts as Record<string, unknown>;
    if (opts.mode === "update" && (opts.unroot || opts.relock || opts.restore || config.bootloaderAction !== null || config.sims.some(sim => sim.carrier !== null))) return null;
    if (opts.mode === "manual") {
      const allowed: Record<string, string[]> = {
        backup: ["backup"], restore: ["restore"], unlock: ["prep", "backup", "unlock", "setup-min"],
        relock: ["prep", "backup", "unroot", "relock", "setup-relock"], root: ["prep", "backup", "root"],
        unroot: ["prep", "backup", "unroot"], volte: ["efs-input", "backup", "efs-preflight", "efs", "verify", "volte-props", "final-verify"], verify: [],
      };
      if (opts.unroot || opts.relock || opts.restore || config.firmware !== null || value.steps.some(step => !allowed[opts.manualTask as string]?.includes(step.id))) return null;
    }
    const enabled = value.steps.filter(step => step.enabled);
    if (enabled.length !== value.runSteps.length) return null;
    for (const [index, step] of value.runSteps.entries()) {
      if (!object(step) || step.id !== enabled[index].id || !text(step.title)
          || !text(step.status) || !statuses.has(step.status) || !integer(step.manualDone)
          || typeof step.progress !== "number" || !Number.isFinite(step.progress) || step.progress < 0 || step.progress > 1
          || !texts(step.logs) || step.logs.length > 300
          || (step.communicationSkipped !== undefined && typeof step.communicationSkipped !== "boolean")
          || step.manualDone > ((definitions.get(step.id as string)?.manual as string[] | undefined)?.length ?? 0)) return null;
      if (step.sub !== undefined && (!object(step.sub) || !texts(step.sub.list)
          || !integer(step.sub.done) || step.sub.done > step.sub.list.length)) return null;
    }
    if (value.awaitingNext !== undefined && value.awaitingNext !== null &&
        (!text(value.awaitingNext) || value.cursor === 0 ||
          value.runSteps[value.cursor - 1]?.id !== value.awaitingNext ||
          !["done", "skipped"].includes(value.runSteps[value.cursor - 1]?.status))) return null;
    if (value.backupOmissions !== undefined) {
      const omissions = value.backupOmissions;
      if (!object(omissions) || typeof omissions.pending !== "boolean" || !Array.isArray(omissions.apps)
          || !omissions.apps.every(app => object(app) && text(app.package) && texts(app.reasons)
            && integer(app.removedFiles) && integer(app.removedBytes) && typeof app.cleanupPending === "boolean")) return null;
    }
    if (value.firmware !== null && (!object(value.firmware)
        || !["partition", "path", "version", "fingerprint"].every(key => text((value.firmware as Record<string, unknown>)[key]))
        || !integer(value.firmware.imageBytes) || !integer(value.firmware.downloadedBytes))) return null;
    if (value.stop !== null && (!object(value.stop) || !["stepId", "stepTitle", "reason", "at"].every(key => text((value.stop as Record<string, unknown>)[key])))) return null;
    if (value.sims !== undefined && (!Array.isArray(value.sims) || !value.sims.every(sim => object(sim)
        && [1, 2].includes(sim.slot as number) && (sim.carrier === null || text(sim.carrier)) && text(sim.state)))) return null;
    return value as unknown as RunJournal;
  } catch {
    return null;
  }
}
