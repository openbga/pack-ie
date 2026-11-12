import type { ReactNode } from "react";
import type { LastError } from "@/api/types";
import { Alert } from "@/components/Alert";
import { Timestamp } from "@/components/Timestamp";
import { formatAge } from "@/lib/format";
import { parseErrorSummary } from "./errorSummary";
import styles from "./LastErrorNote.module.css";

/** What the snapshot knows around an error, for the full structured view. */
export interface ErrorContext {
  /** The failure streak of the endpoint that failed, when known. */
  failureStreak?: number | null;
  /** The retry state in words, such as "Retry 3 in 5 s" or "Waiting for peer". */
  retry?: ReactNode;
  /** What the retry is of; `Retry` when absent. A destination's is `Delivery retry`. */
  retryLabel?: string;
  /** The endpoint the error belongs to, shown when the summary does not name it. */
  where?: ReactNode;
  /** The health of the source session, to tell it apart from a destination's retries. */
  session?: ReactNode;
}

/**
 * The rows that say what failed: from the engine's pattern the operation, the endpoint and the
 * cause; otherwise the part the page resolved. The page's own wording of the part that failed
 * (destination labels, not ids) wins over the pattern's role.
 */
function whereRows(
  parts: ReturnType<typeof parseErrorSummary>,
  where: ReactNode | undefined,
): { label: string; value: ReactNode }[] {
  if (parts !== null) {
    return [
      { label: "Operation", value: parts.operation },
      { label: "Endpoint", value: where ?? `${parts.role}: ${parts.endpointLabel}` },
      { label: "Cause", value: parts.cause },
    ];
  }
  return where === undefined ? [] : [{ label: "Endpoint", value: where }];
}

/**
 * The last error of a channel or endpoint with what the operator should do next. `tone` says
 * how current it is: `danger` while the thing is failing, `warning` while it is recovering,
 * `info` once it runs normally again and this is history. With `context`, the full structure
 * of operator-console.md section 4: operation, endpoint role and label, cause, failure streak,
 * time, retry state and next action. Parts the summary does not carry are left out.
 */
export function LastErrorNote({
  error,
  tone,
  title = "Last error",
  context,
}: {
  error: LastError;
  tone: "danger" | "warning" | "info";
  title?: string;
  context?: ErrorContext;
}) {
  const parts = parseErrorSummary(error.summary);
  const rows: { label: string; value: ReactNode }[] = [];
  if (context !== undefined) {
    rows.push(...whereRows(parts, context.where));
    if (context.session !== undefined)
      rows.push({ label: "Source session", value: context.session });
    if (context.failureStreak !== undefined && context.failureStreak !== null) {
      rows.push({ label: "Failure streak", value: String(context.failureStreak) });
    }
    rows.push({
      label: "Time",
      value: (
        <>
          <Timestamp value={error.at} seconds /> ({formatAge(error.at)})
        </>
      ),
    });
    if (context.retry !== undefined)
      rows.push({ label: context.retryLabel ?? "Retry", value: context.retry });
  }
  return (
    <Alert tone={tone} title={title}>
      <p>{error.summary}</p>
      {rows.length > 0 && (
        <dl className={styles.context}>
          {rows.map((row) => (
            <div key={row.label} className={styles.row}>
              <dt>{row.label}</dt>
              <dd>{row.value}</dd>
            </div>
          ))}
        </dl>
      )}
      <p>
        <strong>Next step:</strong> {error.nextAction}
      </p>
      {context === undefined && (
        <p className={styles.when}>
          <Timestamp value={error.at} seconds /> ({formatAge(error.at)})
        </p>
      )}
    </Alert>
  );
}
