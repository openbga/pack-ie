import { Link } from "react-aria-components";
import type { ResendMode, ResendOutcome, ResendResult } from "@/api/types";
import { StatusBadge } from "@/components/StatusBadge";
import { rejectionTitle } from "./messageWording";
import styles from "./ResendResult.module.css";

const plural = (count: number, one: string, many: string) => (count === 1 ? one : many);

/** The toast that words what a resend did, however many destinations took it. */
export function outcomeToast(result: ResendResult) {
  const accepted = result.destinations.filter((d) => d.rejected === null).length;
  const total = result.destinations.length;
  if (accepted === total && total > 0) {
    return {
      tone: "success" as const,
      title: `Resend accepted for ${accepted} ${plural(accepted, "destination", "destinations")}`,
    };
  }
  if (accepted === 0) {
    return {
      tone: "warning" as const,
      title: "Nothing was resent",
      description: "The engine refused every destination. The dialog says why.",
    };
  }
  return {
    tone: "warning" as const,
    title: `Resend accepted for ${accepted} of ${total} destinations`,
    description: "The engine refused the others. The dialog says why.",
  };
}

function outcomeText(outcome: ResendOutcome, mode: ResendMode): string {
  if (outcome.rejected !== null) return outcome.rejected.message;
  return mode === "same_message"
    ? "Queued again. A new attempt cycle starts with the stored output."
    : "Accepted. The new message is processed with the current configuration.";
}

/** What the engine did for every destination asked for, in its own words for a refusal. */
export function ResendResultView({ mode, result }: { mode: ResendMode; result: ResendResult }) {
  const accepted = result.destinations.filter((d) => d.rejected === null).length;
  return (
    <>
      <p role="status">
        {accepted} of {result.destinations.length}{" "}
        {plural(result.destinations.length, "destination", "destinations")} accepted.
      </p>
      <ul className={styles.results} aria-label="Result per destination">
        {result.destinations.map((outcome) => (
          <li key={outcome.destinationId} className={styles.result}>
            <span className={styles.destination}>{outcome.destinationLabel}</span>
            <span>
              <StatusBadge tone={outcome.rejected === null ? "success" : "warning"}>
                {outcome.rejected === null ? "Accepted" : rejectionTitle(outcome.rejected.code)}
              </StatusBadge>
            </span>
            <span>{outcomeText(outcome, mode)}</span>
          </li>
        ))}
      </ul>
      {result.newMessageId !== null && (
        <p>
          <Link href={`/messages/${encodeURIComponent(result.newMessageId)}`}>
            Open the new message
          </Link>
        </p>
      )}
    </>
  );
}
