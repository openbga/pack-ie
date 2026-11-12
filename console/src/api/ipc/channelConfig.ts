/**
 * Channel documents (docs/spec/configuration.md section 6): the engine's channel to the
 * console's, and back. Same rules as `endpointConfig.ts`: read with the specification's
 * defaults filled in, write by overwriting only what the console changed (so `tags`, `delivery`
 * policy, unmodeled retry and protocol settings survive), new items sparse. Filters, mappings
 * and protocol settings are in `filters.ts`, `mappings.ts` and `connectorConfig.ts`.
 */

import { clone } from "@/lib/clone";
import type {
  ChannelConfig,
  DestinationConfig,
  MessageChannelConfig,
  RetentionPolicy,
  RetryPolicy,
  SourceConfig,
} from "../types";
import { bridgeFromWire, bridgeToWire } from "./bridgeConfig";
import {
  DEFAULT_ATTEMPTS,
  DEFAULT_INITIAL_MS,
  DEFAULT_MAX_MS,
  DEFAULT_RETENTION_DAYS,
} from "./channelDefaults";
import { type Issues, settingsFromWire, settingsToWire } from "./connectorConfig";
import { filtersFromWire, filtersToWire, sameFilters } from "./filters";
import { type MappingParts, mappingFromWire, mappingToWire, sameMapping } from "./mappings";
import { isEqual } from "./util";
import type {
  WireBridge,
  WireChannel,
  WireDelivery,
  WireDeliveryOverrides,
  WireDestination,
  WireMessage,
  WireSettings,
  WireSource,
} from "./wire";

const SOURCE_ID = "source";
const NO_MAPPING: MappingParts = {
  mappings: [],
  codeTables: {},
  requireSingleGroup: false,
  require: [],
};

// ── Destinations ───────────────────────────────────────────────────────────────────────

/** The delivery policy a destination without overrides gets: the channel's, else the defaults. */
function deliveryDefaults(channel: WireDelivery | undefined): RetryPolicy {
  return {
    initialMs: channel?.retry?.initial_delay_ms ?? DEFAULT_INITIAL_MS,
    maxMs: channel?.retry?.max_delay_ms ?? DEFAULT_MAX_MS,
    maxAttempts: channel?.max_attempts ?? DEFAULT_ATTEMPTS,
  };
}

function retryOf(
  delivery: WireDeliveryOverrides | undefined,
  channel: WireDelivery | undefined,
): RetryPolicy {
  const fallback = deliveryDefaults(channel);
  return {
    initialMs: delivery?.retry?.initial_delay_ms ?? fallback.initialMs,
    maxMs: delivery?.retry?.max_delay_ms ?? fallback.maxMs,
    maxAttempts: delivery?.max_attempts ?? fallback.maxAttempts,
  };
}

function destinationFromWire(
  raw: WireDestination,
  channelDelivery: WireDelivery | undefined,
): DestinationConfig {
  const shown = mappingFromWire(raw.mapping);
  const mapping = shown ?? NO_MAPPING;
  return {
    id: raw.destination_id,
    endpointId: raw.endpoint,
    protocol: raw.protocol,
    settings: settingsFromWire(raw.protocol, raw.settings),
    label: raw.label,
    enabled: raw.enabled ?? true,
    filters: filtersFromWire(raw.filters),
    mappings: mapping.mappings,
    codeTables: mapping.codeTables,
    ...(shown === null ? { mappingHidden: true as const } : {}),
    ...(mapping.requireSingleGroup ? { requireSingleGroup: true as const } : {}),
    ...(mapping.require.length > 0 ? { require: mapping.require } : {}),
    retry: retryOf(raw.delivery, channelDelivery),
  };
}

/** Sets `settings`, or removes it when there are none. */
function assignSettings(target: { settings?: WireSettings }, settings: WireSettings): void {
  if (Object.keys(settings).length === 0) delete target.settings;
  else target.settings = settings;
}

/** The fields a source and a destination share: endpoint, protocol, protocol settings. */
function patchConnector(
  next: { endpoint: string; protocol: WireSource["protocol"]; settings?: WireSettings },
  ui: SourceConfig | DestinationConfig,
  raw: { settings?: WireSettings } | undefined,
  base: SourceConfig | DestinationConfig | null,
  path: string,
  issues: Issues,
): void {
  if (raw === undefined || base === null) {
    next.endpoint = ui.endpointId;
    next.protocol = ui.protocol;
    assignSettings(next, settingsToWire(ui, undefined, null, path, issues));
    return;
  }
  if (ui.endpointId !== base.endpointId) next.endpoint = ui.endpointId;
  const kept = ui.protocol === base.protocol;
  if (!kept) next.protocol = ui.protocol;
  const settings = settingsToWire(
    ui,
    kept ? (raw.settings ?? {}) : undefined,
    kept ? base.settings : null,
    path,
    issues,
  );
  if (!kept || !isEqual(settings, raw.settings ?? {})) assignSettings(next, settings);
}

const MAPPING_HIDDEN =
  "This mapping uses features the console cannot show; change it in runtime.json.";

function patchMapping(
  next: WireDestination,
  ui: DestinationConfig,
  base: DestinationConfig | null,
  path: string,
  issues: Issues,
): void {
  const parts = {
    mappings: ui.mappings,
    codeTables: ui.codeTables,
    requireSingleGroup: ui.requireSingleGroup === true,
    require: ui.require ?? [],
  };
  if (
    base !== null &&
    sameMapping(parts, {
      mappings: base.mappings,
      codeTables: base.codeTables,
      requireSingleGroup: base.requireSingleGroup === true,
      require: base.require ?? [],
    })
  ) {
    return;
  }
  if (
    next.mapping !== undefined &&
    next.mapping !== null &&
    mappingFromWire(next.mapping) === null
  ) {
    issues.add(`${path}.mappings`, MAPPING_HIDDEN);
    return;
  }
  const mapping = mappingToWire(parts, next.mapping);
  if (mapping !== null) next.mapping = mapping;
  else if (base === null) delete next.mapping;
  else next.mapping = null;
}

function patchRetry(
  next: WireDestination,
  ui: DestinationConfig,
  fallback: RetryPolicy,
  base: DestinationConfig | null,
  path: string,
  issues: Issues,
): void {
  const was = base?.retry ?? fallback;
  if (ui.retry.maxAttempts === null) {
    if (was.maxAttempts !== null) {
      issues.add(`${path}.retry.maxAttempts`, "Maximum attempts is required.");
    }
  } else if (ui.retry.maxAttempts !== was.maxAttempts) {
    next.delivery = { ...next.delivery, max_attempts: ui.retry.maxAttempts };
  }
  if (ui.retry.initialMs !== was.initialMs || ui.retry.maxMs !== was.maxMs) {
    next.delivery = {
      ...next.delivery,
      retry: {
        ...next.delivery?.retry,
        initial_delay_ms: ui.retry.initialMs,
        max_delay_ms: ui.retry.maxMs,
      },
    };
  }
}

function destinationToWire(
  ui: DestinationConfig,
  raw: WireDestination | undefined,
  channelDelivery: WireDelivery | undefined,
  path: string,
  issues: Issues,
): WireDestination {
  const base = raw === undefined ? null : destinationFromWire(raw, channelDelivery);
  const next: WireDestination =
    raw === undefined
      ? {
          destination_id: ui.id,
          label: ui.label,
          ...(ui.enabled ? {} : { enabled: false }),
          endpoint: ui.endpointId,
          protocol: ui.protocol,
        }
      : clone(raw);
  if (base !== null && ui.label !== base.label) next.label = ui.label;
  if (base !== null && ui.enabled !== base.enabled) next.enabled = ui.enabled;
  patchConnector(next, ui, raw, base, path, issues);
  if (base === null ? ui.filters.length > 0 : !sameFilters(ui.filters, base.filters)) {
    const filters = filtersToWire(ui.filters, next.filters);
    if (filters.length === 0) delete next.filters;
    else next.filters = filters;
  }
  patchMapping(next, ui, base, path, issues);
  patchRetry(next, ui, deliveryDefaults(channelDelivery), base, path, issues);
  return next;
}

// ── Message channels ───────────────────────────────────────────────────────────────────

function sourceFromWire(raw: WireSource): SourceConfig {
  return {
    id: SOURCE_ID,
    endpointId: raw.endpoint,
    protocol: raw.protocol,
    settings: settingsFromWire(raw.protocol, raw.settings),
    duplicateDetection: raw.duplicate_detection ?? true,
  };
}

function sourceToWire(
  ui: SourceConfig,
  raw: WireSource | undefined,
  path: string,
  issues: Issues,
): WireSource {
  const base = raw === undefined ? null : sourceFromWire(raw);
  const next: WireSource =
    raw === undefined ? { endpoint: ui.endpointId, protocol: ui.protocol } : clone(raw);
  patchConnector(next, ui, raw, base, `${path}.source`, issues);
  if (base === null ? !ui.duplicateDetection : ui.duplicateDetection !== base.duplicateDetection) {
    next.duplicate_detection = ui.duplicateDetection;
  }
  return next;
}

function retentionFromWire(raw: WireMessage["retention"]): RetentionPolicy {
  return {
    days: raw?.max_age_days === undefined ? DEFAULT_RETENTION_DAYS : raw.max_age_days,
    maxMessages: raw?.max_messages ?? null,
  };
}

function retentionToWire(
  raw: WireMessage["retention"],
  ui: RetentionPolicy,
  base: RetentionPolicy | null,
): WireMessage["retention"] {
  const next = { ...raw };
  if (ui.days !== (base?.days ?? DEFAULT_RETENTION_DAYS)) next.max_age_days = ui.days;
  if (ui.maxMessages !== (base?.maxMessages ?? null)) next.max_messages = ui.maxMessages;
  return next;
}

function messageFromWire(raw: WireMessage): MessageChannelConfig {
  return {
    kind: "message",
    id: raw.channel_id,
    label: raw.label,
    enabled: raw.enabled ?? true,
    source: sourceFromWire(raw.source),
    filters: filtersFromWire(raw.filters),
    destinations: raw.destinations.map((d) => destinationFromWire(d, raw.delivery)),
    retention: retentionFromWire(raw.retention),
  };
}

export function channelFromWire(raw: WireChannel): ChannelConfig {
  return raw.kind === "bridge" ? bridgeFromWire(raw) : messageFromWire(raw);
}

/** Writes the message-channel fields that are plain values of the channel itself. */
function patchMessageFields(
  next: WireMessage,
  ui: MessageChannelConfig,
  base: MessageChannelConfig | null,
): void {
  if (base === null ? !ui.enabled : ui.enabled !== base.enabled) next.enabled = ui.enabled;
  if (base !== null && ui.label !== base.label) next.label = ui.label;
  const retention = base?.retention ?? retentionFromWire(undefined);
  if (!isEqual(ui.retention, retention)) {
    next.retention = retentionToWire(next.retention, ui.retention, base?.retention ?? null);
  }
  if (base === null ? ui.filters.length > 0 : !sameFilters(ui.filters, base.filters)) {
    const filters = filtersToWire(ui.filters, next.filters);
    if (filters.length === 0) delete next.filters;
    else next.filters = filters;
  }
}

function destinationsToWire(
  ui: MessageChannelConfig,
  raw: WireMessage | undefined,
  path: string,
  issues: Issues,
): WireDestination[] {
  const originals = new Map(raw?.destinations.map((d) => [d.destination_id, d]));
  return ui.destinations.map((destination, j) =>
    destinationToWire(
      destination,
      originals.get(destination.id),
      raw?.delivery,
      `${path}.destinations[${j}]`,
      issues,
    ),
  );
}

function messageToWire(
  ui: MessageChannelConfig,
  raw: WireMessage | undefined,
  path: string,
  issues: Issues,
): WireMessage {
  const base = raw === undefined ? null : messageFromWire(raw);
  const next: WireMessage =
    raw === undefined
      ? {
          channel_id: ui.id,
          label: ui.label,
          kind: "message",
          source: { endpoint: ui.source.endpointId, protocol: ui.source.protocol },
          destinations: [],
        }
      : clone(raw);
  patchMessageFields(next, ui, base);
  next.source = sourceToWire(ui.source, raw?.source, path, issues);
  next.destinations = destinationsToWire(ui, raw, path, issues);
  return next;
}

/**
 * The engine channel for `ui`; `raw` is the original with the same id and kind, when there is
 * one. `path` is the channel's console path, for issues.
 */
export function channelToWire(
  ui: ChannelConfig,
  raw: WireChannel | undefined,
  path: string,
  issues: Issues,
): WireChannel {
  const original = raw?.kind === ui.kind ? raw : undefined;
  if (ui.kind === "message") {
    return messageToWire(ui, original as WireMessage | undefined, path, issues);
  }
  return bridgeToWire(ui, original as WireBridge | undefined, path, issues);
}
