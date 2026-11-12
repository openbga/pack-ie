import type {
  AlertThresholds,
  ChannelConfig,
  Comparison,
  ConfigDocument,
  DestinationConfig,
  FilterRule,
  MessageChannelConfig,
  Protocol,
  RetryPolicy,
  SourceConfig,
} from "../types";
import { bridgeChannels } from "./bridgeCatalog";
import { CHANNEL_META, type ChannelMeta } from "./channelMeta";
import { bridgeSpecs, messageEndpoints } from "./endpointCatalog";
import { connectorSettings, mappingsFor } from "./mappingCatalog";
import type { PanelKey } from "./panels";

type FilterDef = [path: string, operator: Comparison["operator"], value: string];

/** A destination: its endpoint and protocol, and optionally one filter of its own. */
type DestinationDef = [endpointId: string, protocol: Protocol, filter?: FilterDef];

interface MessageDef {
  slug: string;
  label: string;
  source: string;
  protocol: Protocol;
  destinations: DestinationDef[];
  meta: ChannelMeta;
  filters?: FilterDef[];
  enabled?: boolean;
  /** Source duplicate detection; on unless the definition says otherwise. */
  duplicateDetection?: boolean;
}

const RESULT_TYPE: Record<Protocol, string> = {
  hl7_mllp: "ORU^R01",
  astm: "ASTM result",
  au: "AU result",
  delimited_file: "CSV row",
  raw: "Raw frame",
};

function results(
  slug: string,
  label: string,
  source: string,
  protocol: Protocol,
  panel: PanelKey,
  destinations: MessageDef["destinations"],
  weight: number,
  rate: number,
): MessageDef {
  return {
    slug,
    label,
    source,
    protocol,
    destinations,
    filters: [[protocol === "delimited_file" ? "col.sample_id" : "R.3", "exists", ""]],
    meta: {
      panel,
      weight,
      outbound: 20,
      rate,
      types: [RESULT_TYPE[protocol]],
    },
  };
}

const LIS_OUT: MessageDef["destinations"] = [["ep-lis-out", "hl7_mllp"]];

const MESSAGE_DEFS: MessageDef[] = [
  results(
    "au680",
    "AU680 results",
    "ep-au680",
    "au",
    "chem",
    [
      ["ep-lis-out", "hl7_mllp"],
      // Quality-control runs stay out of the archive.
      ["ep-archive", "delimited_file", ["R.3", "not_in", "QC1, QC2"]],
    ],
    14,
    1.4,
  ),
  results("xn1000", "Sysmex XN-1000 results", "ep-xn1000", "astm", "hema", LIS_OUT, 12, 1.1),
  results("xn2000", "Sysmex XN-2000 results", "ep-xn2000", "astm", "hema", LIS_OUT, 8, 0.8),
  results(
    "c311",
    "cobas c 311 results",
    "ep-c311",
    "astm",
    "chem",
    [
      ["ep-lis-out", "hl7_mllp"],
      ["ep-archive", "delimited_file"],
    ],
    10,
    0.9,
  ),
  results("e411", "cobas e 411 results", "ep-e411", "astm", "immuno", LIS_OUT, 8, 0.5),
  results("i1000", "Architect i1000SR results", "ep-i1000", "astm", "immuno", LIS_OUT, 6, 0.4),
  results("vitros350", "Vitros 350 results", "ep-vitros350", "astm", "chem", LIS_OUT, 5, 0.4),
  results(
    "sta",
    "STA Compact results",
    "ep-sta",
    "astm",
    "coag",
    [
      ["ep-lis-out", "hl7_mllp"],
      ["ep-lis-backup", "hl7_mllp"],
    ],
    3,
    0.3,
  ),
  results("urisys", "Urisys results", "ep-urisys", "astm", "urine", LIS_OUT, 4, 0.5),
  results("istat", "i-STAT results", "ep-istat", "astm", "gases", LIS_OUT, 2, 0.2),
  results("d10", "D-10 HbA1c results", "ep-d10", "astm", "hba1c", LIS_OUT, 2, 0.2),
  {
    slug: "qc",
    label: "QC results archive",
    source: "ep-qc-in",
    protocol: "delimited_file",
    destinations: [["ep-archive", "delimited_file"]],
    duplicateDetection: false,
    meta: { panel: "chem", weight: 3, outbound: 0, rate: 0.15, types: ["CSV row"] },
  },
  order("order-au680", "LIS orders to AU680", [["ep-au680-orders", "au"]], 5, [
    ["OBR.4.1", "starts_with", "AU"],
  ]),
  order("order-xn1000", "LIS orders to XN-1000", [["ep-xn-orders", "astm"]], 4, [
    ["OBR.4.1", "starts_with", "CBC"],
  ]),
  order("order-cobas", "LIS orders to cobas", [["ep-cobas-orders", "astm"]], 4, [
    ["OBR.4.1", "starts_with", "CB"],
  ]),
  {
    slug: "orders-import",
    label: "Orders folder import",
    source: "ep-orders-sftp",
    protocol: "delimited_file",
    destinations: [["ep-lis-orders-in", "hl7_mllp"]],
    meta: { panel: "order", weight: 3, outbound: 950, rate: 0.2, types: ["CSV row"] },
  },
  results(
    "reflab",
    "Reference lab results",
    "ep-reflab-sftp",
    "delimited_file",
    "chem",
    LIS_OUT,
    2,
    0.1,
  ),
  {
    slug: "billing",
    label: "Billing export",
    source: "ep-adt-in",
    protocol: "hl7_mllp",
    destinations: [["ep-billing-sftp", "delimited_file"]],
    meta: { panel: "order", weight: 4, outbound: 500, rate: 0.3, types: ["ADT^A08", "ADT^A01"] },
  },
  {
    slug: "worklist",
    label: "Worklist export",
    source: "ep-lis-orders",
    protocol: "hl7_mllp",
    destinations: [["ep-worklist", "delimited_file"]],
    enabled: false,
    meta: { panel: "order", weight: 2, outbound: 900, rate: 0.1, types: ["ORM^O01"] },
  },
  results("poc", "Point-of-care glucose", "ep-poc-in", "delimited_file", "poc", LIS_OUT, 3, 0.3),
  results("abl", "Blood gas results", "ep-abl", "astm", "gases", LIS_OUT, 2, 0.2),
  {
    slug: "outreach",
    label: "Outreach results relay",
    source: "ep-outreach-in",
    protocol: "hl7_mllp",
    destinations: [["ep-outreach-out", "hl7_mllp"]],
    meta: { panel: "chem", weight: 1, outbound: 500, rate: 0.1, types: ["ORU^R01"] },
  },
  order(
    "clinic-orders",
    "Referring clinic orders",
    [["ep-lis-orders-in", "hl7_mllp"]],
    1,
    [],
    "ep-clinic-sftp",
    "delimited_file",
  ),
  {
    slug: "dashboard",
    label: "Dashboard feed",
    source: "ep-dash-src",
    protocol: "hl7_mllp",
    destinations: [["ep-dashboard", "hl7_mllp"]],
    meta: { panel: "chem", weight: 2, outbound: 400, rate: 0.4, types: ["ORU^R01", "ORU^R30"] },
  },
];

function order(
  slug: string,
  label: string,
  destinations: MessageDef["destinations"],
  weight: number,
  filters: NonNullable<MessageDef["filters"]>,
  source = "ep-lis-orders",
  protocol: Protocol = "hl7_mllp",
): MessageDef {
  return {
    slug,
    label,
    source,
    protocol,
    destinations,
    filters,
    meta: {
      panel: "order",
      weight,
      outbound: 980,
      rate: 0.3,
      types: [protocol === "hl7_mllp" ? "ORM^O01" : "CSV row"],
    },
  };
}

function sourceOf(def: MessageDef): SourceConfig {
  return {
    id: "src",
    endpointId: def.source,
    protocol: def.protocol,
    settings: connectorSettings(def.protocol, "source"),
    duplicateDetection: def.duplicateDetection ?? true,
  };
}

const DESTINATION_RETRY: RetryPolicy = { initialMs: 2000, maxMs: 300_000, maxAttempts: 5 };

function filterRules(defs: readonly FilterDef[]): FilterRule[] {
  return defs.map(([path, operator, value], index) => ({
    id: `flt-${index + 1}`,
    path,
    operator,
    value,
    ignoreCase: false,
  }));
}

function destination(
  index: number,
  [endpointId, protocol, filter]: DestinationDef,
  sourceProtocol: Protocol,
  labels: ReadonlyMap<string, string>,
): DestinationConfig {
  const mapping = mappingsFor(sourceProtocol, protocol);
  return {
    id: `dst-${index + 1}`,
    endpointId,
    protocol,
    settings: connectorSettings(protocol, "destination"),
    label: labels.get(endpointId) ?? endpointId,
    enabled: true,
    filters: filterRules(filter ? [filter] : []),
    mappings: mapping.rules,
    codeTables: mapping.codeTables,
    retry: { ...DESTINATION_RETRY },
  };
}

function messageChannel(
  def: MessageDef,
  labels: ReadonlyMap<string, string>,
): MessageChannelConfig {
  const id = `ch-msg-${def.slug}`;
  CHANNEL_META[id] = def.meta;
  return {
    kind: "message",
    id,
    label: def.label,
    enabled: def.enabled ?? true,
    source: sourceOf(def),
    filters: filterRules(def.filters ?? []),
    destinations: def.destinations.map((each, index) =>
      destination(index, each, def.protocol, labels),
    ),
    retention: { days: 30, maxMessages: 2_000_000 },
  };
}

/** The engine's default alert thresholds (docs/spec/configuration.md §4.3). */
const DEFAULT_THRESHOLDS: AlertThresholds = {
  endpointDownSeconds: 60,
  queueDepthWarning: 1000,
  bufferPressurePercent: 80,
  diskFreeMinMib: 1024,
};

/** The version of the active document at the start of every mock run. */
const INITIAL_VERSION = 47;

export function buildInitialDocument(): ConfigDocument {
  const bridges = bridgeSpecs();
  const messageEndpointList = messageEndpoints();
  const labels = new Map(messageEndpointList.map((endpoint) => [endpoint.id, endpoint.label]));
  const channels: ChannelConfig[] = [
    ...bridgeChannels(),
    ...MESSAGE_DEFS.map((def) => messageChannel(def, labels)),
  ];
  return {
    version: INITIAL_VERSION,
    endpoints: [
      ...bridges.flatMap((spec) => [spec.endpointA, spec.endpointB]),
      ...messageEndpointList,
    ],
    channels,
    alertThresholds: DEFAULT_THRESHOLDS,
  };
}

/** A configuration with nothing in it: what an engine has before the first apply. */
export function emptyDocument(): ConfigDocument {
  return {
    version: INITIAL_VERSION,
    endpoints: [],
    channels: [],
    alertThresholds: DEFAULT_THRESHOLDS,
  };
}
