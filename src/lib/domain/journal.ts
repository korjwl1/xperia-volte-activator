import type { RunJournal } from "$lib/types";

const object = (value: unknown): value is Record<string, unknown> => !!value && typeof value === "object" && !Array.isArray(value);
const text = (value: unknown): value is string => typeof value === "string";
const texts = (value: unknown): value is string[] => Array.isArray(value) && value.every(text);
const integer = (value: unknown): value is number => typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
const option = (value: unknown) => object(value) && ["unroot", "relock", "restore"].every(key => typeof value[key] === "boolean");
const manualIds = new Set(["usb-debug", "su-grant", "magisk-patch", "oem-toggle", "mode-wait", "ims-check", "unlock-code", "firmware-select", "backup-notice", "flash-mode", "ims-precheck", "smsie-export", "smsie-import", "contacts-import"]);
const kinds = new Set(["backup", "unlock", "setup", "root", "efs-preflight", "efs", "verify", "volte-props", "fw-download", "fw-flash", "fw-verify", "unroot", "relock", "final-verify", "restore", "dexopt"]);
const statuses = new Set(["pending", "running", "done", "failed", "skipped", "manual-wait"]);
const carriers = new Set(["SKT", "KT", "LGU", "LGU_V"]);

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
        || (value.imsUnverified !== undefined && typeof value.imsUnverified !== "boolean")) return null;
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
    const enabled = value.steps.filter(step => step.enabled);
    if (enabled.length !== value.runSteps.length) return null;
    for (const [index, step] of value.runSteps.entries()) {
      if (!object(step) || step.id !== enabled[index].id || !text(step.title)
          || !text(step.status) || !statuses.has(step.status) || !integer(step.manualDone)
          || typeof step.progress !== "number" || !Number.isFinite(step.progress) || step.progress < 0 || step.progress > 1
          || !texts(step.logs) || step.logs.length > 300
          || step.manualDone > ((definitions.get(step.id as string)?.manual as string[] | undefined)?.length ?? 0)) return null;
      if (step.sub !== undefined && (!object(step.sub) || !texts(step.sub.list)
          || !integer(step.sub.done) || step.sub.done > step.sub.list.length)) return null;
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
