import type { ChannelSnapshot } from "@/api/types";
import { Countdown } from "./Countdown";
import { ShapeChip } from "./ShapeChip";
import { channelShape } from "./stateShapes";

/**
 * A channel's state with its stop reason in words; while it retries, the live countdown to
 * the next attempt ("Retrying in 5 s"). A paused or disabled channel is neutral, never an error.
 */
export function ChannelStateChip({
  channel,
}: {
  channel: Pick<ChannelSnapshot, "state" | "stopReason" | "nextRetryAt">;
}) {
  const shape = channelShape(channel);
  if (channel.state === "retrying" && channel.nextRetryAt !== null) {
    return (
      <ShapeChip shape={shape}>
        {shape.label} <Countdown until={channel.nextRetryAt} />
      </ShapeChip>
    );
  }
  return <ShapeChip shape={shape} />;
}
