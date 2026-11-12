import type { ChannelSnapshot, DestinationStatus } from "@/api/types";
import { Countdown } from "./Countdown";
import { isRetrying } from "./errorOrigin";

/**
 * What the channel is doing about a failure, in words: when the next attempt is, that it is
 * waiting for a peer, or that no retry is scheduled (and why). Visible without opening
 * anything else (operator-console.md section 4, "Error presentation"). A message channel that
 * runs while a destination retries its deliveries says so (pass its `destinations`): the
 * source session is healthy and the retry belongs to the destination.
 */
export function RetryState({
  channel,
  destinations = [],
}: {
  channel: ChannelSnapshot;
  destinations?: readonly DestinationStatus[];
}) {
  const waiting = channel.bridge?.sessionState === "waiting_for_peer";
  switch (channel.state) {
    case "retrying":
      if (channel.nextRetryAt === null) return <>Attempting to reconnect now</>;
      return (
        <>
          Next attempt <Countdown until={channel.nextRetryAt} />
          {channel.counters.retries > 0 && ` (retry ${channel.counters.retries})`}
        </>
      );
    case "stopped":
      return channel.stopReason === "paused" ? (
        <>No retry is scheduled: paused by operator</>
      ) : (
        <>No retry is scheduled</>
      );
    case "starting":
      return waiting ? <>Waiting for peer</> : <>Starting</>;
    case "running":
    case "degraded":
      if (waiting) return <>Waiting for peer</>;
      return (
        <ConnectedRetry destinations={destinations} isDegraded={channel.state === "degraded"} />
      );
  }
}

/** A channel whose own connections are up: only a destination can still be retrying. */
function ConnectedRetry({
  destinations,
  isDegraded,
}: {
  destinations: readonly DestinationStatus[];
  isDegraded: boolean;
}) {
  const retrying = destinations
    .filter(isRetrying)
    .sort((a, b) => Date.parse(a.nextAttemptAt ?? "") - Date.parse(b.nextAttemptAt ?? ""));
  const [next] = retrying;
  if (next?.nextAttemptAt == null) {
    return isDegraded ? <>The engine reports no retry time</> : <>Not retrying</>;
  }
  const names = retrying.map((destination) => destination.label).join(", ");
  return (
    <>
      Source session is healthy. Delivery to {names} retries{" "}
      <Countdown until={next.nextAttemptAt} />
    </>
  );
}
