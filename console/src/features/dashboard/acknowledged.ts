import type { ChannelSnapshot, DestinationSummary } from "@/api/types";
import { formatAge, formatCount } from "@/lib/format";

/**
 * What a message channel's destinations acknowledged, as the dashboard words it. One
 * destination is stated plainly. With several, the console cannot prove that every message was
 * acknowledged by all of them, so each destination's own figure is shown with its label, in the
 * snapshot's order. Disabled destinations that never acknowledged anything are left out.
 */
function shownDestinations(channel: ChannelSnapshot): readonly DestinationSummary[] | null {
  const { destinations } = channel;
  if (destinations === undefined) return null;
  return destinations.filter((d) => d.enabled || d.acknowledged > 0);
}

/** `Acknowledged 2,136`, or `Acknowledged 2,136 (LIS) · 1,068 (backup)`; null when not reported. */
export function acknowledgedCounts(channel: ChannelSnapshot): string | null {
  const shown = shownDestinations(channel);
  if (shown === null) return null;
  if (shown.length === 0) return "Acknowledged 0";
  const [only] = shown;
  if (shown.length === 1 && only !== undefined) {
    return `Acknowledged ${formatCount(only.acknowledged)}`;
  }
  return `Acknowledged ${shown.map((d) => `${formatCount(d.acknowledged)} (${d.label})`).join(" · ")}`;
}

/** `Acknowledged 8 s ago`, or per destination `Acknowledged 8 s ago (LIS) · never (backup)`. */
export function acknowledgedTimes(channel: ChannelSnapshot, now: number): string | null {
  const shown = shownDestinations(channel);
  if (shown === null) return null;
  const age = (d: DestinationSummary) =>
    d.lastAcknowledgedAt === null ? "never" : formatAge(d.lastAcknowledgedAt, now);
  const [only] = shown;
  if (shown.length === 0) return "Acknowledged never";
  if (shown.length === 1 && only !== undefined) return `Acknowledged ${age(only)}`;
  return `Acknowledged ${shown.map((d) => `${age(d)} (${d.label})`).join(" · ")}`;
}
