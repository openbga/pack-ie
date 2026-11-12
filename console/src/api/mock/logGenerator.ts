import type { ConfigDocument, EndpointKind, LogEvent, LogLevel } from "../types";
import { endpointTarget } from "./endpointInfo";
import type { Rng } from "./prng";
import { iso } from "./util";

interface LogEndpoint {
  id: string;
  target: string;
  kind: EndpointKind;
}

export interface LogChannel {
  id: string;
  isBridge: boolean;
  endpoints: LogEndpoint[];
}

export type LogDraft = Omit<LogEvent, "seq">;

export function buildLogChannels(document: ConfigDocument): LogChannel[] {
  return document.channels.map((channel) => {
    const ids =
      channel.kind === "bridge"
        ? [channel.endpointAId, channel.endpointBId]
        : [channel.source.endpointId, ...channel.destinations.map((d) => d.endpointId)];
    const endpoints: LogEndpoint[] = [];
    for (const id of ids) {
      const endpoint = document.endpoints.find((e) => e.id === id);
      if (endpoint) endpoints.push({ id, target: endpointTarget(endpoint), kind: endpoint.kind });
    }
    return { id: channel.id, isBridge: channel.kind === "bridge", endpoints };
  });
}

interface LogContext {
  channel: LogChannel;
  endpoint: LogEndpoint | undefined;
  rng: Rng;
  messageId: string;
}

/** `event` is a name of diagnostics.md 7.3; routine low-level lines have none. */
type Made = {
  message: string;
  fields: Record<string, string>;
  event?: string;
  withMessage?: boolean;
};
type Template = (p: LogContext) => Made;

/** A template, optionally limited to endpoints of certain kinds or to bridge channels. */
interface Entry {
  kinds?: EndpointKind[];
  bridgeOnly?: boolean;
  make: Template;
}

const num = (rng: Rng, min: number, max: number): string => String(rng.int(min, max));
const target = (p: LogContext): string => p.endpoint?.target ?? "";
const CLIENTS: EndpointKind[] = ["tcp_client", "sftp"];
const FILES: EndpointKind[] = ["folder", "sftp"];

const TEMPLATES: Record<LogLevel, Entry[]> = {
  trace: [
    {
      make: (p) => ({
        message: `read ${num(p.rng, 1, 256)} bytes`,
        fields: { bytes: num(p.rng, 1, 256) },
      }),
    },
    {
      make: (p) => ({
        message: `wrote ${num(p.rng, 1, 64)} bytes`,
        fields: { bytes: num(p.rng, 1, 64) },
      }),
    },
    {
      kinds: ["serial", "tcp_listener"],
      make: () => ({ message: "frame checksum ok", fields: {} }),
    },
  ],
  debug: [
    {
      kinds: ["serial", "tcp_listener"],
      make: (p) => ({
        message: "frame received",
        fields: { frame: num(p.rng, 1, 7), bytes: num(p.rng, 20, 240) },
      }),
    },
    {
      kinds: CLIENTS,
      make: (p) => ({ message: "heartbeat ok", fields: { rtt_ms: num(p.rng, 1, 40) } }),
    },
    {
      kinds: FILES,
      make: (p) => ({
        message: `polled ${target(p)}: ${num(p.rng, 0, 3)} new files`,
        fields: { target: target(p) },
      }),
    },
    { make: () => ({ message: "ACK queued for sending", fields: {} }) },
    {
      make: (p) => ({
        message: "message stored",
        fields: { bytes: num(p.rng, 200, 1800), ack_code: "AA" },
        event: "message.committed",
        withMessage: true,
      }),
    },
  ],
  info: [
    {
      kinds: CLIENTS,
      make: (p) => ({
        message: `connected to ${target(p)}`,
        fields: { peer: target(p) },
        event: "endpoint.connected",
      }),
    },
    {
      kinds: ["serial"],
      make: (p) => ({
        message: `opened ${target(p)}`,
        fields: { port: target(p) },
        event: "endpoint.connected",
      }),
    },
    {
      kinds: ["tcp_listener"],
      make: (p) => ({
        message: `listening on ${target(p)}`,
        fields: { bind: target(p) },
        event: "endpoint.connected",
      }),
    },
    {
      make: () => ({
        message: "ACK received",
        fields: { ack_code: "AA", ack_level: "application" },
        event: "delivery.state_changed",
        withMessage: true,
      }),
    },
    {
      make: (p) => ({
        message: "delivery acknowledged",
        fields: { attempt_no: num(p.rng, 1, 2), delivery_state: "delivered" },
        event: "delivery.state_changed",
        withMessage: true,
      }),
    },
    {
      bridgeOnly: true,
      make: (p) => ({
        message: "session paired",
        fields: { side_a: target(p), pairing: "parallel" },
        event: "bridge.session_opened",
      }),
    },
    {
      kinds: FILES,
      make: (p) => ({
        message: "file picked up",
        fields: { name: `results-${num(p.rng, 100, 999)}.csv` },
      }),
    },
  ],
  warn: [
    {
      kinds: CLIENTS,
      make: (p) => ({
        message: `retry ${num(p.rng, 1, 5)}/5 in ${num(p.rng, 2, 16)}s: connection refused`,
        fields: { peer: target(p), retry_count: num(p.rng, 1, 5) },
        event: "endpoint.retry_scheduled",
      }),
    },
    {
      bridgeOnly: true,
      make: (p) => ({
        message: `buffer ${num(p.rng, 80, 95)}% full`,
        fields: { direction: "a_to_b" },
        event: "buffer.overflow_detected",
      }),
    },
    {
      make: () => ({
        message: "ACK timeout after 10000 ms",
        fields: { attempt_no: "1", delivery_state: "timed_out" },
        event: "delivery.state_changed",
        withMessage: true,
      }),
    },
    {
      kinds: ["serial", "tcp_listener"],
      make: () => ({
        message: "frame rejected: checksum mismatch (NAK sent)",
        fields: { error_code: "checksum_mismatch" },
        event: "message.invalid",
      }),
    },
  ],
  error: [
    {
      kinds: CLIENTS,
      make: (p) => ({
        message: `delivery failed: connection refused (${target(p)})`,
        fields: { attempt_no: num(p.rng, 1, 5), delivery_state: "failed" },
        event: "delivery.state_changed",
        withMessage: true,
      }),
    },
    {
      kinds: ["serial"],
      make: (p) => ({
        message: `serial port ${target(p)} not found`,
        fields: { error_code: "not_found" },
        event: "endpoint.state_changed",
      }),
    },
    {
      make: () => ({
        message: "AE received: application error",
        fields: { ack_code: "AE", ack_level: "application" },
        event: "delivery.state_changed",
        withMessage: true,
      }),
    },
    {
      kinds: ["sftp"],
      make: () => ({
        message: "SFTP authentication failed",
        fields: { user: "labxfer" },
        event: "endpoint.state_changed",
      }),
    },
    {
      kinds: ["folder"],
      make: (p) => ({
        message: `cannot read ${target(p)}: access denied`,
        fields: {},
        event: "endpoint.state_changed",
      }),
    },
  ],
};

function applicable(
  entries: Entry[],
  channel: LogChannel,
  endpoint: LogEndpoint | undefined,
): Entry[] {
  const fitting = entries.filter((entry) => {
    if (entry.bridgeOnly && !channel.isBridge) return false;
    return (
      entry.kinds === undefined || (endpoint !== undefined && entry.kinds.includes(endpoint.kind))
    );
  });
  return fitting.length > 0
    ? fitting
    : entries.filter((e) => e.kinds === undefined && !e.bridgeOnly);
}

/** The module that logs events of this family; routine lines come from the channel's own code. */
export function targetOf(event: string | undefined | null): string {
  const family = event?.split(".")[0];
  switch (family) {
    case "endpoint":
    case "listener":
      return "packie_transport::endpoint";
    case "bridge":
    case "buffer":
      return "packie_routing::bridge";
    case "message":
    case "delivery":
      return "packie_routing::message";
    case "config":
      return "packie_config::apply";
    case "alert":
      return "packie_diagnostics::alert";
    case "operator":
      return "packie::ipc";
    default:
      return "packie_routing::channel";
  }
}

const LEVEL_CUTS: [LogLevel, number][] = [
  ["trace", 0.08],
  ["debug", 0.35],
  ["info", 0.87],
  ["warn", 0.96],
  ["error", 1],
];

/** Produces plausible, body-free log events for the channels of the document. */
export class LogGenerator {
  constructor(
    private readonly rng: Rng,
    private channels: LogChannel[],
    private messageCount: number,
  ) {}

  setChannels(channels: LogChannel[]): void {
    this.channels = channels;
  }

  private level(): LogLevel {
    const r = this.rng.float();
    return (LEVEL_CUTS.find(([, cut]) => r < cut) ?? LEVEL_CUTS[2] ?? ["info", 1])[0];
  }

  next(atMs: number): LogDraft {
    const level = this.level();
    const channel = this.rng.pick(this.channels);
    if (channel === undefined) {
      return {
        at: iso(atMs),
        level,
        target: "packie::node",
        event: null,
        channelId: null,
        endpointId: null,
        messageId: null,
        message: "engine heartbeat",
        fields: {},
      };
    }
    const endpoint = channel.endpoints.length > 0 ? this.rng.pick(channel.endpoints) : undefined;
    const template = this.rng.pick(applicable(TEMPLATES[level], channel, endpoint)).make;
    const messageId = `msg-${this.rng.int(0, Math.max(this.messageCount - 1, 0))}`;
    const made = template({ channel, endpoint, rng: this.rng, messageId });
    return {
      at: iso(atMs),
      level,
      target: targetOf(made.event),
      event: made.event ?? null,
      channelId: channel.id,
      endpointId: endpoint?.id ?? null,
      messageId: made.withMessage ? messageId : null,
      message: made.message,
      fields: { channel_id: channel.id, ...made.fields },
    };
  }
}
