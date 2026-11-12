/**
 * Protocol settings of connectors (docs/spec/configuration.md section 7), both ways. The
 * console models the HL7 acknowledgement mode, the ASTM link role, the AU instrument sheet and,
 * for delimited files, the whole settings object as JSON text (`templateJson`). Everything else
 * a protocol has (timeouts, identities, charsets) stays as it was.
 */

import type { AstmSettings, Hl7Settings } from "@/ipc/generated";
import { clone } from "@/lib/clone";
import type { AckMode, ConfigIssue, ConnectorConfig, ConnectorSettings, Protocol } from "../types";
import { auFromWire, auToWire } from "./auConfig";
import { isEqual, isRecord } from "./util";
import type { WireSettings } from "./wire";

/** Where problems found while converting are reported, at console paths. */
export interface Issues {
  add(path: string, message: string): void;
}

export function issueList(): Issues & { list: ConfigIssue[] } {
  const list: ConfigIssue[] = [];
  return {
    list,
    add: (path, message) => {
      list.push({ severity: "error", path, message });
    },
  };
}

/** The settings keys the console models, checked against the generated protocol settings. */
const ACK_MODE = "ack_mode" satisfies keyof Hl7Settings;
const ASTM_ROLE = "role" satisfies keyof AstmSettings;

const DEFAULT_ACK_MODE: AckMode = "original";
const DEFAULT_ASTM_ROLE = "computer";

/**
 * Settings the generated types may not have yet (the instrument variants), read and written as
 * plain keys. The engine's defaults: what an absent key means.
 */
const VARIANTS = {
  hl7: [
    ["hl7Version", "version", "2.5.1"],
    ["requireEndCr", "require_end_cr", true],
    ["ackMessageTypes", "ack_message_types", ["ACK"]],
    ["replyType", "reply_type", "ack"],
  ],
  astm: [
    ["maxFrameChars", "max_frame_chars", 240],
    ["enqAttempts", "enq_attempts", 3],
    ["busyRetryMs", "busy_retry_ms", 10_000],
    ["sampleIdPath", "sample_id_path", "O.3"],
  ],
} as const;

type VariantRow = readonly [field: keyof ConnectorSettings, key: string, fallback: unknown];

/** Each variant field of `rows`, from the wire: the value there, else the engine's default. */
function variantsFromWire(rows: readonly VariantRow[], settings: WireSettings | undefined) {
  const out: Record<string, unknown> = {};
  for (const [field, key, fallback] of rows) {
    const value = settings?.[key];
    out[field] = clone(value === undefined || value === null ? fallback : value);
  }
  return out as Partial<ConnectorSettings>;
}

/** Writes the variant fields that differ from `base` (all that differ from the default for a new connector). */
function variantsToWire(
  rows: readonly VariantRow[],
  into: WireSettings,
  ui: ConnectorSettings,
  base: ConnectorSettings | null,
): void {
  for (const [field, key, fallback] of rows) {
    const value = ui[field];
    if (value === undefined) continue;
    const was = base === null ? fallback : (base[field] ?? fallback);
    if (!isEqual(value, was)) into[key] = clone(value);
  }
}

/** The settings the console shows for a connector speaking `protocol`. */
export function settingsFromWire(
  protocol: Protocol,
  settings: WireSettings | undefined,
): ConnectorSettings {
  switch (protocol) {
    case "hl7_mllp":
      return {
        ackMode: (settings?.[ACK_MODE] as AckMode | undefined) ?? DEFAULT_ACK_MODE,
        ...variantsFromWire(VARIANTS.hl7, settings),
      };
    case "astm":
      return {
        astmRole:
          (settings?.[ASTM_ROLE] as ConnectorSettings["astmRole"] | undefined) ?? DEFAULT_ASTM_ROLE,
        ...variantsFromWire(VARIANTS.astm, settings),
      };
    case "au": {
      const au = auFromWire(settings);
      return au === undefined ? {} : { au };
    }
    case "delimited_file":
      return { templateJson: JSON.stringify(settings ?? {}, null, 2) };
    case "raw":
      return {};
  }
}

/** The settings object in `templateJson`; a text that is not one is reported, not sent. */
function templateFrom(
  ui: ConnectorSettings,
  path: string,
  issues: Issues,
): Record<string, unknown> | null {
  const text = ui.templateJson ?? "";
  let message = "";
  let parsed: unknown = null;
  if (text.trim() === "") message = "A column template is required.";
  else {
    try {
      parsed = JSON.parse(text);
    } catch {
      message = "The template is not valid JSON.";
    }
  }
  if (message === "" && !isRecord(parsed)) message = "The template must be a JSON object.";
  if (message !== "" || !isRecord(parsed)) {
    issues.add(`${path}.settings.templateJson`, message);
    return null;
  }
  return parsed;
}

/** Settings for a connector that has none yet: only what differs from the defaults. */
function newSettings(
  protocol: Protocol,
  ui: ConnectorSettings,
  path: string,
  issues: Issues,
): WireSettings {
  const settings: WireSettings = {};
  switch (protocol) {
    case "hl7_mllp":
      if (ui.ackMode !== undefined && ui.ackMode !== DEFAULT_ACK_MODE) {
        settings[ACK_MODE] = ui.ackMode;
      }
      variantsToWire(VARIANTS.hl7, settings, ui, null);
      break;
    case "astm":
      if (ui.astmRole !== undefined && ui.astmRole !== DEFAULT_ASTM_ROLE) {
        settings[ASTM_ROLE] = ui.astmRole;
      }
      variantsToWire(VARIANTS.astm, settings, ui, null);
      break;
    case "au":
      if (ui.au !== undefined) auToWire(settings, ui.au, undefined);
      break;
    case "delimited_file":
      return templateFrom(ui, path, issues) ?? {};
    case "raw":
      break;
  }
  return settings;
}

/**
 * The engine settings for a connector. `raw` is the original settings, only when the
 * connector kept its protocol; `base` is what reading them gave.
 */
export function settingsToWire(
  ui: ConnectorConfig,
  raw: WireSettings | undefined,
  base: ConnectorSettings | null,
  path: string,
  issues: Issues,
): WireSettings {
  if (raw === undefined || base === null)
    return newSettings(ui.protocol, ui.settings, path, issues);
  const next = clone(raw);
  const { settings } = ui;
  switch (ui.protocol) {
    case "hl7_mllp":
      if (settings.ackMode !== undefined && settings.ackMode !== base.ackMode) {
        next[ACK_MODE] = settings.ackMode;
      }
      variantsToWire(VARIANTS.hl7, next, settings, base);
      break;
    case "astm":
      if (settings.astmRole !== undefined && settings.astmRole !== base.astmRole) {
        next[ASTM_ROLE] = settings.astmRole;
      }
      variantsToWire(VARIANTS.astm, next, settings, base);
      break;
    case "au":
      if (settings.au !== undefined) auToWire(next, settings.au, base.au);
      break;
    case "delimited_file":
      if (settings.templateJson !== base.templateJson) {
        return templateFrom(settings, path, issues) ?? next;
      }
      break;
    case "raw":
      break;
  }
  return next;
}
