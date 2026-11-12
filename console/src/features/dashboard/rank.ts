import type { Alert, ChannelSnapshot, Snapshot } from "@/api/types";

/** A channel with what the dashboard ranks it by, worked out once per snapshot. */
export interface ChannelRow {
  channel: ChannelSnapshot;
  /** 1 (most urgent) to 8, from operator-console.md section 3. */
  rank: number;
  /** An open `critical` alert concerns this channel: the row shows danger emphasis. */
  critical: boolean;
  /** Open alerts of this channel. */
  alerts: readonly Alert[];
  /** Deliveries that need attention (rejected, failed, timed out, outcome unknown). */
  unresolved: number;
  /** When the newest error happened, from the snapshot; null for a channel without one. */
  lastErrorAt: number | null;
}

/** A recent error lifts a channel above quiet ones in its rank. */
const RECENT_ERROR_MS = 5 * 60_000;
/** Traffic this recent is "active" (rank 5), not idle (rank 6). */
const RECENT_TRAFFIC_MS = 60_000;

const time = (value: string | null | undefined): number | null => {
  if (value === null || value === undefined) return null;
  const parsed = Date.parse(value);
  return Number.isNaN(parsed) ? null : parsed;
};

/** The newest activity the snapshot reports for the channel, from either side or its source. */
function lastActivity(channel: ChannelSnapshot): number | null {
  const times = [
    channel.lastActivityAt,
    channel.bridge?.endpointA.lastActivityAt,
    channel.bridge?.endpointB.lastActivityAt,
    channel.source?.lastReceivedAt,
  ]
    .map(time)
    .filter((value) => value !== null);
  return times.length === 0 ? null : Math.max(...times);
}

function lastErrorTime(channel: ChannelSnapshot): number | null {
  const times = [channel.lastError?.at, channel.recentErrors[0]?.at]
    .map(time)
    .filter((value) => value !== null);
  return times.length === 0 ? null : Math.max(...times);
}

/**
 * The rank of one channel (1 is the most urgent). A stopped channel that failed
 * (`fatal`, `configuration`) is a failure, not an intentional stop, so it ranks with
 * degraded ones; paused and disabled channels rank last and never above an active one.
 */
function rankOf(channel: ChannelSnapshot, alerts: readonly Alert[], referenceTime: number): number {
  if (alerts.some((alert) => alert.severity === "critical")) return 1;
  switch (channel.state) {
    case "retrying":
      return 2;
    case "degraded":
      return 3;
    case "starting":
      return 7;
    case "stopped":
      // Whatever the reason: a stopped channel is not failing by itself (operator-console.md §2).
      // One the engine reports as a problem carries a critical alert, which ranks it first.
      return 8;
    case "running": {
      const attention =
        alerts.some((alert) => alert.severity === "warning") ||
        (channel.queue?.needsAttention ?? 0) > 0;
      if (attention) return 4;
      const active = lastActivity(channel);
      return active !== null && referenceTime - active <= RECENT_TRAFFIC_MS ? 5 : 6;
    }
  }
}

/** Rank, then (inside a rank) recent error first, the most recent error first, then A to Z. */
function compareRows(a: ChannelRow, b: ChannelRow, referenceTime: number): number {
  const recent = (row: ChannelRow) =>
    row.lastErrorAt !== null && referenceTime - row.lastErrorAt <= RECENT_ERROR_MS;
  return (
    a.rank - b.rank ||
    Number(recent(b)) - Number(recent(a)) ||
    (b.lastErrorAt ?? Number.NEGATIVE_INFINITY) - (a.lastErrorAt ?? Number.NEGATIVE_INFINITY) ||
    a.channel.label.localeCompare(b.channel.label)
  );
}

/**
 * Every channel of the snapshot in dashboard order. Times are measured against the
 * snapshot's own clock, so the order does not depend on when the page happens to render.
 */
export function rankChannels(snapshot: Snapshot): ChannelRow[] {
  const referenceTime = time(snapshot.takenAt) ?? Date.now();
  return snapshot.channels
    .map((channel): ChannelRow => {
      const alerts = snapshot.openAlerts.filter(
        (alert) => alert.state === "open" && alert.channelId === channel.id,
      );
      return {
        channel,
        rank: rankOf(channel, alerts, referenceTime),
        critical: alerts.some((alert) => alert.severity === "critical"),
        alerts,
        unresolved: channel.queue?.needsAttention ?? 0,
        lastErrorAt: lastErrorTime(channel),
      };
    })
    .sort((a, b) => compareRows(a, b, referenceTime));
}
