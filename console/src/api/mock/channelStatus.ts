import type {
  BridgeChannelConfig,
  BridgeStatus,
  BufferStatus,
  ChannelConfig,
  ChannelEndpointStatus,
  ChannelState,
  EndpointConfig,
  EndpointState,
  MessageChannelConfig,
  RecentError,
  SessionState,
  SourceSession,
  SourceStatus,
} from "../types";
import { channelEndpointIds } from "./endpointInfo";
import { hashString } from "./prng";
import { type ChannelRuntime, endpointViewState } from "./runtimeState";
import type { SnapshotInputs } from "./snapshot";
import { iso, SECOND } from "./util";

/** Average size of a buffered segment; the segment limit is the byte limit over this. */
const SEGMENT_BYTES = 256;
/** A listener source that heard from its peer this recently is `receiving`. */
const RECEIVING_MS = 5 * SECOND;

/** A believable peer address for a connected endpoint; null when the transport has none. */
export function peerAddress(endpoint: EndpointConfig): string | null {
  switch (endpoint.kind) {
    case "tcp_listener": {
      const seed = hashString(endpoint.id);
      const entry = endpoint.allowlist[0];
      const octet = 20 + (seed % 200);
      const host = entry?.includes("/")
        ? `${entry.split("/")[0]?.split(".").slice(0, 3).join(".")}.${octet}`
        : (entry ?? `10.0.4.${octet}`);
      return `${host}:${49_152 + (seed % 16_000)}`;
    }
    case "tcp_client":
    case "sftp":
      return `${endpoint.host}:${endpoint.port}`;
    case "serial":
    case "folder":
      return null;
  }
}

/** This channel's use of one of its endpoints (diagnostics.md "Endpoint reference"). */
function sideStatus(
  inputs: SnapshotInputs,
  endpointId: string,
  role: string,
  channelState: ChannelState,
): ChannelEndpointStatus {
  const { document, runtime, derived } = inputs;
  const config = document.endpoints.find((endpoint) => endpoint.id === endpointId);
  const rt = runtime.endpoints.get(endpointId);
  const state = endpointViewState(
    channelState,
    derived.endpoints.get(endpointId) ?? "disconnected",
  );
  const failing = rt?.fault != null && channelState !== "stopped" && state !== "connected";
  return {
    endpointId,
    label: config?.label ?? endpointId,
    kind: config?.kind ?? "tcp_client",
    role,
    state,
    consecutiveFailureCount: failing ? (rt?.failureStreak ?? 0) : 0,
    activePeer: state === "connected" && config !== undefined ? peerAddress(config) : null,
    lastActivityAt: rt?.lastActivityMs == null ? null : iso(rt.lastActivityMs),
  };
}

function sessionOf(state: ChannelState, sides: EndpointState[]): SessionState {
  if (state === "stopped") return "closed";
  if (state === "starting") return "waiting_for_peer";
  if (state !== "retrying") return "bridged";
  const broken = sides.some((side) => side === "retrying" || side === "error");
  return !broken && sides.includes("listening") ? "waiting_for_peer" : "closed";
}

/** Buffer fill (0 to 1) per direction; a stopped channel has no buffers. */
export function bufferFills(runtime: ChannelRuntime, stopped: boolean): [number, number] {
  return stopped ? [0, 0] : [runtime.bufferA, runtime.bufferB];
}

export function bufferStatuses(
  config: BridgeChannelConfig,
  runtime: ChannelRuntime,
  stopped: boolean,
): BufferStatus[] {
  const [fillA, fillB] = bufferFills(runtime, stopped);
  const maxSegments = Math.max(1, Math.floor(config.bufferBytes / SEGMENT_BYTES));
  const status = (
    direction: BufferStatus["direction"],
    fill: number,
    overflowing: boolean,
  ): BufferStatus => {
    const usedBytes = Math.round(fill * config.bufferBytes);
    return {
      direction,
      usedBytes,
      capacityBytes: config.bufferBytes,
      segments: Math.min(maxSegments, Math.ceil(usedBytes / SEGMENT_BYTES)),
      maxSegments,
      overflowing,
    };
  };
  return [
    status("a_to_b", fillA, runtime.overflow?.side === "a"),
    status("b_to_a", fillB, runtime.overflow?.side === "b"),
  ];
}

export function bridgeStatus(
  config: BridgeChannelConfig,
  inputs: SnapshotInputs,
  state: ChannelState,
  runtime: ChannelRuntime,
): BridgeStatus {
  const a = sideStatus(inputs, config.endpointAId, "a", state);
  const b = sideStatus(inputs, config.endpointBId, "b", state);
  const framed = config.tapFramer !== "none";
  const { counters } = runtime;
  return {
    sessionState: sessionOf(state, [a.state, b.state]),
    connectStrategy: config.pairing,
    receiver: config.pairing === "receiver_then_sender" ? config.pairingReceiver : null,
    endpointA: a,
    endpointB: b,
    bytesAToB: counters.bytesIn,
    bytesBToA: counters.bytesOut,
    messagesAToB: framed ? counters.messagesIn : null,
    messagesBToA: framed ? counters.messagesOut : null,
    framingErrors: framed ? runtime.framingErrors : null,
    tapDropped: framed ? runtime.tapDropped : null,
    overflowCount: runtime.overflowCount,
    tap: framed
      ? {
          framer: config.tapFramer,
          recordMessages: true,
          transcriptViewers: runtime.transcriptViewers,
        }
      : null,
  };
}

function sourceSession(
  state: ChannelState,
  side: ChannelEndpointStatus,
  runtime: ChannelRuntime,
  nowMs: number,
): SourceSession {
  if (state === "stopped" || state === "retrying") return "closed";
  if (state === "starting") return "idle";
  if (side.state === "listening") return "waiting_for_peer";
  const last = runtime.lastActivityMs;
  return last !== null && nowMs - last < RECEIVING_MS ? "receiving" : "idle";
}

export function sourceStatus(
  config: MessageChannelConfig,
  inputs: SnapshotInputs,
  state: ChannelState,
  runtime: ChannelRuntime,
): SourceStatus {
  const side = sideStatus(inputs, config.source.endpointId, "source", state);
  return {
    endpoint: side,
    protocol: config.source.protocol,
    session: sourceSession(state, side, runtime, inputs.nowMs),
    lastReceivedAt: runtime.lastActivityMs === null ? null : iso(runtime.lastActivityMs),
    received: runtime.counters.messagesIn,
    ...runtime.source,
  };
}

export function recentErrors(runtime: ChannelRuntime): RecentError[] {
  return runtime.errors.map((error) => ({ at: iso(error.atMs), summary: error.summary }));
}

/** The soonest scheduled retry of an endpoint this channel waits for; null when none waits. */
export function channelNextRetry(
  config: ChannelConfig,
  inputs: SnapshotInputs,
  state: ChannelState,
): string | null {
  if (state === "stopped") return null;
  let soonest: number | null = null;
  for (const id of channelEndpointIds(config)) {
    const endpoint = inputs.derived.endpoints.get(id);
    const at = inputs.runtime.endpoints.get(id)?.nextRetryMs ?? null;
    if (at === null || (endpoint !== "retrying" && endpoint !== "error")) continue;
    if (soonest === null || at < soonest) soonest = at;
  }
  return soonest === null ? null : iso(soonest);
}
