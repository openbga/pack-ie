import { useEngineQuery, useSnapshot } from "@/api/hooks";
import { formatCount } from "@/lib/format";

/** Deliveries that removing a channel or destination closes, as far as the engine reports them. */
export interface ClosedCounts {
  /** Rejected, failed, timed out or outcome unknown now; null when not known. */
  attention: number | null;
  /** Queued now; null when not known. */
  queued: number | null;
}

const UNKNOWN: ClosedCounts = { attention: null, queued: null };

/** What removing channel `channelId` closes: the snapshot's `queue.needsAttention`. */
export function useChannelClosed(channelId: string): ClosedCounts {
  const snapshot = useSnapshot();
  const queue = snapshot.data?.channels.find((each) => each.id === channelId)?.queue;
  return queue ? { attention: queue.needsAttention, queued: null } : UNKNOWN;
}

/**
 * What removing a destination of channel `channelId` closes, from the channel detail's
 * per-destination counts; unknown while loading or for a destination the engine does not run.
 */
export function useDestinationClosed(channelId: string): (connectorId: string) => ClosedCounts {
  const detail = useEngineQuery(["getChannel", channelId], (api) => api.getChannel(channelId));
  return (connectorId) => {
    const status = detail.data?.destinations.find((each) => each.destinationId === connectorId);
    if (status === undefined) return UNKNOWN;
    const attention = Object.values(status.unresolved).reduce((sum, count) => sum + count, 0);
    return { attention, queued: status.queued };
  };
}

function countText({ attention, queued }: ClosedCounts): string | null {
  const parts = [
    attention !== null && `${formatCount(attention)} need attention`,
    queued !== null && `${formatCount(queued)} queued`,
  ].filter((part): part is string => part !== false);
  return parts.length > 0 ? ` Right now ${parts.join(" and ")}.` : null;
}

/**
 * The consequence of removing a channel or destination for its deliveries (docs/spec
 * operator-console section 10, Deleting): the ones not yet resolved are closed as cancelled.
 */
export function DeliveriesClosed({
  noun,
  counts,
}: {
  noun: "channel" | "destination";
  counts: ClosedCounts;
}) {
  return (
    <p>
      Unresolved deliveries of this {noun} are closed when it is removed: queued, rejected, failed,
      timed out and outcome unknown ones become cancelled and are not delivered or retried.
      {countText(counts)} Acknowledged history stays
      {noun === "channel" ? "; stored messages remain readable until retention purges them" : ""}.
    </p>
  );
}
