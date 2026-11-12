// Small hand-made engine data for the Operate screens' tests; synthetic, no real identifiers.

import type { EngineApi } from "@/api/engine";
import type {
  Alert,
  BridgeStatus,
  ChannelDetail,
  ChannelEndpointStatus,
  ChannelSnapshot,
  ConnectionStatus,
  Counters,
  DestinationStatus,
  EndpointDetail,
  EndpointSnapshot,
  EngineError,
  Snapshot,
  SourceStatus,
} from "@/api/types";

export const ago = (seconds: number): string => new Date(Date.now() - seconds * 1000).toISOString();

const COUNTERS: Counters = {
  bytesIn: 2048,
  bytesOut: 1024,
  messagesIn: 12,
  messagesOut: 11,
  retries: 0,
};

/** One side of a bridge, as the engine reports it. */
export function side(overrides: Partial<ChannelEndpointStatus> = {}): ChannelEndpointStatus {
  return {
    endpointId: "ep-com3",
    label: "Analyzer COM3",
    kind: "serial",
    role: "a",
    state: "connected",
    consecutiveFailureCount: 0,
    activePeer: null,
    lastActivityAt: ago(8),
    ...overrides,
  };
}

/** A bridged session with traffic both ways and a tap that frames HL7. */
export function bridgeStatus(overrides: Partial<BridgeStatus> = {}): BridgeStatus {
  return {
    sessionState: "bridged",
    connectStrategy: "parallel",
    receiver: null,
    endpointA: side(),
    endpointB: side({
      endpointId: "ep-lis",
      label: "LIS listener",
      kind: "tcp_listener",
      role: "b",
      state: "listening",
      activePeer: "10.0.4.12:51044",
      lastActivityAt: ago(120),
    }),
    bytesAToB: 2048,
    bytesBToA: 512,
    messagesAToB: null,
    messagesBToA: null,
    framingErrors: null,
    tapDropped: null,
    overflowCount: 0,
    tap: null,
    ...overrides,
  };
}

export function channel(overrides: Partial<ChannelSnapshot> = {}): ChannelSnapshot {
  return {
    id: "ch-alpha",
    label: "Alpha bridge",
    kind: "bridge",
    enabled: true,
    state: "running",
    stopReason: null,
    protocol: null,
    endpoints: [
      { endpointId: "ep-com3", label: "Analyzer COM3", state: "connected" },
      { endpointId: "ep-lis", label: "LIS listener", state: "listening" },
    ],
    counters: COUNTERS,
    configVersion: 7,
    instanceStartedAt: ago(3600),
    nextRetryAt: null,
    allowedActions: null,
    recentErrors: [],
    buffers: [
      {
        direction: "a_to_b",
        usedBytes: 1024,
        capacityBytes: 65_536,
        segments: 2,
        maxSegments: 256,
        overflowing: false,
      },
      {
        direction: "b_to_a",
        usedBytes: 0,
        capacityBytes: 65_536,
        segments: 0,
        maxSegments: 256,
        overflowing: false,
      },
    ],
    bridge: bridgeStatus(),
    source: null,
    queue: null,
    pressure: 0.02,
    lastActivityAt: ago(120),
    lastError: null,
    tapActive: false,
    ...overrides,
  };
}

/** A message channel's source connector, receiving. */
export function sourceStatus(overrides: Partial<SourceStatus> = {}): SourceStatus {
  return {
    endpoint: side({
      endpointId: "ep-src",
      label: "Chemistry source",
      kind: "tcp_listener",
      role: "source",
      state: "connected",
      activePeer: "10.0.4.20:50321",
    }),
    protocol: "hl7_mllp",
    session: "receiving",
    lastReceivedAt: ago(15),
    received: 120,
    duplicates: 2,
    invalid: 1,
    filtered: 3,
    aborted: 0,
    ...overrides,
  };
}

export function messageChannel(overrides: Partial<ChannelSnapshot> = {}): ChannelSnapshot {
  return channel({
    id: "ch-hl7",
    label: "Results to LIS",
    kind: "message",
    protocol: "hl7_mllp",
    bridge: null,
    source: sourceStatus(),
    endpoints: [
      { endpointId: "ep-src", label: "Chemistry source", state: "connected" },
      { endpointId: "ep-lis", label: "LIS listener", state: "listening" },
    ],
    buffers: [],
    queue: { depth: 4, needsAttention: 0, oldestQueuedAt: ago(30) },
    ...overrides,
  });
}

export function endpoint(overrides: Partial<EndpointSnapshot> = {}): EndpointSnapshot {
  return {
    id: "ep-com3",
    label: "Analyzer COM3",
    kind: "serial",
    enabled: true,
    state: "connected",
    summary: "COM3 · 9600 8N1",
    counters: COUNTERS,
    lastActivityAt: ago(60),
    stateSince: ago(3600),
    lastError: null,
    channelIds: ["ch-alpha"],
    heldBy: "ch-alpha",
    activePeer: null,
    consecutiveFailureCount: 0,
    nextRetryAt: null,
    ...overrides,
  };
}

export function alert(overrides: Partial<Alert> = {}): Alert {
  return {
    id: "al-1",
    severity: "critical",
    kind: "endpoint_down",
    title: "Endpoint down",
    detail: "Analyzer COM3 has been disconnected for 10 minutes.",
    channelId: "ch-alpha",
    endpointId: "ep-com3",
    firstSeenAt: ago(600),
    lastSeenAt: ago(5),
    occurrences: 3,
    state: "open",
    clearedAt: null,
    nextAction: "Check the serial cable and the analyzer's power.",
    ...overrides,
  };
}

export function snapshot(overrides: Partial<Snapshot> = {}): Snapshot {
  return {
    takenAt: new Date().toISOString(),
    node: {
      nodeId: "node-1",
      nodeName: "Lab PC 1",
      version: "0.1.0",
      startedAt: ago(90_000),
      state: "running",
      hosting: "service",
      configVersion: 7,
      pendingConfigVersion: null,
      totals: { deliveriesUnresolved: 0, deliveriesQueued: 4, alertsWarning: 0, alertsCritical: 0 },
      store: {
        state: "ok",
        messageCount: 1234,
        databaseBytes: 5 * 1024 * 1024,
        diskFreeBytes: 50 * 1024 ** 3,
        diskTotalBytes: 100 * 1024 ** 3,
      },
    },
    channels: [channel(), messageChannel()],
    endpoints: [
      endpoint(),
      endpoint({ id: "ep-lis", label: "LIS listener", kind: "tcp_listener", state: "listening" }),
    ],
    openAlerts: [],
    ...overrides,
  };
}

/** A channel that has stopped on an error, with the error and what to do about it. */
export function failingChannel(overrides: Partial<ChannelSnapshot> = {}): ChannelSnapshot {
  return channel({
    id: "ch-zulu",
    label: "Zulu bridge",
    state: "degraded",
    pressure: 0.97,
    lastError: {
      at: ago(30),
      summary: "Buffer full: the LIS stopped reading",
      nextAction: "Check that the LIS listener is running, then restart the channel.",
    },
    ...overrides,
  });
}

export function destination(overrides: Partial<DestinationStatus> = {}): DestinationStatus {
  return {
    destinationId: "dst-1",
    label: "LIS results",
    endpoint: { endpointId: "ep-lis", label: "LIS listener", state: "listening" },
    protocol: "hl7_mllp",
    queued: 4,
    sending: 0,
    oldestQueuedAt: ago(45),
    unresolved: { rejected: 0, failed: 2, timed_out: 1, outcome_unknown: 0 },
    transformErrors: 1,
    counters: {
      attempts: 47,
      acknowledged: 40,
      rejected: 0,
      timedOut: 1,
      outcomeUnknown: 0,
      failed: 2,
      bytesOut: 24_000,
    },
    lastAcknowledgedAt: ago(90),
    lastAttemptAt: ago(20),
    nextAttemptAt: null,
    lastError: null,
    enabled: true,
    ...overrides,
  };
}

export function channelDetail(
  snapshotOf: ChannelSnapshot = channel(),
  destinations: DestinationStatus[] = [],
): ChannelDetail {
  const config: ChannelDetail["config"] =
    snapshotOf.kind === "bridge"
      ? {
          kind: "bridge",
          id: snapshotOf.id,
          label: snapshotOf.label,
          enabled: true,
          endpointAId: "ep-com3",
          endpointBId: "ep-lis",
          pairing: "parallel",
          pairingReceiver: null,
          bufferBytes: 65_536,
          retry: { initialMs: 1000, maxMs: 30_000, maxAttempts: 5 },
          tapFramer: "none",
          tapRetention: { days: 30, maxMessages: null },
        }
      : {
          kind: "message",
          id: snapshotOf.id,
          label: snapshotOf.label,
          enabled: true,
          source: {
            id: "src",
            endpointId: "ep-src",
            protocol: "hl7_mllp",
            settings: {},
            duplicateDetection: true,
          },
          filters: [],
          destinations: [],
          retention: { days: 30, maxMessages: null },
        };
  return { snapshot: snapshotOf, config, destinations };
}

export function endpointDetail(
  snapshotOf: EndpointSnapshot = endpoint(),
  channels: EndpointDetail["channels"] = [
    { id: "ch-alpha", label: "Alpha bridge", role: "Side A" },
  ],
): EndpointDetail {
  return {
    snapshot: snapshotOf,
    config: {
      kind: "serial",
      id: snapshotOf.id,
      label: snapshotOf.label,
      enabled: true,
      port: "COM3",
      baudRate: 9600,
      dataBits: 8,
      parity: "none",
      stopBits: 1,
      flowControl: "none",
    },
    channels,
  };
}

export function connection(
  state: ConnectionStatus["state"] = "connected",
  detail: string | null = null,
): ConnectionStatus {
  return { state, endpoint: "/run/packie.sock", detail, since: ago(600), actor: "LAB\\jdoe" };
}

export function engineError(
  code: EngineError["code"],
  message = "Something went wrong.",
): EngineError {
  return { code, message, fieldErrors: [], correlationId: "corr-1" };
}

/** A stub engine with only the methods a test provides. */
export const stubApi = (methods: Partial<EngineApi>): EngineApi => methods as unknown as EngineApi;
