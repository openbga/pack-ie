import type { ChannelSnapshot, DestinationStatus, EndpointSnapshot } from "@/api/types";
import { channelUrgency } from "@/lib/status";
import { Countdown } from "../shared-operate/Countdown";
import { type ErrorOrigin, resolveErrorOrigin } from "../shared-operate/errorOrigin";
import { LastErrorNote } from "../shared-operate/LastErrorNote";
import { RetryState } from "../shared-operate/RetryState";
import { sourceSessionLabel } from "../shared-operate/stateShapes";

/** How current a last error is, from what the channel is doing now. */
function errorTone(channel: ChannelSnapshot): "danger" | "warning" | "info" {
  const urgency = channelUrgency(channel);
  return urgency >= 3 ? "danger" : urgency === 2 ? "warning" : "info";
}

function deliveryRetry(destination: DestinationStatus, nextAttemptAt: string | null) {
  if (nextAttemptAt !== null) return <Countdown until={nextAttemptAt} />;
  if (!destination.enabled) return <>No attempt is scheduled: the destination is disabled</>;
  if (destination.queued + destination.sending === 0) {
    return <>No attempt is scheduled: no delivery waits for this destination</>;
  }
  // The engine says deliveries wait but gives no time: say so, not that nothing is scheduled.
  return <>The engine reports no time for the next attempt</>;
}

/** The context the error needs from where it came from: streak, retry and what is healthy. */
function contextOf(
  channel: ChannelSnapshot,
  destinations: readonly DestinationStatus[],
  origin: ErrorOrigin | null,
) {
  if (origin?.kind === "destination") {
    const { destination, streak, nextAttemptAt } = origin;
    return {
      failureStreak: streak,
      where:
        destination.label === destination.endpoint.label
          ? `Destination ${destination.label}`
          : `Destination ${destination.label}: ${destination.endpoint.label}`,
      retryLabel: "Delivery retry",
      retry: deliveryRetry(destination, nextAttemptAt),
      session:
        channel.source === null ? undefined : (
          <>
            {sourceSessionLabel(channel.source.session)} ({channel.source.endpoint.label})
          </>
        ),
    };
  }
  return {
    failureStreak: origin?.streak ?? null,
    where: origin?.kind === "source" ? `Source: ${origin.label}` : undefined,
    retry: <RetryState channel={channel} destinations={destinations} />,
  };
}

/**
 * The last error of a channel, with the context of the part that failed: a destination's own
 * failure streak and delivery retry, apart from the health of the source session
 * (operator-console.md section 4, "Error presentation").
 */
export function ChannelError({
  channel,
  destinations,
  endpoints,
}: {
  channel: ChannelSnapshot;
  destinations: readonly DestinationStatus[];
  endpoints: readonly EndpointSnapshot[];
}) {
  if (channel.lastError === null) return null;
  const origin = resolveErrorOrigin(channel, channel.lastError, destinations, endpoints);
  return (
    <LastErrorNote
      error={channel.lastError}
      tone={errorTone(channel)}
      context={contextOf(channel, destinations, origin)}
    />
  );
}
