// How channel and endpoint states look in the Operate screens (docs/spec/operator-console.md
// section 2): a tone, a distinct shape and a text label for every state, so color is never
// the only signal. Routine states stay neutral; color marks what needs someone.
import {
  Check,
  Circle,
  CircleDashed,
  CircleDot,
  LoaderCircle,
  type LucideIcon,
  Minus,
  Pause,
  RotateCw,
  Square,
  TriangleAlert,
  X,
} from "lucide-react";
import type { ChannelSnapshot, EndpointState, SessionState, SourceSession } from "@/api/types";
import type { Tone } from "@/components/tones";

export interface Shape {
  tone: Tone;
  /** The state in words. */
  label: string;
  Icon: LucideIcon;
}

type ChannelStateFields = Pick<ChannelSnapshot, "state" | "stopReason">;

/** A filled circle is drawn by the chip; the other shapes are outline icons. */
export const FILLED_SHAPES: ReadonlySet<LucideIcon> = new Set([Circle]);

function stoppedShape(reason: ChannelSnapshot["stopReason"]): Shape {
  switch (reason) {
    case "paused":
      return { tone: "neutral", label: "Paused by operator", Icon: Pause };
    case "disabled":
      return { tone: "neutral", label: "Disabled", Icon: Minus };
    case "configuration":
      return { tone: "neutral", label: "Stopped: configuration invalid", Icon: Square };
    case "fatal":
      return { tone: "neutral", label: "Stopped: error", Icon: Square };
    case "replaced":
      return { tone: "neutral", label: "Stopped: replaced by configuration", Icon: Square };
    case "shutdown":
      return { tone: "neutral", label: "Stopped: engine shutting down", Icon: Square };
    case "store_unavailable":
      return { tone: "neutral", label: "Waiting for the message store", Icon: Square };
    case "not_implemented":
      return { tone: "neutral", label: "Stopped: not implemented", Icon: Square };
    case null:
      return { tone: "neutral", label: "Stopped", Icon: Square };
  }
}

export function channelShape(channel: ChannelStateFields): Shape {
  switch (channel.state) {
    case "running":
      return { tone: "neutral", label: "Running", Icon: Circle };
    case "starting":
      return { tone: "info", label: "Starting", Icon: LoaderCircle };
    case "retrying":
      return { tone: "warning", label: "Retrying", Icon: RotateCw };
    case "degraded":
      return { tone: "warning", label: "Degraded", Icon: TriangleAlert };
    case "stopped":
      return stoppedShape(channel.stopReason);
  }
}

export function endpointShape(state: EndpointState): Shape {
  switch (state) {
    case "connected":
      return { tone: "neutral", label: "Connected", Icon: Check };
    case "listening":
      return { tone: "neutral", label: "Listening", Icon: CircleDot };
    case "connecting":
      return { tone: "info", label: "Connecting", Icon: LoaderCircle };
    case "retrying":
      return { tone: "warning", label: "Retrying", Icon: RotateCw };
    case "disconnected":
      return { tone: "neutral", label: "Disconnected", Icon: CircleDashed };
    case "error":
      return { tone: "danger", label: "Error", Icon: X };
    case "disabled":
      return { tone: "neutral", label: "Disabled", Icon: Minus };
  }
}

const SESSION_LABELS: Record<SessionState, string> = {
  waiting_for_peer: "Waiting for peer",
  bridged: "Bridged",
  closing: "Closing",
  closed: "Closed",
};

export function sessionLabel(state: SessionState): string {
  return SESSION_LABELS[state];
}

const SOURCE_SESSION_LABELS: Record<SourceSession, string> = {
  idle: "Idle",
  receiving: "Receiving",
  waiting_for_peer: "Waiting for peer",
  closed: "Closed",
};

export function sourceSessionLabel(state: SourceSession): string {
  return SOURCE_SESSION_LABELS[state];
}
