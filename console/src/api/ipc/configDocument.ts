/**
 * The whole `runtime.json` document, both ways. The console edits `ConfigDocument`; the engine
 * has the document of docs/spec/configuration.md section 4. Writing needs the document the edit
 * started from (`original`, the one read at `ConfigDocument.version`) so that everything the
 * console does not model (`schema_version`, `diagnostics`, `tags`, ...) is kept; see
 * `endpointConfig.ts` and `channelConfig.ts`.
 */

import type { ChannelOutcome, ConfigDiff, Problem } from "@/ipc/generated";
import type {
  AlertThresholds,
  ApplyPlan,
  ApplyResult,
  ConfigDocument,
  ConfigIssue,
  ValidationResult,
} from "../types";
import { channelFromWire, channelToWire } from "./channelConfig";
import { issueList } from "./connectorConfig";
import { endpointFromWire, endpointToWire } from "./endpointConfig";
import { toUiPath } from "./paths";
import { applyPatch, isEqual, type Patch } from "./util";
import { outcomeFromIpc } from "./versions";
import type { WireAlerts, WireConfig } from "./wire";

const DEFAULT_ALERTS: AlertThresholds = {
  endpointDownSeconds: 60,
  queueDepthWarning: 1000,
  bufferPressurePercent: 80,
  diskFreeMinMib: 1024,
};

/** `alerts` keys the console models (checked against the generated thresholds). */
const ALERT_KEYS = {
  endpointDownSeconds: "endpoint_down_seconds",
  queueDepthWarning: "delivery_backlog_count",
  bufferPressurePercent: "buffer_high_percent",
  diskFreeMinMib: "disk_low_megabytes",
} as const satisfies Record<keyof AlertThresholds, keyof WireAlerts>;

function alertsFromWire(raw: WireAlerts | undefined): AlertThresholds {
  const read = (field: keyof AlertThresholds) => raw?.[ALERT_KEYS[field]] ?? DEFAULT_ALERTS[field];
  return {
    endpointDownSeconds: read("endpointDownSeconds"),
    queueDepthWarning: read("queueDepthWarning"),
    bufferPressurePercent: read("bufferPressurePercent"),
    diskFreeMinMib: read("diskFreeMinMib"),
  };
}

const setter =
  (field: keyof AlertThresholds) =>
  (r: WireAlerts, u: AlertThresholds): void => {
    r[ALERT_KEYS[field]] = u[field];
  };

const ALERTS: Patch<AlertThresholds, WireAlerts> = {
  endpointDownSeconds: setter("endpointDownSeconds"),
  queueDepthWarning: setter("queueDepthWarning"),
  bufferPressurePercent: setter("bufferPressurePercent"),
  diskFreeMinMib: setter("diskFreeMinMib"),
};

export function documentFromWire(raw: WireConfig): ConfigDocument {
  return {
    version: raw.config_version ?? 0,
    endpoints: (raw.endpoints ?? []).map(endpointFromWire),
    channels: (raw.channels ?? []).map(channelFromWire),
    alertThresholds: alertsFromWire(raw.alerts),
  };
}

export interface ConvertedDocument {
  document: WireConfig;
  /** Problems the console found itself while converting (at console paths). */
  issues: ConfigIssue[];
}

/** The `alerts` object for `ui`, or undefined when there is none to write. */
function alertsToWire(
  ui: AlertThresholds,
  original: WireConfig | undefined,
): WireAlerts | undefined {
  const alerts: WireAlerts = { ...original?.alerts };
  applyPatch(alerts, ui, original === undefined ? null : alertsFromWire(original.alerts), ALERTS);
  if (original === undefined) {
    for (const [field, key] of Object.entries(ALERT_KEYS)) {
      if (alerts[key] === DEFAULT_ALERTS[field as keyof AlertThresholds]) delete alerts[key];
    }
  }
  if (original?.alerts !== undefined && isEqual(alerts, original.alerts)) return original.alerts;
  return Object.keys(alerts).length === 0 ? undefined : alerts;
}

/** The engine document for `ui`. `original` is the document `ui` was edited from, if known. */
export function documentToWire(ui: ConfigDocument, original?: WireConfig): ConvertedDocument {
  const issues = issueList();
  const endpoints = new Map(original?.endpoints?.map((e) => [e.endpoint_id, e]));
  const channels = new Map(original?.channels?.map((c) => [c.channel_id, c]));
  const document: WireConfig = { ...original };
  document.schema_version ??= 1;
  if (original?.config_version !== undefined || ui.version !== 0) {
    document.config_version = ui.version;
  }
  if (ui.endpoints.length > 0 || original?.endpoints !== undefined) {
    document.endpoints = ui.endpoints.map((endpoint) =>
      endpointToWire(endpoint, endpoints.get(endpoint.id)),
    );
  }
  if (ui.channels.length > 0 || original?.channels !== undefined) {
    document.channels = ui.channels.map((channel, i) =>
      channelToWire(channel, channels.get(channel.id), `channels[${i}]`, issues),
    );
  }
  const alerts = alertsToWire(ui.alertThresholds, original);
  if (alerts === undefined) delete document.alerts;
  else document.alerts = alerts;
  return { document, issues: issues.list };
}

/** Problems the engine reported, at console paths. `document` is what the engine checked. */
function issuesFromWire(issues: readonly Problem[], document?: WireConfig): ConfigIssue[] {
  return issues.map(({ severity, code, path, message }) => ({
    severity,
    path: toUiPath(path, document),
    message,
    code,
  }));
}

/** The engine's plan, in the console's shape. */
function planFromWire(diff: ConfigDiff): ApplyPlan {
  return {
    channels: diff.channels.map(({ channel_id, label, change }) => ({
      id: channel_id,
      label,
      change,
    })),
    endpoints: diff.endpoints.map(({ endpoint_id, label, change }) => ({
      id: endpoint_id,
      label,
      change,
    })),
    alertsChanged: diff.alerts_changed,
  };
}

/**
 * A validation outcome: the engine's problems after the console's own, and the plan the
 * engine gives (`diff`, empty when the document has errors).
 */
export function validationResult(
  local: ConfigIssue[],
  reported: readonly Problem[],
  document: WireConfig | undefined,
  diff: ConfigDiff,
): ValidationResult {
  const issues = [...local, ...issuesFromWire(reported, document)];
  const ok = !issues.some((issue) => issue.severity === "error");
  return { ok, issues, plan: planFromWire(diff) };
}

/**
 * What an apply did, from the engine's per-channel outcomes. `restartedChannelIds` are the
 * channels that were restarted (`restarted`; a channel that was merely `started`, `stopped`,
 * `updated` in place or `removed` is not a restart), `unchangedChannelCount` the `unchanged`
 * ones, and `failures` the `failed` ones with the engine's reason.
 */
export function applyResult(version: number, outcomes: readonly ChannelOutcome[]): ApplyResult {
  const result: ApplyResult = {
    version,
    outcomes: outcomes.map(outcomeFromIpc),
    restartedChannelIds: [],
    unchangedChannelCount: 0,
    failures: [],
  };
  for (const outcome of outcomes) {
    switch (outcome.outcome) {
      case "restarted":
        result.restartedChannelIds.push(outcome.channel_id);
        break;
      case "unchanged":
        result.unchangedChannelCount += 1;
        break;
      case "failed":
        result.failures.push({ channelId: outcome.channel_id, reason: outcome.reason });
        break;
      case "updated":
      case "started":
      case "stopped":
      case "removed":
        break;
      default: {
        // A new outcome of a regenerated type must be placed above.
        const unhandled: never = outcome;
        void unhandled;
      }
    }
  }
  return result;
}
