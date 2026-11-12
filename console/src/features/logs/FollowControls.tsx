import { Button } from "@/components/Button";
import { ToggleChip } from "@/components/ToggleChip";
import { formatCount } from "@/lib/format";
import styles from "./FollowControls.module.css";
import { MAX_EVENTS } from "./useLogStream";

interface FollowControlsProps {
  /** Following is on: new events are being received. */
  following: boolean;
  /** Following cannot be on: the time range ends in the past. */
  unavailable: boolean;
  onFollowChange: (following: boolean) => void;
  /** The connection to the engine is down: following resumes by itself when it returns. */
  waiting: boolean;
  /** Following is held, so the rows do not move. */
  paused: boolean;
  /** Events received while held. */
  held: number;
  onPause: () => void;
  onResume: () => void;
}

/**
 * Follow, and Pause or Jump to live while following. The state is said in words, in a polite
 * live region: "Following", "Waiting for the engine…" while the connection is down, or
 * "Paused — 37 new events" once the list is held, by the operator, by scrolling away from the
 * newest event or by opening one. Jump to live shows the newest events and follows again.
 */
export function FollowControls({
  following,
  unavailable,
  onFollowChange,
  waiting,
  paused,
  held,
  onPause,
  onResume,
}: FollowControlsProps) {
  return (
    <div className={styles.follow}>
      <ToggleChip isSelected={following} isDisabled={unavailable} onChange={onFollowChange}>
        Follow
      </ToggleChip>
      {following && (
        <Button onPress={paused ? onResume : onPause}>{paused ? "Jump to live" : "Pause"}</Button>
      )}
      <span role="status" className={paused ? styles.held : styles.state}>
        {statusText({ following, unavailable, waiting, paused, held })}
      </span>
    </div>
  );
}

function statusText({
  following,
  unavailable,
  waiting,
  paused,
  held,
}: Pick<FollowControlsProps, "following" | "unavailable" | "waiting" | "paused" | "held">): string {
  if (unavailable) return "Not following: the range ends in the past";
  if (!following) return "Not following";
  if (waiting) return "Waiting for the engine… following resumes automatically";
  if (paused) return `Paused — ${formatCount(held)} new ${held === 1 ? "event" : "events"}`;
  return `Following · newest ${formatCount(MAX_EVENTS)} rows kept`;
}
