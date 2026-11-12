// Snapshots (architecture §6) and the channel and endpoint detail pages.

import type { ChannelConfig, EndpointConfig } from "./config";
import type {
  AlertKind,
  AlertSeverity,
  ChannelKind,
  ChannelState,
  EndpointKind,
  EndpointState,
  PairingStrategy,
  Protocol,
  StopReason,
} from "./core";

export interface Counters {
  bytesIn: number;
  bytesOut: number;
  messagesIn: number;
  messagesOut: number;
  retries: number;
}

export interface LastError {
  at: string;
  summary: string;
  /** What the operator should do next. */
  nextAction: string;
}

export interface EndpointRef {
  endpointId: string;
  /** The endpoint's label at snapshot time, so lists need no second lookup. */
  label: string;
  state: EndpointState;
}

export interface BufferStatus {
  direction: "a_to_b" | "b_to_a";
  usedBytes: number;
  capacityBytes: number;
  segments: number;
  /** Null when the engine does not report the segment limit. */
  maxSegments: number | null;
  /** The buffer filled up and the writer is waiting (never dropping). */
  overflowing: boolean;
}

/** One channel's use of an endpoint (diagnostics.md "Endpoint reference"). */
export interface ChannelEndpointStatus {
  endpointId: string;
  label: string;
  kind: EndpointKind;
  /** `a`, `b`, `source`, or a destination id. */
  role: string;
  state: EndpointState;
  /** Failure streak. */
  consecutiveFailureCount: number;
  /** Peer address for listeners and connected clients. */
  activePeer: string | null;
  /** Last successful I/O on this side; null when the engine does not say. */
  lastActivityAt: string | null;
}

export type SessionState = "waiting_for_peer" | "bridged" | "closing" | "closed";

/** What only a bridge channel has (operator-console.md §4 "Bridge channel page"). */
export interface BridgeStatus {
  sessionState: SessionState;
  connectStrategy: PairingStrategy;
  receiver: "a" | "b" | null;
  endpointA: ChannelEndpointStatus;
  endpointB: ChannelEndpointStatus;
  bytesAToB: number;
  bytesBToA: number;
  /** Framed message counts; null without a tap framer. */
  messagesAToB: number | null;
  messagesBToA: number | null;
  framingErrors: number | null;
  tapDropped: number | null;
  /** Overflow episodes since the instance started. */
  overflowCount: number;
  tap: { framer: string; recordMessages: boolean; transcriptViewers: number } | null;
}

export type SourceSession = "idle" | "receiving" | "waiting_for_peer" | "closed";

/** What only a message channel has: its source connector. */
export interface SourceStatus {
  endpoint: ChannelEndpointStatus;
  protocol: Protocol;
  session: SourceSession;
  lastReceivedAt: string | null;
  received: number;
  duplicates: number;
  invalid: number;
  filtered: number;
  aborted: number;
}

export type ChannelAction = "pause" | "resume" | "restart";

export interface RecentError {
  at: string;
  summary: string;
}

export interface QueueStatus {
  /** Deliveries not yet in a terminal state. */
  depth: number;
  /** Deliveries in `failed`, `rejected`, `timed_out` or `outcome_unknown`. */
  needsAttention: number;
  oldestQueuedAt: string | null;
}

/**
 * What a message channel's snapshot says of one destination: enough to total what it
 * acknowledged without reading the channel's detail.
 */
export interface DestinationSummary {
  destinationId: string;
  label: string;
  enabled: boolean;
  /** Deliveries it acknowledged since the instance started. */
  acknowledged: number;
  lastAcknowledgedAt: string | null;
}

export interface ChannelSnapshot {
  id: string;
  label: string;
  kind: ChannelKind;
  enabled: boolean;
  state: ChannelState;
  stopReason: StopReason | null;
  /** Message channels only. */
  protocol: Protocol | null;
  /** Bridge: [A, B]. Message channel: [source, ...destinations]. */
  endpoints: EndpointRef[];
  counters: Counters;
  /** Version of the configuration the running instance started from; null when unknown. */
  configVersion: number | null;
  /** When the running instance started; null while stopped or when unknown. */
  instanceStartedAt: string | null;
  /** While a retry waits: when it will be attempted. */
  nextRetryAt: string | null;
  /** Newest first. */
  recentErrors: RecentError[];
  /** What the engine allows an operator to do now; null when it does not say (older engines). */
  allowedActions: ChannelAction[] | null;
  /** Bridges only: per-direction buffer fill. */
  buffers: BufferStatus[];
  /** Bridges only. */
  bridge: BridgeStatus | null;
  /** Message channels only. */
  source: SourceStatus | null;
  /** Message channels only. */
  queue: QueueStatus | null;
  /** Message channels only: each destination's acknowledgements; absent when not reported. */
  destinations?: DestinationSummary[];
  /** 0 to 1: the fullest buffer, or queue depth against its alert threshold. */
  pressure: number;
  lastActivityAt: string | null;
  lastError: LastError | null;
  /** Whether the traffic tap is on (a console is watching it). */
  tapActive: boolean;
}

export interface EndpointSnapshot {
  id: string;
  label: string;
  kind: EndpointKind;
  enabled: boolean;
  state: EndpointState;
  /** One line describing where it points: `COM3 · 9600 8N1`, `tcp://10.0.4.12:3001`. */
  summary: string;
  counters: Counters;
  lastActivityAt: string | null;
  /** When the current state began. */
  stateSince: string;
  lastError: LastError | null;
  channelIds: string[];
  /** Channel holding an exclusive resource (serial port, listener socket); null for none. */
  heldBy: string | null;
  /** Connected peer address when known. */
  activePeer: string | null;
  consecutiveFailureCount: number;
  nextRetryAt: string | null;
}

export interface StoreStatus {
  /** `not_configured`: this engine build runs without a message store. */
  state: "ok" | "unavailable" | "not_configured";
  /** Each figure is null when the engine does not report it. */
  messageCount: number | null;
  databaseBytes: number | null;
  diskFreeBytes: number | null;
  diskTotalBytes: number | null;
}

export interface NodeSnapshot {
  nodeId: string;
  nodeName: string;
  version: string;
  startedAt: string;
  /** `degraded` when any channel or endpoint needs attention. */
  state: "running" | "degraded" | "stopping";
  /** Null when the engine does not report how it is hosted. */
  hosting: "service" | "console" | null;
  configVersion: number;
  /** Set while an apply runs. */
  pendingConfigVersion: number | null;
  /** Null when the engine does not report store figures yet. */
  store: StoreStatus | null;
  /** Null when the engine does not report totals yet. */
  totals: NodeTotals | null;
}

export interface NodeTotals {
  /** Rejected, failed, timed out or outcome unknown. */
  deliveriesUnresolved: number;
  deliveriesQueued: number;
  alertsWarning: number;
  alertsCritical: number;
}

export interface Snapshot {
  takenAt: string;
  node: NodeSnapshot;
  channels: ChannelSnapshot[];
  endpoints: EndpointSnapshot[];
  /** Open alerts, most severe first. */
  openAlerts: Alert[];
}

export interface Alert {
  id: string;
  severity: AlertSeverity;
  kind: AlertKind;
  title: string;
  /** More about the condition; empty when the engine sends only a summary. */
  detail: string;
  channelId: string | null;
  endpointId: string | null;
  firstSeenAt: string;
  lastSeenAt: string;
  /** How many times the condition was raised; null when the engine does not count. */
  occurrences: number | null;
  state: "open" | "cleared";
  clearedAt: string | null;
  /** The clear next action the operator should take. */
  nextAction: string;
}

/** The delivery states that wait for an operator decision (operator-console.md section 2). */
export type AttentionState = "rejected" | "failed" | "timed_out" | "outcome_unknown";

/** Delivery counters of one destination since its channel instance started. */
export interface DestinationCounters {
  attempts: number;
  acknowledged: number;
  rejected: number;
  timedOut: number;
  outcomeUnknown: number;
  failed: number;
  bytesOut: number;
}

export interface DestinationStatus {
  /** The destination connector id within the channel. */
  destinationId: string;
  /** The destination's own label (the endpoint's label is in `endpoint`). */
  label: string;
  endpoint: EndpointRef;
  protocol: Protocol;
  /** Deliveries waiting for an attempt (current store count). */
  queued: number;
  /** Deliveries in flight (current store count). */
  sending: number;
  /** Receipt time of the oldest queued delivery; null when nothing waits. */
  oldestQueuedAt: string | null;
  /** Current store counts of deliveries needing attention, by state. */
  unresolved: Record<AttentionState, number>;
  /** Of the failed ones, those whose mapping failed (reason `transform_error`, no attempt made). */
  transformErrors: number;
  /** Since the instance started. */
  counters: DestinationCounters;
  lastAcknowledgedAt: string | null;
  lastAttemptAt: string | null;
  /** Earliest scheduled attempt. */
  nextAttemptAt: string | null;
  lastError: LastError | null;
  /** A disabled destination gets no new deliveries; its queued ones wait. */
  enabled: boolean;
}

export interface ChannelDetail {
  snapshot: ChannelSnapshot;
  config: ChannelConfig;
  /** Message channels only; empty for bridges. */
  destinations: DestinationStatus[];
}

export interface EndpointDetail {
  snapshot: EndpointSnapshot;
  config: EndpointConfig;
  channels: { id: string; label: string; role: string }[];
}
