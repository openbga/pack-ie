// Bridge channel documents: the engine's to the console's and back (see `channelConfig.ts`).
import type { TapRetention } from "@/ipc/generated";
import { clone } from "@/lib/clone";
import type { BridgeChannelConfig } from "../types";
import {
  DEFAULT_BUFFER_BYTES,
  DEFAULT_INITIAL_MS,
  DEFAULT_MAX_MS,
  DEFAULT_TAP_RETENTION_DAYS,
} from "./channelDefaults";
import type { Issues } from "./connectorConfig";
import { applyPatch, isEqual, type Patch } from "./util";
import type { WireBridge } from "./wire";

export function bridgeFromWire(raw: WireBridge): BridgeChannelConfig {
  return {
    kind: "bridge",
    id: raw.channel_id,
    label: raw.label,
    enabled: raw.enabled ?? true,
    endpointAId: raw.endpoint_a,
    endpointBId: raw.endpoint_b,
    pairing: raw.connection?.connect_strategy ?? "parallel",
    pairingReceiver: raw.connection?.receiver ?? null,
    bufferBytes: raw.buffer?.max_bytes ?? DEFAULT_BUFFER_BYTES,
    retry: {
      initialMs: raw.retry?.initial_delay_ms ?? DEFAULT_INITIAL_MS,
      maxMs: raw.retry?.max_delay_ms ?? DEFAULT_MAX_MS,
      // A bridge retries until it connects; the specification has no attempt limit for it.
      maxAttempts: null,
    },
    tapFramer: raw.tap?.framer ?? "none",
    tapRetention: {
      days:
        raw.tap?.retention?.max_age_days === undefined
          ? DEFAULT_TAP_RETENTION_DAYS
          : raw.tap.retention.max_age_days,
      maxMessages: raw.tap?.retention?.max_messages ?? null,
    },
  };
}

const BRIDGE: Patch<BridgeChannelConfig, WireBridge> = {
  label: (r, u) => {
    r.label = u.label;
  },
  enabled: (r, u) => {
    r.enabled = u.enabled;
  },
  endpointAId: (r, u) => {
    r.endpoint_a = u.endpointAId;
  },
  endpointBId: (r, u) => {
    r.endpoint_b = u.endpointBId;
  },
  pairing: (r, u) => {
    r.connection = { ...r.connection, connect_strategy: u.pairing };
  },
  pairingReceiver: (r, u) => {
    const { receiver: _, ...rest } = r.connection ?? {};
    r.connection = u.pairingReceiver === null ? rest : { ...rest, receiver: u.pairingReceiver };
  },
  bufferBytes: (r, u) => {
    r.buffer = { ...r.buffer, max_bytes: u.bufferBytes };
  },
  retry: (r, u, base) => {
    const retry = { ...r.retry };
    if (base === null || u.retry.initialMs !== base.retry.initialMs) {
      retry.initial_delay_ms = u.retry.initialMs;
    }
    if (base === null || u.retry.maxMs !== base.retry.maxMs) retry.max_delay_ms = u.retry.maxMs;
    r.retry = retry;
  },
  tapFramer: (r, u) => {
    r.tap = { ...r.tap, framer: u.tapFramer };
  },
  tapRetention: (r, u, base) => {
    // Only a limit that changed is written; the tap's other keys stay as they were.
    const retention: Partial<TapRetention> = { ...r.tap?.retention };
    if (base === null || u.tapRetention.days !== base.tapRetention.days) {
      retention.max_age_days = u.tapRetention.days;
    }
    if (base === null || u.tapRetention.maxMessages !== base.tapRetention.maxMessages) {
      retention.max_messages = u.tapRetention.maxMessages;
    }
    r.tap = { ...r.tap, retention: retention as TapRetention };
  },
};

/** What a new bridge leaves out because the engine assumes it. */
const BRIDGE_DEFAULTS: Record<string, Record<string, unknown>> = {
  connection: { connect_strategy: "parallel" },
  buffer: { max_bytes: DEFAULT_BUFFER_BYTES },
  retry: { initial_delay_ms: DEFAULT_INITIAL_MS, max_delay_ms: DEFAULT_MAX_MS },
  tap: {
    framer: "none",
    retention: { max_age_days: DEFAULT_TAP_RETENTION_DAYS, max_messages: null },
  },
};

function pruneDefaults(next: WireBridge): void {
  const sections = next as unknown as Record<string, Record<string, unknown> | undefined>;
  for (const [section, defaults] of Object.entries(BRIDGE_DEFAULTS)) {
    const values = sections[section];
    if (values === undefined) continue;
    for (const [key, value] of Object.entries(defaults)) {
      if (isEqual(values[key], value)) delete values[key];
    }
    if (Object.keys(values).length === 0) delete sections[section];
  }
}

export function bridgeToWire(
  ui: BridgeChannelConfig,
  raw: WireBridge | undefined,
  path: string,
  issues: Issues,
): WireBridge {
  const next: WireBridge =
    raw === undefined
      ? {
          channel_id: ui.id,
          label: ui.label,
          kind: "bridge",
          ...(ui.enabled ? {} : { enabled: false }),
          endpoint_a: ui.endpointAId,
          endpoint_b: ui.endpointBId,
        }
      : clone(raw);
  applyPatch(next, ui, raw === undefined ? null : bridgeFromWire(raw), BRIDGE);
  if (raw === undefined) {
    pruneDefaults(next);
    if (next.enabled === true) delete next.enabled;
  }
  if (ui.retry.maxAttempts !== null) {
    issues.add(
      `${path}.retry.maxAttempts`,
      "A bridge retries until it connects; leave the attempt limit empty.",
    );
  }
  return next;
}
