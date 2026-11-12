import { useState } from "react";
import styles from "./DateTimeField.module.css";

/** An RFC 3339 time as the value of a local `datetime-local` input; empty for no or a relative time. */
function toLocalInput(value: string | undefined): string {
  const date = new Date(value ?? "");
  if (value === undefined || Number.isNaN(date.getTime())) return "";
  const pad = (part: number) => String(part).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

interface DateTimeFieldProps {
  label: string;
  /** The filter's time, RFC 3339. */
  value: string | undefined;
  /** The time typed, as RFC 3339, or `undefined` once cleared. */
  onCommit: (value: string | undefined) => void;
}

/**
 * A date and time in this computer's time zone. It reports once editing is done (leaving the
 * field or Enter): while a part is half typed the browser's value is empty, which must not
 * clear the filter.
 */
export function DateTimeField({ label, value, onCommit }: DateTimeFieldProps) {
  const current = toLocalInput(value);
  const [draft, setDraft] = useState(current);
  // The filter can change from outside (a quick range, "Clear filters"): show it then.
  const [shown, setShown] = useState(current);
  if (current !== shown) {
    setShown(current);
    setDraft(current);
  }
  function commit() {
    if (draft === current) return;
    const time = new Date(draft);
    onCommit(draft === "" || Number.isNaN(time.getTime()) ? undefined : time.toISOString());
  }
  return (
    <label className={styles.dateField}>
      <span className={styles.dateLabel}>{label}</span>
      <input
        type="datetime-local"
        className={styles.dateInput}
        value={draft}
        onChange={(event) => setDraft(event.target.value)}
        onBlur={commit}
        onKeyDown={(event) => {
          if (event.key === "Enter") commit();
        }}
      />
    </label>
  );
}
