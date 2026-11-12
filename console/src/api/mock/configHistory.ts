import { clone } from "@/lib/clone";
import type {
  ChannelConfig,
  ChannelOutcome,
  ConfigDocument,
  ConfigVerification,
  ConfigVersionSummary,
  EndpointConfig,
  MessageChannelConfig,
} from "../types";
import { channelsUsing } from "./endpointInfo";
import { mappingsFor } from "./mappingCatalog";
import { DAY, HOUR, iso } from "./util";

export interface StoredVersion {
  summary: ConfigVersionSummary;
  document: ConfigDocument;
}

interface Step {
  /** The OS account that applied it. */
  by: string;
  /** Why. */
  note: string;
  /** Reverts this step on a document, so older versions can be derived from the current one. */
  undo: (document: ConfigDocument) => void;
  /**
   * What applying it did to the channels it touched, from the document it produced; every other
   * channel was unchanged. Absent for the startup import, which records no outcomes.
   */
  outcomes?: (document: ConfigDocument) => ChannelOutcome[];
}

const outcome = (channelId: string, kind: ChannelOutcome["outcome"]): ChannelOutcome => ({
  channelId,
  outcome: kind,
});

/** The channels that use an edited endpoint were restarted. */
function restartedUsers(document: ConfigDocument, endpointId: string): ChannelOutcome[] {
  return channelsUsing(document, endpointId).map((c) => outcome(c.id, "restarted"));
}

function endpoint(document: ConfigDocument, id: string): EndpointConfig | undefined {
  return document.endpoints.find((e) => e.id === id);
}

function channel(document: ConfigDocument, id: string): ChannelConfig | undefined {
  return document.channels.find((c) => c.id === id);
}

function removeChannels(document: ConfigDocument, ids: string[]): void {
  document.channels = document.channels.filter((c) => !ids.includes(c.id));
}

function removeEndpoints(document: ConfigDocument, ids: string[]): void {
  document.endpoints = document.endpoints.filter((e) => !ids.includes(e.id));
}

function legacyChannel(): MessageChannelConfig {
  const mapping = mappingsFor("hl7_mllp", "astm");
  return {
    kind: "message",
    id: "ch-msg-cobas-legacy",
    label: "Cobas legacy orders",
    enabled: true,
    source: {
      id: "src",
      endpointId: "ep-lis-orders",
      protocol: "hl7_mllp",
      settings: { ackMode: "original" },
      duplicateDetection: true,
    },
    filters: [],
    destinations: [
      {
        id: "dst-1",
        label: "cobas order port",
        endpointId: "ep-cobas-orders",
        protocol: "astm",
        settings: { astmRole: "computer" },
        enabled: true,
        filters: [],
        mappings: mapping.rules,
        codeTables: mapping.codeTables,
        retry: { initialMs: 2000, maxMs: 300_000, maxAttempts: 5 },
      },
    ],
    retention: { days: 30, maxMessages: 2_000_000 },
  };
}

/** Oldest first. Step `k` created version `current - (steps - 1) + k`; the last one made the current version. */
const STEPS: Step[] = [
  {
    by: "system",
    note: "imported from the runtime file",
    undo: () => {},
  },
  {
    by: "LAB\\jdoe",
    note: "Add the dashboard feed for the clinic display",
    outcomes: () => [outcome("ch-msg-dashboard", "started")],
    undo: (d) => {
      removeChannels(d, ["ch-msg-dashboard"]);
      removeEndpoints(d, ["ep-dashboard"]);
    },
  },
  {
    by: "LAB\\jdoe",
    note: "LIS moved to a new address after the server migration",
    outcomes: (d) => restartedUsers(d, "ep-lis-out"),
    undo: (d) => {
      const e = endpoint(d, "ep-lis-out");
      if (e?.kind === "tcp_client") e.host = "10.0.4.20";
    },
  },
  {
    by: "LAB\\mlopez",
    note: "Allow more retries for the AU680 results",
    outcomes: () => [outcome("ch-msg-au680", "updated")],
    undo: (d) => {
      const c = channel(d, "ch-msg-au680");
      if (c?.kind === "message") {
        for (const destination of c.destinations) destination.retry.maxAttempts = 3;
      }
    },
  },
  {
    by: "LAB\\jdoe",
    note: "Point-of-care glucose export from ward 3",
    outcomes: () => [outcome("ch-msg-poc", "started")],
    undo: (d) => {
      removeChannels(d, ["ch-msg-poc"]);
      removeEndpoints(d, ["ep-poc-in"]);
    },
  },
  {
    by: "LAB\\jdoe",
    note: "Restrict the Sysmex XN-1000 to its own address",
    outcomes: (d) => restartedUsers(d, "ep-xn1000"),
    undo: (d) => {
      const e = endpoint(d, "ep-xn1000");
      if (e?.kind === "tcp_listener") {
        e.allowlist = [];
        e.allowAnyPeer = true;
      }
    },
  },
  {
    by: "LAB\\jdoe",
    note: "Raise the queue warning after the weekend backlog",
    outcomes: () => [],
    undo: (d) => {
      d.alertThresholds.queueDepthWarning = 500;
    },
  },
  {
    by: "LAB\\mlopez",
    note: "Change request CR-1042: Architect retention to 90 days",
    outcomes: () => [outcome("ch-msg-i1000", "updated")],
    undo: (d) => {
      const c = channel(d, "ch-msg-i1000");
      if (c?.kind === "message") c.retention.days = 14;
    },
  },
  {
    by: "LAB\\jdoe",
    note: "Retire the Cobas legacy orders channel",
    outcomes: () => [outcome("ch-msg-cobas-legacy", "removed")],
    undo: (d) => {
      d.channels.push(legacyChannel());
    },
  },
  {
    by: "LAB\\jdoe",
    note: "STA Compact baud rate set to 19200 after the cable swap",
    // The serial port was still held by the vendor's service tool: the channel kept its old definition.
    outcomes: () => [
      { channelId: "ch-msg-sta", outcome: "failed", reason: "Cannot open COM24: access denied" },
    ],
    undo: (d) => {
      const e = endpoint(d, "ep-sta");
      if (e?.kind === "serial") e.baudRate = 9600;
    },
  },
  {
    by: "LAB\\jdoe",
    note: "rollback of 42",
    outcomes: () => [outcome("ch-msg-abl", "updated")],
    undo: (d) => {
      const c = channel(d, "ch-msg-abl");
      if (c?.kind === "message") {
        for (const destination of c.destinations) {
          destination.mappings = destination.mappings.slice(0, 2);
        }
      }
    },
  },
  {
    by: "LAB\\mlopez",
    note: "Reference lab results: send everything, not only finals",
    outcomes: () => [outcome("ch-msg-reflab", "updated")],
    undo: (d) => {
      const c = channel(d, "ch-msg-reflab");
      if (c?.kind === "message") c.filters = [];
    },
  },
  {
    by: "LAB\\jdoe",
    note: "Dashboard feed keeps 30 days",
    outcomes: () => [outcome("ch-msg-dashboard", "updated")],
    undo: (d) => {
      const c = channel(d, "ch-msg-dashboard");
      if (c?.kind === "message") c.retention.days = 7;
    },
  },
];

/** Older versions were signed off by the colleague; the newest two have not been yet. */
/** Every channel of `document`: the step's outcomes, `unchanged` for the rest, then the removed ones. */
function outcomesOf(step: Step, document: ConfigDocument): ChannelOutcome[] {
  if (step.outcomes === undefined) return [];
  const touched = new Map(step.outcomes(document).map((each) => [each.channelId, each]));
  const all = document.channels.map((c) => touched.get(c.id) ?? outcome(c.id, "unchanged"));
  const present = new Set(document.channels.map((c) => c.id));
  return [...all, ...[...touched.values()].filter((each) => !present.has(each.channelId))];
}

function verifications(version: number, current: number, nowMs: number): ConfigVerification[] {
  if (version > current - 2) return [];
  const at = nowMs - 3 * DAY - (current - version) * 29 * HOUR;
  return [
    {
      by: "LAB\\mlopez",
      at: iso(at),
      note: `Validation record VR-2026-${String(version).padStart(3, "0")}`,
    },
  ];
}

/**
 * Every version from the oldest step to `current`, newest last. Older documents are derived by
 * undoing the history steps from the current document, newest first.
 */
export function buildVersionHistory(current: ConfigDocument, nowMs: number): StoredVersion[] {
  const versions: StoredVersion[] = [];
  const document = clone(current);
  for (let k = STEPS.length - 1; k >= 0; k--) {
    const step = STEPS[k] as Step;
    const version = current.version - (STEPS.length - 1) + k;
    versions.unshift({
      summary: {
        version,
        appliedAt: iso(nowMs - 3 * DAY - 2 * HOUR - (current.version - version) * 31 * HOUR),
        appliedBy: step.by,
        note: step.note,
        endpoints: document.endpoints.length,
        channels: document.channels.length,
        verifications: verifications(version, current.version, nowMs),
        outcomes: outcomesOf(step, document),
        current: version === current.version,
      },
      document: { ...clone(document), version },
    });
    step.undo(document);
  }
  return versions;
}
