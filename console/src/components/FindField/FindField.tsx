import { Search, X } from "lucide-react";
import { useState } from "react";
import { Button, Input, Label, SearchField } from "react-aria-components";
import { formatCount } from "@/lib/format";
import styles from "./FindField.module.css";
import { useTypingPause } from "./useTypingPause";

/** Rows shown and rows in total, for the count line. */
interface FindCountProps {
  shown: number;
  total: number;
  /** What the list holds, plural and lower case, such as "analytes". */
  noun: string;
  /** The same in the singular, such as "analyte", for a list of one. */
  one: string;
}

interface FindFieldProps {
  /** The field's specific accessible name, such as "Find analytes". */
  label: string;
  /**
   * The text shown beside the box, "Find" by default. A toolbar with several criteria names
   * each ("Identifier"); `label` stays the accessible name.
   */
  visibleLabel?: string;
  /** An example or the fields searched. */
  placeholder?: string;
  /** The find text in effect, usually from `?q=` (`useFindParam`). */
  value: string;
  /** Called after a short pause in typing, and at once on Enter, Escape or clear. */
  onChange: (value: string) => void;
  /** Shown as "12 of 340 analytes", or "340 analytes" when nothing is excluded. */
  count?: FindCountProps;
}

export function FindCount({ shown, total, noun, one }: FindCountProps) {
  const what = total === 1 ? one : noun;
  return shown === total
    ? `${formatCount(total)} ${what}`
    : `${formatCount(shown)} of ${formatCount(total)} ${what}`;
}

/**
 * Find in a directory: searches the identifiers and names its grid shows. Escape clears it.
 * The count line is a polite live region, so the new count is announced after each change.
 */
export function FindField({
  label,
  visibleLabel = "Find",
  placeholder = label,
  value,
  onChange,
  count,
}: FindFieldProps) {
  const [text, setText] = useState(value);
  // The text in effect can change from outside (Back, "Clear search"): show it then. Our own
  // report comes back trimmed; the box keeps what was typed, such as the space after a word.
  const [shownValue, setShownValue] = useState(value);
  const pause = useTypingPause(onChange);
  if (value !== shownValue) {
    setShownValue(value);
    if (value !== text.trim()) {
      pause.cancel();
      setText(value);
    }
  }
  // One of several criteria in a toolbar is not a search landmark of its own.
  const Wrapper = visibleLabel === "Find" ? "search" : "div";
  return (
    <Wrapper className={styles.find}>
      <SearchField
        value={text}
        onChange={(next) => {
          setText(next);
          pause.later(next);
        }}
        onSubmit={pause.now}
        onClear={() => pause.now("")}
        className={styles.field}
      >
        <Label className={styles.label}>
          <span aria-hidden="true">{visibleLabel}</span>
          <span className="visually-hidden">{label}</span>
        </Label>
        <div className={styles.control}>
          <Search aria-hidden="true" className={styles.icon} />
          <Input placeholder={placeholder} className={styles.input} />
          <Button className={styles.fieldButton}>
            <X aria-hidden="true" />
          </Button>
        </div>
      </SearchField>
      {count && (
        <p role="status" className={styles.count}>
          <FindCount {...count} />
        </p>
      )}
    </Wrapper>
  );
}
