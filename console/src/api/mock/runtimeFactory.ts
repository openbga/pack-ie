import type { ChannelConfig, ChannelState, Counters, LogLevel, TapFramer } from "../types";
import { metaFor } from "./channelMeta";
import type { Rng } from "./prng";
import type { ChannelRuntime, EndpointRuntime, SourceCounters } from "./runtimeState";

export interface RuntimeEvent {
  level: LogLevel;
  channelId: string | null;
  endpointId: string | null;
  messageId: null;
  message: string;
  fields: Record<string, string>;
}

export type CommandKind = "pause" | "resume" | "restart";

export type CommandOutcome =
  | { ok: true; events: RuntimeEvent[] }
  | { ok: false; code: "not_found" | "conflict"; message: string };

export const RESTART_MS = 1500;
export function zeroCounters(): Counters {
  return { bytesIn: 0, bytesOut: 0, messagesIn: 0, messagesOut: 0, retries: 0 };
}

/** Bytes of acknowledgement traffic per forwarded message, by what the bridge carries. */
const ACK_BYTES: Record<TapFramer, number> = { none: 6, astm: 14, au: 6, hl7_mllp: 70 };

export function ackBytes(framer: TapFramer): number {
  return ACK_BYTES[framer];
}

/** Messages going B to A per message going A to B (acknowledgement messages), in percent. */
export function replyPercent(framer: TapFramer): number {
  return framer === "hl7_mllp" ? 100 : 20;
}

function historyCounters(config: ChannelConfig, rng: Rng, messages: number): Counters {
  const counters = zeroCounters();
  counters.retries = rng.int(0, 30);
  counters.bytesIn = messages * rng.int(300, 900);
  if (config.kind === "message") {
    counters.messagesIn = messages;
    counters.messagesOut = messages * config.destinations.length;
    counters.bytesOut = Math.floor(counters.bytesIn * 0.9);
    return counters;
  }
  if (config.tapFramer !== "none") {
    counters.messagesIn = messages;
    counters.messagesOut = Math.floor((messages * replyPercent(config.tapFramer)) / 100);
  }
  counters.bytesOut = messages * ACK_BYTES[config.tapFramer];
  return counters;
}

function historySource(config: ChannelConfig, rng: Rng, messages: number): SourceCounters {
  const none = { duplicates: 0, invalid: 0, filtered: 0, aborted: 0 };
  if (config.kind !== "message") return none;
  return {
    duplicates: config.source.duplicateDetection ? Math.floor(messages * 0.01 * rng.float()) : 0,
    invalid: Math.floor(messages * 0.003 * rng.float()),
    filtered: config.filters.length > 0 ? Math.floor(messages * 0.04 * rng.float()) : 0,
    aborted: Math.floor(messages * 0.001 * rng.float()),
  };
}

/** A new channel record. With history it has run for a while (about 50 minutes of traffic). */
export function newChannelRuntime(
  config: ChannelConfig,
  rng: Rng,
  withHistory: boolean,
  now: number,
  configVersion: number,
): ChannelRuntime {
  const meta = metaFor(config.id);
  const ageSec = 3000 * (0.6 + rng.float());
  const messages = withHistory ? Math.round(meta.rate * ageSec) : 0;
  const base = config.kind === "message" ? rng.int(0, 6) : 0;
  return {
    stop: null,
    stopError: null,
    startingUntil: 0,
    counters: withHistory ? historyCounters(config, rng, messages) : zeroCounters(),
    lastActivityMs: null,
    lastError: null,
    errors: [],
    instanceStartMs: withHistory ? now - Math.round(ageSec * 1000) : now,
    configVersion,
    retainedAcked: 0,
    bufferA: config.kind === "bridge" ? 0.05 + rng.float() * 0.2 : 0,
    bufferB: config.kind === "bridge" ? 0.02 + rng.float() * 0.1 : 0,
    overflow: null,
    overflowCount: 0,
    framingErrors: withHistory && config.kind === "bridge" ? rng.int(0, 3) : 0,
    tapDropped: 0,
    source: historySource(config, rng, messages),
    queueDepth: base,
    queueBase: base,
    needsAttention: 0,
    transcriptViewers: 0,
  };
}

/** A new instance of a channel: counters, buffers and the overflow episode start over. */
export function resetInstance(runtime: ChannelRuntime, now: number, configVersion: number): void {
  runtime.retainedAcked += runtime.counters.messagesOut;
  runtime.counters = zeroCounters();
  runtime.instanceStartMs = now;
  runtime.configVersion = configVersion;
  runtime.overflow = null;
  runtime.overflowCount = 0;
  runtime.framingErrors = 0;
  runtime.tapDropped = 0;
  runtime.source = { duplicates: 0, invalid: 0, filtered: 0, aborted: 0 };
  if (runtime.bufferA > 0) runtime.bufferA = 0.02;
  if (runtime.bufferB > 0) runtime.bufferB = 0.02;
}

export function newEndpointRuntime(now: number, rng: Rng): EndpointRuntime {
  return {
    counters: zeroCounters(),
    lastActivityMs: null,
    lastState: "connected",
    stateSince: now - rng.int(2, 70) * 3_600_000,
    lastError: null,
    fault: null,
    peerConnected: rng.chance(0.6),
    failureStreak: 0,
    nextRetryMs: null,
    lastAttemptMs: null,
  };
}

/** Wait before the next retry after `streak` failures: a few seconds, doubling up to 20 s. */
export function retryDelayMs(streak: number, rng: Rng): number {
  return Math.min(20_000, 2000 * 2 ** Math.min(Math.max(streak - 1, 0), 4)) + rng.int(0, 500);
}

export function addCounters(counters: Counters, delta: Partial<Counters>): void {
  counters.bytesIn += delta.bytesIn ?? 0;
  counters.bytesOut += delta.bytesOut ?? 0;
  counters.messagesIn += delta.messagesIn ?? 0;
  counters.messagesOut += delta.messagesOut ?? 0;
  counters.retries += delta.retries ?? 0;
}

export function event(
  level: LogLevel,
  channelId: string | null,
  endpointId: string | null,
  message: string,
  fields: Record<string, string>,
): RuntimeEvent {
  return { level, channelId, endpointId, messageId: null, message, fields };
}

export const NOT_FOUND = "No longer exists";
const ALREADY_STOPPED = "Channel is already stopped";
const NOT_PAUSED = "Not paused";
const DISABLED = "Disabled in configuration: enable it in the configuration editor";
const USE_RESUME = "Channel is paused: use Resume";
const CONFIG_STOPPED =
  "The channel stopped on a configuration error; fix and apply the configuration first.";

/** The engine's wording for a command that cannot run now (operator-console.md section 5). */
export function refuse(
  kind: CommandKind,
  config: ChannelConfig,
  state: ChannelState,
  stopReason: string | null,
): string | null {
  if (kind === "pause") return state === "stopped" ? ALREADY_STOPPED : null;
  if (!config.enabled) return DISABLED;
  if (kind === "resume") return stopReason === "paused" ? null : NOT_PAUSED;
  if (stopReason === "paused") return USE_RESUME;
  return stopReason === "configuration" ? CONFIG_STOPPED : null;
}
