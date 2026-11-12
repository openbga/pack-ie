import type {
  Alert,
  ChannelConfig,
  ChannelSnapshot,
  ConfigDocument,
  EndpointConfig,
  EndpointRef,
  EndpointSnapshot,
  EndpointState,
  LastError,
  NodeSnapshot,
  NodeTotals,
  Snapshot,
} from "../types";
import {
  bridgeStatus,
  bufferFills,
  bufferStatuses,
  channelNextRetry,
  peerAddress,
  recentErrors,
  sourceStatus,
} from "./channelStatus";
import { destinationStatuses } from "./destinations";
import { channelEndpointIds, channelsUsing, endpointSummary } from "./endpointInfo";
import type { Runtime } from "./runtime";
import {
  type ChannelRuntime,
  type DerivedStates,
  endpointViewState,
  queuePressure,
  toLastError,
} from "./runtimeState";
import { iso } from "./util";

export interface SnapshotInputs {
  document: ConfigDocument;
  runtime: Runtime;
  derived: DerivedStates;
  openAlerts: Alert[];
  nowMs: number;
  startedAtMs: number;
  messageCount: number;
  /** The version of an apply that is running; null otherwise. */
  pendingConfigVersion: number | null;
  node: { id: string; name: string; version: string };
}

/** The side of the channel an endpoint is, as the engine's error summaries name it. */
function sideOf(config: ChannelConfig, endpointId: string): string {
  if (config.kind === "bridge") return config.endpointAId === endpointId ? "A" : "B";
  if (config.source.endpointId === endpointId) return "source";
  const destination = config.destinations.find((d) => d.endpointId === endpointId);
  return `destination ${destination?.id ?? endpointId}`;
}

const OPERATIONS: Record<EndpointConfig["kind"], string> = {
  serial: "open",
  tcp_listener: "bind",
  tcp_client: "connect",
  sftp: "connect",
  folder: "read",
};

/**
 * An endpoint's failure in the engine's one error pattern (diagnostics.md 7.4): which channel,
 * which side, which endpoint, the operation and the cause, then what happens next.
 */
function inEnginePattern(
  config: ChannelConfig,
  endpoint: EndpointConfig | undefined,
  error: LastError,
): LastError {
  if (endpoint === undefined) return error;
  const cause = error.summary.replace(/[.\s]+$/, "");
  const summary = `Channel ${config.id} ${sideOf(config, endpoint.id)} endpoint "${endpoint.label}" ${OPERATIONS[endpoint.kind]} failed: ${cause}.`;
  return { ...error, summary };
}

/** The error to show for a channel: the failing endpoint's, the channel's own, or its last one. */
function channelError(
  config: ChannelConfig,
  runtime: ChannelRuntime,
  inputs: SnapshotInputs,
): LastError | null {
  const { derived, document } = inputs;
  const stopped = derived.channels.get(config.id)?.state === "stopped";
  for (const id of channelEndpointIds(config)) {
    const state = derived.endpoints.get(id);
    const fault = inputs.runtime.endpoints.get(id)?.fault;
    if (!stopped && fault !== null && fault !== undefined && state !== "disabled") {
      const error = toLastError(fault.error);
      return error === null
        ? null
        : inEnginePattern(
            config,
            document.endpoints.find((e) => e.id === id),
            error,
          );
    }
  }
  return toLastError(runtime.stopError ?? runtime.lastError);
}

export function endpointRefs(
  config: ChannelConfig,
  document: ConfigDocument,
  derived: DerivedStates,
): EndpointRef[] {
  const refs: EndpointRef[] = [];
  const channel = derived.channels.get(config.id)?.state ?? "stopped";
  for (const id of channelEndpointIds(config)) {
    const endpoint = document.endpoints.find((e) => e.id === id);
    refs.push({
      endpointId: id,
      label: endpoint?.label ?? id,
      state: endpointViewState(channel, derived.endpoints.get(id) ?? "disconnected"),
    });
  }
  return refs;
}

function queueStatus(rt: ChannelRuntime, nowMs: number) {
  return {
    depth: rt.queueDepth,
    needsAttention: rt.needsAttention,
    oldestQueuedAt:
      rt.queueDepth > 0 ? iso(nowMs - Math.min(rt.queueDepth * 4000 + 2000, 6 * 3_600_000)) : null,
  };
}

export function channelSnapshot(
  config: ChannelConfig,
  inputs: SnapshotInputs,
): ChannelSnapshot | null {
  const { document, runtime, derived, nowMs } = inputs;
  const rt = runtime.channels.get(config.id);
  const state = derived.channels.get(config.id);
  if (rt === undefined || state === undefined) return null;
  const stopped = state.state === "stopped";
  const bridge = config.kind === "bridge" ? config : null;
  const message = config.kind === "message" ? config : null;
  const pressure =
    bridge !== null
      ? Math.max(...bufferFills(rt, stopped))
      : queuePressure(rt, document.alertThresholds.queueDepthWarning);
  return {
    id: config.id,
    label: config.label,
    kind: config.kind,
    enabled: config.enabled,
    state: state.state,
    stopReason: state.stopReason,
    protocol: message?.source.protocol ?? null,
    endpoints: endpointRefs(config, document, derived),
    counters: { ...rt.counters },
    configVersion: rt.configVersion,
    instanceStartedAt: stopped ? null : iso(rt.instanceStartMs),
    nextRetryAt: channelNextRetry(config, inputs, state.state),
    allowedActions: null,
    recentErrors: recentErrors(rt),
    buffers: bridge === null ? [] : bufferStatuses(bridge, rt, stopped),
    bridge: bridge === null ? null : bridgeStatus(bridge, inputs, state.state, rt),
    source: message === null ? null : sourceStatus(message, inputs, state.state, rt),
    queue: message === null ? null : queueStatus(rt, nowMs),
    ...(message === null
      ? {}
      : {
          destinations: destinationStatuses(
            message,
            inputs,
            endpointRefs(config, document, derived),
          ).map((d) => ({
            destinationId: d.destinationId,
            label: d.label,
            enabled: d.enabled,
            acknowledged: d.counters.acknowledged,
            lastAcknowledgedAt: d.lastAcknowledgedAt,
          })),
        }),
    pressure: Number(pressure.toFixed(3)),
    lastActivityAt: rt.lastActivityMs === null ? null : iso(rt.lastActivityMs),
    lastError: channelError(config, rt, inputs),
    tapActive: rt.transcriptViewers > 0,
  };
}

/** The channel that holds an exclusive resource (serial port, listener socket) right now. */
function holder(config: EndpointConfig, inputs: SnapshotInputs, state: EndpointState) {
  const exclusive = config.kind === "serial" || config.kind === "tcp_listener";
  const held =
    state === "connected" ||
    (config.kind === "tcp_listener" && (state === "listening" || state === "disconnected"));
  if (!exclusive || !held) return null;
  const channel = channelsUsing(inputs.document, config.id).find(
    (candidate) => inputs.derived.raw.get(candidate.id) !== "stopped",
  );
  return channel?.id ?? null;
}

export function endpointSnapshot(
  config: EndpointConfig,
  inputs: SnapshotInputs,
): EndpointSnapshot | null {
  const { document, runtime, derived } = inputs;
  const rt = runtime.endpoints.get(config.id);
  const state = derived.endpoints.get(config.id);
  if (rt === undefined || state === undefined) return null;
  const retrying = state === "retrying" || state === "error";
  return {
    id: config.id,
    label: config.label,
    kind: config.kind,
    enabled: config.enabled,
    state,
    summary: endpointSummary(config),
    counters: { ...rt.counters },
    lastActivityAt: rt.lastActivityMs === null ? null : iso(rt.lastActivityMs),
    stateSince: iso(rt.stateSince),
    lastError: toLastError(rt.fault?.error ?? rt.lastError),
    channelIds: channelsUsing(document, config.id).map((channel) => channel.id),
    heldBy: holder(config, inputs, state),
    activePeer: state === "connected" ? peerAddress(config) : null,
    consecutiveFailureCount: rt.failureStreak,
    nextRetryAt: retrying && rt.nextRetryMs !== null ? iso(rt.nextRetryMs) : null,
  };
}

function isUnhealthy(channels: ChannelSnapshot[], endpoints: EndpointSnapshot[]): boolean {
  const channelBad = channels.some(
    (c) =>
      c.state === "degraded" ||
      c.state === "retrying" ||
      (c.state === "stopped" && (c.stopReason === "fatal" || c.stopReason === "configuration")),
  );
  return channelBad || endpoints.some((e) => e.state === "error" || e.state === "retrying");
}

function nodeTotals(channels: ChannelSnapshot[], openAlerts: Alert[]): NodeTotals {
  const sum = (value: (channel: ChannelSnapshot) => number) =>
    channels.reduce((total, channel) => total + value(channel), 0);
  return {
    deliveriesUnresolved: sum((c) => c.queue?.needsAttention ?? 0),
    deliveriesQueued: sum((c) => c.queue?.depth ?? 0),
    alertsWarning: openAlerts.filter((a) => a.severity === "warning").length,
    alertsCritical: openAlerts.filter((a) => a.severity === "critical").length,
  };
}

function nodeSnapshot(
  inputs: SnapshotInputs,
  channels: ChannelSnapshot[],
  endpoints: EndpointSnapshot[],
): NodeSnapshot {
  const { runtime, node, document } = inputs;
  return {
    nodeId: node.id,
    nodeName: node.name,
    version: node.version,
    startedAt: iso(inputs.startedAtMs),
    state: isUnhealthy(channels, endpoints) ? "degraded" : "running",
    hosting: "service",
    configVersion: document.version,
    pendingConfigVersion: inputs.pendingConfigVersion,
    totals: nodeTotals(channels, inputs.openAlerts),
    store: {
      state: "ok",
      messageCount: inputs.messageCount,
      databaseBytes: Math.round(runtime.disk.databaseBytes),
      diskFreeBytes: Math.round(runtime.disk.freeBytes),
      diskTotalBytes: runtime.disk.totalBytes,
    },
  };
}

export function buildSnapshot(inputs: SnapshotInputs): Snapshot {
  const channels: ChannelSnapshot[] = [];
  for (const config of inputs.document.channels) {
    const snapshot = channelSnapshot(config, inputs);
    if (snapshot !== null) channels.push(snapshot);
  }
  const endpoints: EndpointSnapshot[] = [];
  for (const config of inputs.document.endpoints) {
    const snapshot = endpointSnapshot(config, inputs);
    if (snapshot !== null) endpoints.push(snapshot);
  }
  return {
    takenAt: iso(inputs.nowMs),
    node: nodeSnapshot(inputs, channels, endpoints),
    channels,
    endpoints,
    openAlerts: inputs.openAlerts,
  };
}
