import type { BridgeChannelConfig, ChannelConfig, Counters, MessageChannelConfig } from "../types";
import type { Rng } from "./prng";
import { ackBytes, addCounters, replyPercent } from "./runtimeFactory";
import type { ChannelRuntime, EndpointRuntime, TrafficDelta } from "./runtimeState";

/** What `count` messages of `bytes` bytes add to a bridge, replies included. */
export function bridgeDelta(
  channel: BridgeChannelConfig,
  runtime: ChannelRuntime,
  count: number,
  bytes: number,
  rng: Rng,
): TrafficDelta {
  if (channel.tapFramer !== "none") {
    if (rng.chance(0.02)) runtime.framingErrors += 1;
    if (rng.chance(0.005)) runtime.tapDropped += rng.int(1, 3);
  }
  return {
    messagesIn: count,
    bytesIn: bytes,
    messagesOut: Math.floor((count * replyPercent(channel.tapFramer)) / 100),
    bytesOut: count * ackBytes(channel.tapFramer),
  };
}

/** Sorts newly received messages into duplicates, invalid, filtered and aborted ones. */
export function settleReceived(
  channel: MessageChannelConfig,
  runtime: ChannelRuntime,
  count: number,
  rng: Rng,
): void {
  for (let i = 0; i < count; i++) {
    const roll = rng.float();
    if (channel.source.duplicateDetection && roll < 0.01) runtime.source.duplicates += 1;
    else if (roll < 0.013) runtime.source.invalid += 1;
    else if (channel.filters.length > 0 && roll < 0.05) runtime.source.filtered += 1;
    else if (roll > 0.999) runtime.source.aborted += 1;
  }
}

/** Spreads a channel's traffic over its endpoints (a bridge's A and B, or source and destinations). */
export function creditEndpoints(
  endpoints: Map<string, EndpointRuntime>,
  channel: ChannelConfig,
  delta: Counters,
  now: number,
  live: boolean,
): void {
  const credit = (id: string, change: Partial<Counters>): void => {
    const runtime = endpoints.get(id);
    if (runtime === undefined) return;
    addCounters(runtime.counters, change);
    if (live && (delta.bytesIn > 0 || delta.bytesOut > 0)) runtime.lastActivityMs = now;
  };
  if (channel.kind === "bridge") {
    const forward = {
      bytesIn: delta.bytesIn,
      messagesIn: delta.messagesIn,
      bytesOut: delta.bytesOut,
      messagesOut: delta.messagesOut,
    };
    credit(channel.endpointAId, forward);
    credit(channel.endpointBId, {
      bytesIn: forward.bytesOut,
      messagesIn: forward.messagesOut,
      bytesOut: forward.bytesIn,
      messagesOut: forward.messagesIn,
    });
    return;
  }
  credit(channel.source.endpointId, { bytesIn: delta.bytesIn, messagesIn: delta.messagesIn });
  const share = channel.destinations.length;
  for (const destination of channel.destinations) {
    credit(destination.endpointId, {
      bytesOut: Math.floor(delta.bytesOut / share),
      messagesOut: Math.floor(delta.messagesOut / share),
    });
  }
}
