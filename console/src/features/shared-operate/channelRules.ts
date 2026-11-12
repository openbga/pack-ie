import type { BufferStatus, ChannelAction, ChannelSnapshot } from "@/api/types";
import type { Tone } from "@/components/tones";
import { formatAge, formatBytes, formatCount } from "@/lib/format";
import { channelUrgency } from "@/lib/status";

/** Whether a channel is failing, recovering, or holding deliveries nobody has resolved. */
export function channelNeedsAttention(channel: ChannelSnapshot): boolean {
  return channelUrgency(channel) >= 2 || (channel.queue?.needsAttention ?? 0) > 0;
}

/** Failing first, then channels with unresolved deliveries, then the fullest, then by name. */
export function compareChannels(a: ChannelSnapshot, b: ChannelSnapshot): number {
  return (
    channelUrgency(b) - channelUrgency(a) ||
    Number((b.queue?.needsAttention ?? 0) > 0) - Number((a.queue?.needsAttention ?? 0) > 0) ||
    b.pressure - a.pressure ||
    a.label.localeCompare(b.label)
  );
}

/** `Analyzer ↔ LIS` for a bridge, `Analyzer → 2 destinations` for a message channel. */
export function endpointsText(channel: ChannelSnapshot): string {
  const [first, ...rest] = channel.endpoints;
  if (first === undefined) return "No endpoints";
  if (channel.kind === "bridge") {
    return rest.length > 0
      ? `${first.label} ↔ ${rest.map((ref) => ref.label).join(", ")}`
      : first.label;
  }
  return `${first.label} → ${rest.length} ${rest.length === 1 ? "destination" : "destinations"}`;
}

/** "2 min ago", or "No activity yet" for a channel that has not moved any data. */
export function activityText(at: string | null): string {
  return at === null ? "No activity yet" : formatAge(at);
}

/** How full a buffer is, 0 to 1: the larger of its byte and segment fill. */
export function bufferFill(buffer: BufferStatus): number {
  const bytes = buffer.capacityBytes > 0 ? buffer.usedBytes / buffer.capacityBytes : 0;
  const segments =
    buffer.maxSegments !== null && buffer.maxSegments > 0
      ? buffer.segments / buffer.maxSegments
      : 0;
  return Math.max(bytes, segments);
}

/** The buffer with the highest fill, for a bridge's summary line. */
export function fullestBuffer(buffers: readonly BufferStatus[]): BufferStatus | undefined {
  let fullest: BufferStatus | undefined;
  let best = -1;
  for (const buffer of buffers) {
    const fill = bufferFill(buffer);
    if (fill > best) {
      fullest = buffer;
      best = fill;
    }
  }
  return fullest;
}

/** Where the load stands, in words: buffer fill for a bridge, queue depth for a message channel. */
export function pressureSummary(channel: ChannelSnapshot): { text: string; tone: Tone } {
  const percent = Math.round(channel.pressure * 100);
  const tone: Tone = channel.pressure >= 0.8 ? "warning" : "neutral";
  if (channel.kind === "bridge") {
    const buffer = fullestBuffer(channel.buffers);
    const detail = buffer
      ? ` (${formatBytes(buffer.usedBytes)} of ${formatBytes(buffer.capacityBytes)})`
      : "";
    return { text: `${percent}% of buffer used${detail}`, tone };
  }
  const depth = channel.queue?.depth ?? 0;
  return { text: `${formatCount(depth)} queued · ${percent}% of alert limit`, tone };
}

/**
 * What an operator can do to a channel now: what the engine says (`allowedActions`), and for
 * an engine that does not say, the supervisor's rules: a running channel can pause or restart;
 * a paused one resumes; one stopped by an error restarts; a disabled one or one with an invalid
 * configuration is changed in the editor.
 */
export function allowedChannelActions(
  channel: Pick<ChannelSnapshot, "state" | "stopReason" | "allowedActions">,
): ChannelAction[] {
  if (channel.allowedActions !== null) return channel.allowedActions;
  if (channel.state !== "stopped") return ["pause", "restart"];
  if (channel.stopReason === "paused") return ["resume"];
  if (channel.stopReason === "fatal") return ["restart"];
  // Not implemented, replaced, shutting down, disabled, invalid: nothing to press here.
  return [];
}
