// Log events and the live traffic tap.
import type { LogLevel } from "./core";

export interface LogEvent {
  /** Monotonic sequence within this engine run; the cursor for paging and following. */
  seq: number;
  at: string;
  level: LogLevel;
  /** Module that logged it, such as `packie_routing::bridge`. */
  target: string;
  /** Catalog event name (diagnostics.md 7.3), such as `channel.started`; null when the engine set none. */
  event: string | null;
  channelId: string | null;
  endpointId: string | null;
  messageId: string | null;
  message: string;
  /** Extra structured fields, never message bodies. */
  fields: Record<string, string>;
}

export interface LogQuery {
  /** This level and anything more severe. */
  minLevel?: LogLevel;
  from?: string;
  to?: string;
  channelId?: string;
  endpointId?: string;
  /** The catalog event name; the engine matches the whole name, not a prefix. */
  event?: string;
  text?: string;
}

/**
 * What `followLogs` reports besides events: the connection to the engine was lost and came back.
 * The events published meanwhile were read from the engine's log and arrive with it, oldest first.
 */
export interface LogFollowNotice {
  /** RFC 3339: when the connection was lost, and when it came back. */
  lostAt: string;
  backAt: string;
  /** How many events were filled in, delivered with this notice. */
  filled: number;
  /**
   * `none`: nothing missed. `gap`: some events of the outage are no longer in the engine's log.
   * `restarted`: the engine restarted, so its earlier events (and their `seq`) are gone; `seq`
   * counts from 1 again.
   */
  missing: "none" | "gap" | "restarted";
}

export interface LogPage {
  /** Newest first. */
  events: LogEvent[];
  /** Pass as `before` to get the next older page; null when there is none. */
  olderCursor: number | null;
}

export interface TrafficSegment {
  seq: number;
  at: string;
  /**
   * The pane. Bridges: A→B or B→A. Message channels: `a_to_b` is the Source pane (received
   * from the source and the engine's responses to it), `b_to_a` the Destinations pane (sent to
   * a destination and what it answered).
   */
  direction: "a_to_b" | "b_to_a";
  base64: string;
  /** Relative to the engine, when the engine says; message channels mark it on each segment. */
  flow?: "inbound" | "outbound";
  /**
   * What the segment is, for message channels: `received` from the source, `response` the
   * engine's answer to the source, `sent` to a destination, `reply` the destination's answer.
   */
  role?: "received" | "response" | "sent" | "reply";
  /** The endpoint the bytes came from or went to. */
  endpointId?: string;
  /** Destinations pane: the destination the segment belongs to; null while the engine does not tag it. */
  destinationId?: string | null;
}
