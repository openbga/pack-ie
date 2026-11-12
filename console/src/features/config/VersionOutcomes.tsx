import type { ChannelOutcome } from "@/api/types";
import { StatusBadge } from "@/components/StatusBadge";
import styles from "@/styles/layout.module.css";
import { capitalized, OUTCOME_TONES, sortedOutcomes } from "./outcomes";

interface VersionOutcomesProps {
  outcomes: readonly ChannelOutcome[];
  /** The version the engine runs now: its failures still wait for a fix; an older one's are history. */
  isActive: boolean;
  /** A channel's label for its id. */
  nameOf: (channelId: string) => string;
}

/**
 * What applying a version did to each channel: every channel it changed with its outcome in
 * words and tone, a failure with the engine's reason, then how many were left alone. A version
 * saved before outcomes were kept has none to show.
 */
export function VersionOutcomes({ outcomes, isActive, nameOf }: VersionOutcomesProps) {
  if (outcomes.length === 0) {
    return (
      <p>
        No per-channel outcomes are recorded for this version: it was saved before outcomes were
        kept, or it is the startup import.
      </p>
    );
  }
  const changed = sortedOutcomes(outcomes).filter((each) => each.outcome !== "unchanged");
  const unchanged = outcomes.length - changed.length;
  const failed = changed.some((each) => each.outcome === "failed");
  return (
    <div className={styles.stack}>
      {changed.length > 0 && (
        <ul className={styles.list} aria-label="Channel outcomes">
          {changed.map((each) => (
            <li key={each.channelId}>
              <StatusBadge tone={OUTCOME_TONES[each.outcome]}>
                {capitalized(each.outcome)}
              </StatusBadge>{" "}
              {nameOf(each.channelId)} <span className={styles.code}>{each.channelId}</span>
              {each.reason && <span>: {each.reason}</span>}
            </li>
          ))}
        </ul>
      )}
      {failed && (
        <p className={styles.hint}>
          {isActive
            ? "The version is active. A channel that failed kept its previous definition; the next apply retries it."
            : "When applied, this version became active, and a channel that failed kept its previous definition. Later versions have replaced it since."}
        </p>
      )}
      <p className={styles.hint}>
        {unchanged === 0
          ? "Every channel was changed."
          : `${unchanged} ${unchanged === 1 ? "channel was" : "channels were"} unchanged.`}
      </p>
    </div>
  );
}
