/**
 * The engine's snapshot (docs/spec/diagnostics.md sections 3 to 6, generated `EngineSnapshot`)
 * to the console's `Snapshot`. Pure. A figure the engine does not report is null (the generated
 * types say which); nothing is invented. The runtime document of the snapshot's config version
 * (null until it has been read) supplies the one thing snapshots do not carry: the queue-depth
 * threshold that pressure is measured against (`alerts.delivery_backlog_count`).
 *
 * Every mapping from an engine enumeration is a `Record` over the generated type, so a value
 * added by a regeneration breaks the build instead of reading as `undefined`.
 */

import type {
  BridgeDetail,
  DestinationSnapshot,
  EngineSnapshot,
  ErrorSummary,
  FlowSnapshot,
  MessageDetail,
  NodeState,
  ChannelSnapshot as RawChannel,
  EndpointSnapshot as RawEndpoint,
  EndpointRef as RawEndpointRef,
  NodeSnapshot as RawNode,
  StopReason as RawStopReason,
  Unresolved,
} from "@/ipc/generated";
import type {
  BridgeStatus,
  BufferStatus,
  ChannelEndpointStatus,
  ChannelSnapshot,
  Counters,
  DestinationSummary,
  EndpointRef,
  EndpointSnapshot,
  LastError,
  NodeSnapshot,
  NodeTotals,
  QueueStatus,
  RecentError,
  Snapshot,
  SourceStatus,
  StopReason,
  StoreStatus,
} from "../types";
import { alertFromIpc, sortAlerts } from "./alerts";
import { endpointSummary } from "./summary";
import type { WireConfig } from "./wire";

const DEFAULT_BACKLOG_THRESHOLD = 1000;

const NODE_STATES: Record<NodeState, NodeSnapshot["state"]> = {
  healthy: "running",
  degraded: "degraded",
  stopped: "stopping",
};

const STOP_REASONS: Record<RawStopReason, StopReason> = {
  operator_paused: "paused",
  disabled_by_config: "disabled",
  removed_by_config: "replaced",
  replaced_by_config: "replaced",
  shutdown: "shutdown",
  store_unavailable: "store_unavailable",
  not_implemented: "not_implemented",
};

// ── Node ───────────────────────────────────────────────────────────────────────────────

function storeFrom({ store, totals }: RawNode): StoreStatus {
  return {
    state: store.state,
    messageCount: totals.messages_stored,
    databaseBytes: store.size_bytes,
    diskFreeBytes: store.free_disk_bytes,
    // Absent from an engine that predates the figure.
    diskTotalBytes: store.total_disk_bytes ?? null,
  };
}

function totalsFrom({ totals }: RawNode): NodeTotals {
  return {
    deliveriesUnresolved: totals.deliveries_unresolved ?? 0,
    deliveriesQueued: totals.deliveries_queued ?? 0,
    alertsWarning: totals.alerts_active.warning,
    alertsCritical: totals.alerts_active.critical,
  };
}

function nodeFrom(node: RawNode): NodeSnapshot {
  return {
    nodeId: node.node_id,
    nodeName: node.display_name,
    version: node.version,
    startedAt: node.started_at,
    state: NODE_STATES[node.node_state],
    hosting: node.hosting_mode,
    configVersion: node.config.active_version ?? 0,
    pendingConfigVersion: node.config.pending_version,
    store: storeFrom(node),
    totals: totalsFrom(node),
  };
}

// ── Channels ───────────────────────────────────────────────────────────────────────────

export function refOf(ref: RawEndpointRef): EndpointRef {
  return { endpointId: ref.endpoint_id, label: ref.label, state: ref.state };
}

function sideOf(ref: RawEndpointRef): ChannelEndpointStatus {
  return {
    endpointId: ref.endpoint_id,
    label: ref.label,
    kind: ref.kind,
    role: ref.role,
    state: ref.state,
    consecutiveFailureCount: ref.consecutive_failures,
    activePeer: ref.active_peer,
    lastActivityAt: ref.last_activity,
  };
}

function bufferOf(direction: BufferStatus["direction"], flow: FlowSnapshot): BufferStatus {
  return {
    direction,
    usedBytes: flow.buffered_bytes,
    capacityBytes: flow.max_bytes,
    segments: flow.buffered_segments,
    maxSegments: flow.max_segments,
    overflowing: flow.overflowing,
  };
}

function buffersOf(bridge: BridgeDetail | null): BufferStatus[] {
  if (bridge === null) return [];
  return [bufferOf("a_to_b", bridge.a_to_b), bufferOf("b_to_a", bridge.b_to_a)];
}

function bridgeFrom(bridge: BridgeDetail): BridgeStatus {
  return {
    sessionState: bridge.session,
    connectStrategy: bridge.connect_strategy,
    receiver: bridge.receiver,
    endpointA: sideOf(bridge.endpoint_a),
    endpointB: sideOf(bridge.endpoint_b),
    bytesAToB: bridge.a_to_b.bytes,
    bytesBToA: bridge.b_to_a.bytes,
    messagesAToB: bridge.a_to_b.messages,
    messagesBToA: bridge.b_to_a.messages,
    framingErrors: bridge.tap.framing_errors,
    tapDropped: bridge.tap.tap_dropped,
    overflowCount: bridge.a_to_b.overflow_count + bridge.b_to_a.overflow_count,
    tap: {
      framer: bridge.tap.framer,
      recordMessages: bridge.tap.record_messages,
      transcriptViewers: bridge.tap.transcript_viewers,
    },
  };
}

function sourceFrom(message: MessageDetail): SourceStatus {
  const { counters } = message;
  return {
    endpoint: sideOf(message.source),
    protocol: message.protocol,
    session: message.session,
    lastReceivedAt: message.last_received_at,
    received: counters.received,
    duplicates: counters.duplicates,
    invalid: counters.invalid,
    filtered: counters.filtered,
    aborted: counters.aborted,
  };
}

const unresolvedCount = (u: Unresolved) => u.rejected + u.timed_out + u.outcome_unknown + u.failed;

function summaryOf(destination: DestinationSnapshot): DestinationSummary {
  return {
    destinationId: destination.destination_id,
    label: destination.label,
    enabled: destination.enabled,
    acknowledged: destination.counters.acknowledged,
    lastAcknowledgedAt: destination.last_ack_at,
  };
}

function queueOf(message: MessageDetail): QueueStatus {
  let depth = 0;
  let oldest: string | null = null;
  for (const { queue } of message.destinations) {
    depth += queue.queued + queue.sending;
    const at = queue.oldest_queued_at;
    if (at !== null && (oldest === null || at < oldest)) oldest = at;
  }
  return { depth, needsAttention: unresolvedCount(message.unresolved), oldestQueuedAt: oldest };
}

/** The fullest buffer, against either of its limits. */
function bufferPressure(buffer: BufferStatus): number {
  if (buffer.overflowing) return 1;
  const bytes = buffer.capacityBytes > 0 ? buffer.usedBytes / buffer.capacityBytes : 0;
  const segments =
    buffer.maxSegments !== null && buffer.maxSegments > 0
      ? buffer.segments / buffer.maxSegments
      : 0;
  return Math.max(bytes, segments);
}

function pressureOf(buffers: BufferStatus[], queue: QueueStatus | null, backlog: number): number {
  const ratios = buffers.map(bufferPressure);
  if (queue !== null) ratios.push(backlog > 0 ? queue.depth / backlog : 0);
  return Math.min(1, Math.max(0, ...ratios));
}

export function lastErrorOf(error: ErrorSummary | null): LastError | null {
  return error === null
    ? null
    : { at: error.at, summary: error.summary, nextAction: error.next_action ?? "" };
}

function recentErrorsOf(channel: RawChannel): RecentError[] {
  return channel.recent_errors.map(({ at, summary }) => ({ at, summary }));
}

/**
 * Bridge: [A, B]. Message channel: [source, ...destinations]. A channel that reports neither
 * (it should not happen) falls back to the endpoints it names, as the endpoint list knows them.
 */
function endpointsOf(channel: RawChannel, known: ReadonlyMap<string, RawEndpoint>): EndpointRef[] {
  const { bridge, message } = channel;
  if (bridge !== null) return [refOf(bridge.endpoint_a), refOf(bridge.endpoint_b)];
  if (message !== null) {
    return [message.source, ...message.destinations.map((d) => d.endpoint)].map(refOf);
  }
  return channel.endpoints.flatMap((id) => {
    const endpoint = known.get(id);
    return endpoint === undefined
      ? []
      : [{ endpointId: id, label: endpoint.label, state: endpoint.state }];
  });
}

/** Bridges count A to B as in and B to A as out; the engine's channel traffic says so already. */
function countersOf(channel: RawChannel): Counters {
  const { traffic } = channel;
  return {
    bytesIn: traffic.bytes_in,
    bytesOut: traffic.bytes_out,
    messagesIn: traffic.messages_in,
    messagesOut: traffic.messages_out,
    retries: channel.retry_count,
  };
}

function channelFrom(
  channel: RawChannel,
  backlog: number,
  known: ReadonlyMap<string, RawEndpoint> = new Map(),
): ChannelSnapshot {
  const buffers = buffersOf(channel.bridge);
  const queue = channel.message === null ? null : queueOf(channel.message);
  return {
    id: channel.channel_id,
    label: channel.label,
    kind: channel.kind,
    enabled: channel.enabled,
    state: channel.state,
    stopReason: channel.stop_reason === null ? null : STOP_REASONS[channel.stop_reason],
    protocol: channel.message?.protocol ?? null,
    endpoints: endpointsOf(channel, known),
    counters: countersOf(channel),
    configVersion: channel.config_version,
    instanceStartedAt: channel.instance_started_at,
    nextRetryAt: channel.next_retry_at,
    recentErrors: recentErrorsOf(channel),
    allowedActions: channel.allowed_actions,
    buffers,
    bridge: channel.bridge === null ? null : bridgeFrom(channel.bridge),
    source: channel.message === null ? null : sourceFrom(channel.message),
    queue,
    ...(channel.message === null
      ? {}
      : { destinations: channel.message.destinations.map(summaryOf) }),
    pressure: pressureOf(buffers, queue, backlog),
    lastActivityAt: channel.traffic.last_activity,
    lastError: lastErrorOf(channel.last_error),
    tapActive: (channel.bridge?.tap.transcript_viewers ?? 0) > 0,
  };
}

// ── Endpoints ──────────────────────────────────────────────────────────────────────────

function endpointFrom(endpoint: RawEndpoint, config: WireConfig | null): EndpointSnapshot {
  const document = config?.endpoints?.find((e) => e.endpoint_id === endpoint.endpoint_id);
  return {
    id: endpoint.endpoint_id,
    label: endpoint.label,
    kind: endpoint.kind,
    enabled: endpoint.enabled,
    state: endpoint.state,
    summary: endpointSummary(document, endpoint),
    counters: {
      bytesIn: endpoint.traffic.bytes_in,
      bytesOut: endpoint.traffic.bytes_out,
      messagesIn: endpoint.traffic.messages_in,
      messagesOut: endpoint.traffic.messages_out,
      retries: endpoint.retry_count,
    },
    lastActivityAt: endpoint.traffic.last_activity,
    stateSince: endpoint.state_since,
    lastError: lastErrorOf(endpoint.last_error),
    channelIds: endpoint.channels,
    heldBy: endpoint.held_by,
    activePeer: endpoint.peer,
    consecutiveFailureCount: endpoint.consecutive_failures,
    nextRetryAt: endpoint.next_retry_at,
  };
}

// ── Snapshot ───────────────────────────────────────────────────────────────────────────

/** `document` is the runtime document of the snapshot's version, when it is known. */
export function snapshotFromIpc(snapshot: EngineSnapshot, document: WireConfig | null): Snapshot {
  const backlog = document?.alerts?.delivery_backlog_count ?? DEFAULT_BACKLOG_THRESHOLD;
  const known = new Map(snapshot.endpoints.map((e) => [e.endpoint_id, e]));
  return {
    takenAt: snapshot.node.captured_at,
    node: nodeFrom(snapshot.node),
    channels: snapshot.channels.map((c) => channelFrom(c, backlog, known)),
    endpoints: snapshot.endpoints.map((e) => endpointFrom(e, document)),
    openAlerts: sortAlerts(snapshot.alerts.map(alertFromIpc)),
  };
}
