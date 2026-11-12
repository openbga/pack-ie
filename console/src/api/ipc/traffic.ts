import type { TapSegment } from "@/ipc/generated";
import type { Unsubscribe } from "../engine";
import type { EngineError, TrafficSegment } from "../types";
import type { IpcClient } from "./client";
import { bytesToBase64 } from "./util";
import type { WireChannel } from "./wire";

type Flow = TrafficSegment["direction"];

/** Message channels: the source side (received, response) and the destination side (sent, reply). */
const messagePane = (role: TapSegment["role"]): Flow =>
  role === "received" || role === "response" ? "a_to_b" : "b_to_a";

/**
 * The pane a segment belongs to, in the console's terms. Bridges: A to B is `a_to_b`, so
 * bytes that came in from A, or go out to B, travel that way. Message channels: by role,
 * what the source sent (`received`) and the engine's answers to it (`response`) are `a_to_b`,
 * the Source pane; what the engine sent to a destination (`sent`) and what it answered
 * (`reply`) are `b_to_a`, the Destinations pane. A channel whose document is not known yet
 * falls back to the segment's direction: inbound is `a_to_b`.
 */
export function trafficDirection(segment: TapSegment, channel: WireChannel | undefined): Flow {
  if (channel?.kind === "message") return messagePane(segment.role);
  if (channel?.kind !== "bridge") return segment.direction === "inbound" ? "a_to_b" : "b_to_a";
  const inbound = segment.direction === "inbound";
  const fromA = segment.endpoint_id === channel.endpoint_a;
  return inbound === fromA ? "a_to_b" : "b_to_a";
}

/**
 * Subscribes to a channel's tap. `channel` resolves the channel's document (for bridge
 * sides); segments wait for it, in order. An engine that refuses the transcript (another
 * client already views it) is reported to `onError` with the engine's own text.
 */
export function subscribeTraffic(
  client: IpcClient,
  channelId: string,
  channel: () => Promise<WireChannel | undefined>,
  onSegments: (segments: TrafficSegment[]) => void,
  onError?: (error: EngineError) => void,
): Unsubscribe {
  let seq = 0;
  let live = true;
  let pending: TapSegment[] = [];
  let ready: Promise<WireChannel | undefined> | null = null;

  const flush = async () => {
    const taken = pending;
    pending = [];
    ready ??= channel().catch(() => undefined);
    const config = await ready;
    if (!live || taken.length === 0) return;
    onSegments(
      taken.map((segment) => ({
        seq: ++seq,
        at: segment.at,
        direction: trafficDirection(segment, config),
        base64: bytesToBase64(segment.bytes),
        flow: segment.direction,
        endpointId: segment.endpoint_id,
        role: segment.role,
        destinationId: segment.destination_id,
      })),
    );
  };

  const stop = client.subscribe(
    { type: "traffic", channel_id: channelId },
    (event) => {
      if (event.type !== "traffic") return;
      pending.push(event.segment);
      if (pending.length === 1) queueMicrotask(() => void flush());
    },
    (error) => {
      if (live) onError?.(error);
    },
  );
  return () => {
    live = false;
    stop();
  };
}
