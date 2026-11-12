// How engine states are worded and colored. The one place: pages never decide a tone
// themselves, so a failing channel looks the same on the dashboard, in lists and in details.
// Color marks what needs attention (routine states are neutral); every state has text too.
import type {
  AlertSeverity,
  ChannelKind,
  ChannelSnapshot,
  DeliveryState,
  EndpointKind,
  EndpointState,
  LogLevel,
  Protocol,
} from "@/api/types";
import type { Tone } from "@/components/tones";

export interface Status {
  tone: Tone;
  label: string;
}

/** How loud a status is, so lists can put failures first: higher is more urgent. */
export type Urgency = 0 | 1 | 2 | 3;

/**
 * 3 degraded, 2 retrying, 1 starting or stopped, 0 running. A stopped channel is intentional or
 * built-in (operator-console.md §2): the engine marks one that is a problem with a critical alert.
 */
export function channelUrgency(channel: Pick<ChannelSnapshot, "state" | "stopReason">): Urgency {
  if (channel.state === "degraded") return 3;
  if (channel.state === "stopped") return 1;
  if (channel.state === "retrying") return 2;
  if (channel.state === "starting") return 1;
  return 0;
}

export function endpointStatus(state: EndpointState): Status {
  switch (state) {
    case "connected":
      return { tone: "neutral", label: "Connected" };
    case "listening":
      return { tone: "neutral", label: "Listening" };
    case "connecting":
      return { tone: "info", label: "Connecting" };
    case "retrying":
      return { tone: "warning", label: "Retrying" };
    case "disconnected":
      return { tone: "neutral", label: "Disconnected" };
    case "error":
      return { tone: "danger", label: "Error" };
    case "disabled":
      return { tone: "neutral", label: "Disabled" };
  }
}

export function endpointUrgency(state: EndpointState): Urgency {
  if (state === "error") return 3;
  if (state === "retrying" || state === "disconnected") return 2;
  if (state === "connecting") return 1;
  return 0;
}

export function deliveryStatus(state: DeliveryState): Status {
  switch (state) {
    case "queued":
      return { tone: "info", label: "Queued" };
    case "sending":
      return { tone: "info", label: "Sending" };
    case "acknowledged":
      return { tone: "neutral", label: "Acknowledged" };
    case "rejected":
      return { tone: "danger", label: "Rejected" };
    case "failed":
      return { tone: "danger", label: "Failed" };
    case "timed_out":
      return { tone: "warning", label: "Timed out" };
    case "outcome_unknown":
      return { tone: "warning", label: "Outcome unknown" };
    case "cancelled":
      return { tone: "neutral", label: "Cancelled" };
  }
}

export function alertTone(severity: AlertSeverity): Tone {
  return severity === "critical" ? "danger" : severity === "warning" ? "warning" : "info";
}

export function alertSeverityLabel(severity: AlertSeverity): string {
  return severity === "critical" ? "Critical" : severity === "warning" ? "Warning" : "Info";
}

export function logLevelTone(level: LogLevel): Tone {
  return level === "error" ? "danger" : level === "warn" ? "warning" : "neutral";
}

export const LOG_LEVELS: readonly LogLevel[] = ["error", "warn", "info", "debug", "trace"];

const PROTOCOL_LABELS: Record<Protocol, string> = {
  astm: "ASTM",
  au: "AU",
  hl7_mllp: "HL7 v2",
  delimited_file: "Delimited files",
  raw: "Raw",
};

export function protocolLabel(protocol: Protocol): string {
  return PROTOCOL_LABELS[protocol];
}

export function channelKindLabel(kind: ChannelKind): string {
  return kind === "bridge" ? "Bridge" : "Message";
}

const ENDPOINT_KIND_LABELS: Record<EndpointKind, string> = {
  serial: "Serial",
  tcp_client: "TCP client",
  tcp_listener: "TCP listener",
  folder: "Folder",
  sftp: "SFTP",
};

export function endpointKindLabel(kind: EndpointKind): string {
  return ENDPOINT_KIND_LABELS[kind];
}

export const ENDPOINT_KINDS = Object.keys(ENDPOINT_KIND_LABELS) as EndpointKind[];
