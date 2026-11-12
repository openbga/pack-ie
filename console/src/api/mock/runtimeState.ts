import type {
  ChannelConfig,
  ChannelState,
  Counters,
  EndpointConfig,
  EndpointState,
  LastError,
  StopReason,
} from "../types";
import { channelEndpointIds } from "./endpointInfo";
import { clamp, iso } from "./util";

/** An error as the mock keeps it: a time in milliseconds instead of a string. */
export interface ErrorInfo {
  atMs: number;
  summary: string;
  nextAction: string;
}

export function toLastError(error: ErrorInfo | null): LastError | null {
  if (error === null) return null;
  return { at: iso(error.atMs), summary: error.summary, nextAction: error.nextAction };
}

export interface EndpointFault {
  state: "error" | "retrying" | "disconnected";
  error: ErrorInfo;
  /** When the fault clears by itself; null for faults that stay until the config changes. */
  untilMs: number | null;
}

/** What a stretch of traffic adds to a channel; for a bridge `In` is A to B and `Out` B to A. */
export type TrafficDelta = Pick<Counters, "bytesIn" | "bytesOut" | "messagesIn" | "messagesOut">;

/** Message channel source counters beyond `Counters.messagesIn` (which is `received`). */
export interface SourceCounters {
  duplicates: number;
  invalid: number;
  filtered: number;
  aborted: number;
}

/** A bridge buffer overflow episode: which direction is blocked and the earliest it can end. */
interface OverflowEpisode {
  side: "a" | "b";
  untilMs: number;
}

/** How many recent errors a channel keeps. */
export const RECENT_ERRORS = 10;

/**
 * The mutable state of one channel instance. `counters` and everything next to it count since
 * the instance started: a restart or resume zeroes them. For a bridge `bytesIn`/`messagesIn`
 * count A to B and `bytesOut`/`messagesOut` count B to A.
 */
export interface ChannelRuntime {
  stop: StopReason | null;
  stopError: ErrorInfo | null;
  /** The channel reports `starting` until this time. */
  startingUntil: number;
  counters: Counters;
  lastActivityMs: number | null;
  lastError: ErrorInfo | null;
  /** Newest first, at most `RECENT_ERRORS`. */
  errors: ErrorInfo[];
  /** When the current instance started, and the configuration version it started from. */
  instanceStartMs: number;
  configVersion: number;
  /** Deliveries acknowledged by earlier instances (the store keeps them across restarts). */
  retainedAcked: number;
  /** Buffer fill, 0 to 1, per direction (bridges). */
  bufferA: number;
  bufferB: number;
  overflow: OverflowEpisode | null;
  overflowCount: number;
  framingErrors: number;
  tapDropped: number;
  /** Message channels: source counters. */
  source: SourceCounters;
  /** Message channels: pending deliveries, the level it drifts around, and failed ones. */
  queueDepth: number;
  queueBase: number;
  needsAttention: number;
  /** Transcript viewers: 0 or 1 (one viewer per channel). */
  transcriptViewers: number;
}

/** Appends to a newest-first error list, bounded. */
export function pushError(errors: ErrorInfo[], error: ErrorInfo): void {
  errors.unshift(error);
  if (errors.length > RECENT_ERRORS) errors.length = RECENT_ERRORS;
}

export interface EndpointRuntime {
  counters: Counters;
  lastActivityMs: number | null;
  lastState: EndpointState;
  stateSince: number;
  lastError: ErrorInfo | null;
  fault: EndpointFault | null;
  /** Listeners: whether a peer is connected right now. */
  peerConnected: boolean;
  /** Failed attempts in a row while the fault lasts; 0 when healthy or not run by any channel. */
  failureStreak: number;
  /** When the next retry runs while the endpoint is `retrying` or `error`. */
  nextRetryMs: number | null;
  lastAttemptMs: number | null;
}

type RawChannelStatus = "stopped" | "starting" | "active";

function rawStatus(config: ChannelConfig, runtime: ChannelRuntime, now: number): RawChannelStatus {
  if (!config.enabled || runtime.stop !== null) return "stopped";
  return runtime.startingUntil > now ? "starting" : "active";
}

function endpointStateFor(
  config: EndpointConfig,
  runtime: EndpointRuntime,
  usage: RawChannelStatus[],
): EndpointState {
  if (!config.enabled) return "disabled";
  if (usage.every((status) => status === "stopped")) return "disconnected";
  if (runtime.fault !== null) return runtime.fault.state;
  if (!usage.includes("active")) return "connecting";
  if (config.kind === "tcp_listener") return runtime.peerConnected ? "connected" : "listening";
  return "connected";
}

/** A side is ready when its transport is up: a client connected, a port open, a socket bound. */
function notReady(state: EndpointState): boolean {
  return (
    state === "error" || state === "retrying" || state === "connecting" || state === "disconnected"
  );
}

/**
 * Bridge: running only while the session is bridged, so a listener without a peer reads
 * `retrying`; `degraded` is an overflow episode. Message channel: `retrying` while the source
 * is not ready, `degraded` while a destination is not.
 */
function runningState(
  config: ChannelConfig,
  runtime: ChannelRuntime,
  endpointStates: EndpointState[],
): ChannelState {
  const usable = endpointStates.filter((state) => state !== "disabled");
  if (config.kind === "bridge") {
    if (usable.some((state) => state !== "connected")) return "retrying";
    return runtime.overflow === null ? "running" : "degraded";
  }
  const [source, ...destinations] = usable;
  if (source !== undefined && notReady(source)) return "retrying";
  return destinations.some(notReady) ? "degraded" : "running";
}

function channelStateFor(
  config: ChannelConfig,
  runtime: ChannelRuntime,
  endpointStates: EndpointState[],
  now: number,
): { state: ChannelState; stopReason: StopReason | null } {
  if (!config.enabled) return { state: "stopped", stopReason: "disabled" };
  if (runtime.stop !== null) return { state: "stopped", stopReason: runtime.stop };
  if (runtime.startingUntil > now) return { state: "starting", stopReason: null };
  return { state: runningState(config, runtime, endpointStates), stopReason: null };
}

/**
 * An endpoint as one channel sees it: stopped channels have let go of it, starting ones are
 * still connecting, failures show through.
 */
export function endpointViewState(channel: ChannelState, endpoint: EndpointState): EndpointState {
  if (endpoint === "disabled") return "disabled";
  if (channel === "stopped") return "disconnected";
  if (endpoint === "error" || endpoint === "retrying") return endpoint;
  return channel === "starting" ? "connecting" : endpoint;
}

/** The states of every channel and endpoint at one moment, derived from configuration and faults. */
export interface DerivedStates {
  channels: Map<string, { state: ChannelState; stopReason: StopReason | null }>;
  endpoints: Map<string, EndpointState>;
  raw: Map<string, RawChannelStatus>;
}

export function deriveStates(
  channels: ChannelConfig[],
  endpoints: EndpointConfig[],
  channelRuntime: Map<string, ChannelRuntime>,
  endpointRuntime: Map<string, EndpointRuntime>,
  now: number,
): DerivedStates {
  const raw = new Map<string, RawChannelStatus>();
  const usage = new Map<string, RawChannelStatus[]>();
  for (const channel of channels) {
    const runtime = channelRuntime.get(channel.id);
    if (runtime === undefined) continue;
    const status = rawStatus(channel, runtime, now);
    raw.set(channel.id, status);
    for (const endpointId of new Set(channelEndpointIds(channel))) {
      usage.set(endpointId, [...(usage.get(endpointId) ?? []), status]);
    }
  }
  const endpointStates = new Map<string, EndpointState>();
  for (const endpoint of endpoints) {
    const runtime = endpointRuntime.get(endpoint.id);
    if (runtime === undefined) continue;
    endpointStates.set(
      endpoint.id,
      endpointStateFor(endpoint, runtime, usage.get(endpoint.id) ?? []),
    );
  }
  const channelStates = new Map<string, { state: ChannelState; stopReason: StopReason | null }>();
  for (const channel of channels) {
    const runtime = channelRuntime.get(channel.id);
    if (runtime === undefined) continue;
    const states = channelEndpointIds(channel)
      .map((id) => endpointStates.get(id))
      .filter((state): state is EndpointState => state !== undefined);
    channelStates.set(channel.id, channelStateFor(channel, runtime, states, now));
  }
  return { channels: channelStates, endpoints: endpointStates, raw };
}

/** 0 to 1: queue depth against the alert threshold. */
export function queuePressure(runtime: ChannelRuntime, queueThreshold: number): number {
  return clamp(runtime.queueDepth / Math.max(queueThreshold, 1), 0, 1);
}
