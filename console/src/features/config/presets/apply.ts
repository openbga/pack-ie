// What applying a preset does, worked out before anything changes so the operator sees it first
// (docs: nothing is applied by name; a preset only fills in values the operator can edit).
import { mappingFromWire } from "@/api/ipc/mappings";
import type { WireMapping } from "@/api/ipc/wire";
import { nextId } from "../configDocument";
import { type RequireFormValues, requireToValues } from "../requireForm";
import {
  branches,
  type CodeTableFormValues,
  type RuleFormValues,
  ruleIds,
  ruleToValues,
  tablesToValues,
} from "../ruleForm";
import type { SettingsFormValues } from "../settingsForm";
import type { ConnectorProfile, MappingPreset } from "./catalog";

/** How a settings key of the catalog (the engine's name) shows in the form. */
const SETTINGS: Record<
  string,
  {
    field: keyof SettingsFormValues;
    label: string;
    value: (wire: unknown) => string | boolean | null;
  }
> = {
  ack_mode: { field: "ackMode", label: "Acknowledgement mode", value: text },
  version: { field: "hl7Version", label: "HL7 version", value: text },
  require_end_cr: {
    field: "requireEndCr",
    label: "Carriage return after the end block",
    value: flag,
  },
  ack_message_types: { field: "ackMessageTypes", label: "Accepted answer codes", value: list },
  reply_type: { field: "replyType", label: "Reply with", value: text },
  max_frame_chars: { field: "maxFrameChars", label: "Characters per frame", value: text },
  enq_attempts: { field: "enqAttempts", label: "ENQ attempts", value: text },
  sample_id_path: { field: "sampleIdPath", label: "Sample id path", value: text },
  busy_retry_ms: { field: "busyRetryMs", label: "Wait after a busy answer (ms)", value: text },
};

function text(wire: unknown): string | null {
  return typeof wire === "string" || typeof wire === "number" ? String(wire) : null;
}

function flag(wire: unknown): boolean | null {
  return typeof wire === "boolean" ? wire : null;
}

function list(wire: unknown): string | null {
  return Array.isArray(wire) && wire.every((each) => typeof each === "string")
    ? wire.join(", ")
    : null;
}

/** One setting a preset would write: what it is now and what it becomes. */
export interface SettingChange {
  field: keyof SettingsFormValues;
  label: string;
  now: string | boolean;
  becomes: string | boolean;
  /** False when the connector already has this value. */
  changes: boolean;
}

export interface SettingsPlan {
  changes: SettingChange[];
  /** Keys the form does not know; they are not applied and are named to the operator. */
  unknown: string[];
}

const same = (a: string | boolean, b: string | boolean): boolean => {
  if (typeof a === "string" && typeof b === "string") {
    return (
      a
        .split(",")
        .map((part) => part.trim())
        .join(",") ===
      b
        .split(",")
        .map((part) => part.trim())
        .join(",")
    );
  }
  return a === b;
};

/** The settings `patch` would write over `current`, key by key. */
export function planSettings(
  patch: Record<string, unknown>,
  current: SettingsFormValues,
): SettingsPlan {
  const plan: SettingsPlan = { changes: [], unknown: [] };
  for (const [key, wire] of Object.entries(patch)) {
    const known = SETTINGS[key];
    const becomes = known?.value(wire) ?? null;
    if (known === undefined || becomes === null) {
      plan.unknown.push(key);
      continue;
    }
    const now = current[known.field] as string | boolean;
    plan.changes.push({
      field: known.field,
      label: known.label,
      now,
      becomes,
      changes: !same(now, becomes),
    });
  }
  return plan;
}

/** A setting the example of a mapping preset was read with, that the source does not have yet. */
export interface SourceHint {
  change: SettingChange;
  /** The label of the source preset that sets exactly this, when the catalog has one. */
  profile: string | null;
}

/**
 * The source settings behind a mapping preset's example that differ from the source's own, with
 * the catalog's source preset that sets them. Only settings the form shows are considered; the
 * rest of the example's source settings (an AU parameter sheet) are the operator's own.
 */
export function sourceHints(
  preset: MappingPreset,
  source: SettingsFormValues,
  profiles: readonly ConnectorProfile[],
): SourceHint[] {
  const shown = Object.fromEntries(
    Object.entries(preset.sample.source_settings ?? {}).filter(([key]) => key in SETTINGS),
  );
  return planSettings(shown, source)
    .changes.filter((change) => change.changes)
    .map((change) => {
      const key = Object.keys(SETTINGS).find((each) => SETTINGS[each]?.field === change.field);
      const match = profiles.find(
        (profile) =>
          profile.roles.includes("source") &&
          key !== undefined &&
          JSON.stringify(profile.settings[key]) === JSON.stringify(shown[key]),
      );
      return { change, profile: match?.label ?? null };
    });
}

export function planProfile(profile: ConnectorProfile, current: SettingsFormValues): SettingsPlan {
  return planSettings(profile.settings, current);
}

export type MappingMode = "add" | "replace";

export interface MappingPlan {
  /** The rule list the destination ends up with. */
  mappings: RuleFormValues[];
  codeTables: CodeTableFormValues[];
  rulesAdded: number;
  rulesRemoved: number;
  tablesAdded: string[];
  /** Tables the destination already had under a name the preset uses; they are kept as they are. */
  tablesKept: string[];
  tablesRemoved: number;
  /** The mapping refuses messages with several patients or orders once the preset is applied. */
  requireSingleGroup: boolean;
  /** The source requirements the destination ends up with, and how many the preset adds. */
  require: RequireFormValues[];
  requirementsAdded: number;
  settings: SettingsPlan;
}

/** A rule tree with ids nothing else uses, so that it can join a list that has its own. */
function withFreshIds(rule: RuleFormValues, taken: Set<string>): RuleFormValues {
  const ruleId = nextId("m", taken);
  taken.add(ruleId);
  return {
    ...rule,
    ruleId,
    ...branches(
      rule.then.map((each) => withFreshIds(each, taken)),
      rule.else.map((each) => withFreshIds(each, taken)),
    ),
    rules: rule.rules.map((each) => withFreshIds(each, taken)),
  };
}

/**
 * The destination's mapping after applying `preset`: its rules after the operator's (`add`) or
 * instead of them (`replace`). A table the operator already has by the same name is never
 * overwritten in `add`; `replace` is the only way anything of theirs goes, and it says so.
 */
export function planMapping(
  preset: MappingPreset,
  current: {
    mappings: RuleFormValues[];
    codeTables: CodeTableFormValues[];
    requireSingleGroup: boolean;
    require: RequireFormValues[];
    settings: SettingsFormValues;
  },
  mode: MappingMode,
): MappingPlan | null {
  const parts = mappingFromWire(preset.mapping as WireMapping);
  if (parts === null) return null;
  const keep = mode === "add" ? current.mappings : [];
  const taken = new Set(ruleIds(keep));
  const added = parts.mappings.map((rule) => withFreshIds(ruleToValues(rule), taken));
  const presetTables = tablesToValues(parts.codeTables);
  const have = new Set(current.codeTables.map((table) => table.name.trim()));
  const tablesKept: string[] = [];
  const tablesAdded: string[] = [];
  const tables = mode === "add" ? [...current.codeTables] : [];
  for (const table of presetTables) {
    if (mode === "add" && have.has(table.name)) tablesKept.push(table.name);
    else {
      tablesAdded.push(table.name);
      tables.push(table);
    }
  }
  const kept = mode === "add" ? current.require : [];
  const known = new Set(kept.map((each) => JSON.stringify(each)));
  const fresh = parts.require
    .map(requireToValues)
    .filter((each) => !known.has(JSON.stringify(each)));
  return {
    require: [...kept, ...fresh],
    requirementsAdded: fresh.length,
    mappings: [...keep, ...added],
    codeTables: tables,
    rulesAdded: added.length,
    rulesRemoved: mode === "replace" ? current.mappings.length : 0,
    tablesAdded,
    tablesKept,
    tablesRemoved: mode === "replace" ? current.codeTables.length : 0,
    // A preset that assumes one sample asks for it; it never clears the operator's own choice.
    requireSingleGroup: parts.requireSingleGroup || (mode === "add" && current.requireSingleGroup),
    settings: planSettings(preset.destinationSettings, current.settings),
  };
}
