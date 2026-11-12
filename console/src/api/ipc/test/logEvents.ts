import type { LogEvent as RawLogEvent } from "@/ipc/generated";

export const T0 = Date.parse("2026-11-06T12:00:00Z");

export function event(seq: number, extra: Partial<RawLogEvent> = {}): RawLogEvent {
  return {
    seq,
    at: new Date(T0 + seq * 1000).toISOString(),
    level: "info",
    target: "packie::test",
    event: null,
    message: `event ${seq}`,
    channel_id: null,
    endpoint_id: null,
    message_id: null,
    fields: {},
    ...extra,
  };
}

/** A ring of events 1..n, one per second, served the way the engine does: the newest `limit`. */
