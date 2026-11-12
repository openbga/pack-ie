// The channel form's values and how they become a `ChannelConfig`. Numbers are held as text
// while typed. The form holds both kinds' fields; `kind` decides which are used. Field names
// follow the engine's paths (`source.endpointId`, `destinations.0.retry.maxAttempts`) so engine
// errors land on them; ids of rules and connectors are kept under other names, since the form
// library keys its own list rows by `id`.
import type {
  ChannelConfig,
  ChannelKind,
  ConnectorConfig,
  DestinationConfig,
  PairingStrategy,
  Protocol,
  RetentionPolicy,
  RetryPolicy,
  SourceConfig,
  TapFramer,
} from "@/api/types";
import { protocolLabel } from "@/lib/status";
import { type RequireFormValues, requireToValues, valuesToRequire } from "./requireForm";
import {
  type CodeTableFormValues,
  type FilterFormValues,
  filterToValues,
  type RuleFormValues,
  ruleToValues,
  tablesToValues,
  valuesToFilter,
  valuesToRule,
  valuesToTables,
} from "./ruleForm";
import {
  emptySettings,
  type SettingsFormValues,
  settingsToValues,
  valuesToSettings,
} from "./settingsForm";

interface ConnectorFormValues {
  connectorId: string;
  endpointId: string | null;
  protocol: Protocol | null;
  settings: SettingsFormValues;
}

interface SourceFormValues extends ConnectorFormValues {
  duplicateDetection: boolean;
}

interface RetryFormValues {
  initialMs: string;
  maxMs: string;
  /** Empty retries for ever; only bridges may leave it empty. */
  maxAttempts: string;
}

export interface DestinationFormValues extends ConnectorFormValues {
  label: string;
  enabled: boolean;
  filters: FilterFormValues[];
  mappings: RuleFormValues[];
  codeTables: CodeTableFormValues[];
  /** The mapping is one this console cannot show; it is kept as it is. */
  mappingHidden: boolean;
  /** The mapping refuses a message with more than one patient, order or specimen. */
  requireSingleGroup: boolean;
  require: RequireFormValues[];
  retry: RetryFormValues;
}

export interface ChannelFormValues {
  kind: ChannelKind | null;
  id: string;
  label: string;
  enabled: boolean;
  endpointAId: string | null;
  endpointBId: string | null;
  pairing: PairingStrategy;
  pairingReceiver: "a" | "b" | null;
  bufferBytes: string;
  tapFramer: TapFramer;
  source: SourceFormValues;
  /** The source filters. */
  filters: FilterFormValues[];
  destinations: DestinationFormValues[];
  /** Bridges: the reconnect policy. */
  retry: RetryFormValues;
  retention: { days: string; maxMessages: string };
  /** Bridges: how long the tap keeps the messages it records. */
  tapRetention: { days: string; maxMessages: string };
}

function emptyConnector(connectorId: string): ConnectorFormValues {
  return {
    connectorId,
    endpointId: null,
    protocol: null,
    settings: emptySettings(),
  };
}

function emptySource(): SourceFormValues {
  return { ...emptyConnector("source"), duplicateDetection: true };
}

export function emptyDestination(connectorId: string): DestinationFormValues {
  return {
    ...emptyConnector(connectorId),
    label: "",
    enabled: true,
    filters: [],
    mappings: [],
    codeTables: [],
    mappingHidden: false,
    requireSingleGroup: false,
    require: [],
    retry: { initialMs: "2000", maxMs: "300000", maxAttempts: "5" },
  };
}

export const EMPTY_CHANNEL_VALUES: ChannelFormValues = {
  kind: null,
  id: "",
  label: "",
  enabled: true,
  endpointAId: null,
  endpointBId: null,
  pairing: "parallel",
  pairingReceiver: null,
  bufferBytes: "65536",
  tapFramer: "none",
  source: emptySource(),
  filters: [],
  destinations: [emptyDestination("d1")],
  retry: { initialMs: "1000", maxMs: "60000", maxAttempts: "" },
  retention: { days: "", maxMessages: "" },
  tapRetention: { days: "30", maxMessages: "" },
};

/** What a delimited connector starts with when the protocol is chosen. */
export const STARTER_TEMPLATE = JSON.stringify(
  {
    delimiter: ",",
    header_row: true,
    columns: [{ name: "sample_id", type: "string" }],
  },
  null,
  2,
);

/**
 * docs/spec/configuration.md section 9, `passthrough_protocol`: a destination with no mapping
 * rules and no code tables passes the original message through unchanged, so it must speak the
 * source's protocol. The message, or null when the rule holds or does not apply yet.
 */
export function passthroughProblem(
  sourceProtocol: Protocol | null,
  destination: Pick<DestinationFormValues, "protocol" | "mappings" | "codeTables"> & {
    mappingHidden?: boolean;
  },
): string | null {
  if (sourceProtocol === null || destination.protocol === null) return null;
  if (destination.mappingHidden) return null;
  if (destination.mappings.length > 0 || destination.codeTables.length > 0) return null;
  if (destination.protocol === sourceProtocol) return null;
  return `Without a mapping, this destination must use the source's protocol (${protocolLabel(sourceProtocol)}). Choose that protocol or add a mapping.`;
}

function connectorToValues(connector: ConnectorConfig): ConnectorFormValues {
  return {
    connectorId: connector.id,
    endpointId: connector.endpointId,
    protocol: connector.protocol,
    settings: settingsToValues(connector.settings),
  };
}

function valuesToConnector(values: ConnectorFormValues): ConnectorConfig {
  return {
    id: values.connectorId,
    endpointId: values.endpointId ?? "",
    protocol: values.protocol ?? "raw",
    settings: valuesToSettings(values.settings, values.protocol),
  };
}

const num = (value: string) => Number(value.trim());
const optionalNum = (value: string) => (value.trim() === "" ? null : num(value));

function retryToValues(retry: RetryPolicy): RetryFormValues {
  return {
    initialMs: String(retry.initialMs),
    maxMs: String(retry.maxMs),
    maxAttempts: retry.maxAttempts === null ? "" : String(retry.maxAttempts),
  };
}

function valuesToRetry(values: RetryFormValues): RetryPolicy {
  return {
    initialMs: num(values.initialMs),
    maxMs: num(values.maxMs),
    maxAttempts: optionalNum(values.maxAttempts),
  };
}

function sourceToValues(source: SourceConfig): SourceFormValues {
  return { ...connectorToValues(source), duplicateDetection: source.duplicateDetection };
}

function destinationToValues(destination: DestinationConfig): DestinationFormValues {
  return {
    ...connectorToValues(destination),
    label: destination.label,
    enabled: destination.enabled,
    filters: destination.filters.map(filterToValues),
    mappings: destination.mappings.map(ruleToValues),
    codeTables: tablesToValues(destination.codeTables),
    mappingHidden: destination.mappingHidden === true,
    requireSingleGroup: destination.requireSingleGroup === true,
    require: (destination.require ?? []).map(requireToValues),
    retry: retryToValues(destination.retry),
  };
}

function valuesToDestination(values: DestinationFormValues): DestinationConfig {
  return {
    ...valuesToConnector(values),
    label: values.label.trim(),
    enabled: values.enabled,
    filters: values.filters.map(valuesToFilter),
    mappings: values.mappings.map(valuesToRule),
    codeTables: valuesToTables(values.codeTables),
    ...(values.mappingHidden ? { mappingHidden: true as const } : {}),
    ...(values.requireSingleGroup ? { requireSingleGroup: true as const } : {}),
    ...(values.require.length > 0 ? { require: values.require.map(valuesToRequire) } : {}),
    retry: valuesToRetry(values.retry),
  };
}

const retentionToValues = (retention: RetentionPolicy): ChannelFormValues["retention"] => ({
  days: retention.days === null ? "" : String(retention.days),
  maxMessages: retention.maxMessages === null ? "" : String(retention.maxMessages),
});

const valuesToRetention = (values: ChannelFormValues["retention"]): RetentionPolicy => ({
  days: optionalNum(values.days),
  maxMessages: optionalNum(values.maxMessages),
});

export function channelToValues(config: ChannelConfig): ChannelFormValues {
  const base = {
    ...EMPTY_CHANNEL_VALUES,
    kind: config.kind,
    id: config.id,
    label: config.label,
    enabled: config.enabled,
  };
  if (config.kind === "bridge") {
    return {
      ...base,
      endpointAId: config.endpointAId,
      endpointBId: config.endpointBId,
      pairing: config.pairing,
      pairingReceiver: config.pairingReceiver,
      bufferBytes: String(config.bufferBytes),
      tapFramer: config.tapFramer,
      tapRetention: retentionToValues(config.tapRetention),
      retry: retryToValues(config.retry),
    };
  }
  return {
    ...base,
    source: sourceToValues(config.source),
    filters: config.filters.map(filterToValues),
    destinations: config.destinations.map(destinationToValues),
    retention: retentionToValues(config.retention),
  };
}

/** The channel the form describes; null until a kind is chosen. */
export function valuesToChannel(values: ChannelFormValues): ChannelConfig | null {
  if (values.kind === null) return null;
  const base = {
    id: values.id.trim(),
    label: values.label.trim(),
    enabled: values.enabled,
  };
  if (values.kind === "bridge") {
    return {
      ...base,
      kind: "bridge",
      endpointAId: values.endpointAId ?? "",
      endpointBId: values.endpointBId ?? "",
      pairing: values.pairing,
      pairingReceiver: values.pairing === "receiver_then_sender" ? values.pairingReceiver : null,
      bufferBytes: num(values.bufferBytes),
      retry: valuesToRetry(values.retry),
      tapFramer: values.tapFramer,
      tapRetention: valuesToRetention(values.tapRetention),
    };
  }
  return {
    ...base,
    kind: "message",
    source: {
      ...valuesToConnector(values.source),
      duplicateDetection: values.source.duplicateDetection,
    },
    filters: values.filters.map(valuesToFilter),
    destinations: values.destinations.map(valuesToDestination),
    retention: valuesToRetention(values.retention),
  };
}
