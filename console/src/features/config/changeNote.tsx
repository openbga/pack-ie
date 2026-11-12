import { useRef, useState } from "react";
import { TextField } from "@/components/TextField";

/** Change control (configuration.md section 10): a configuration change says why. */
const NOTE_MAX = 500;
/** A rollback's note is `rollback of <N>: <reason>`, so its reason has less room. */
export const ROLLBACK_REASON_MAX = 450;

const BLANK_NOTE = "Enter why you are making this change.";

/** What is wrong with a note, or null when it will do. */
function noteProblem(note: string, max = NOTE_MAX, blank = BLANK_NOTE): string | null {
  const length = note.trim().length;
  if (length === 0) return blank;
  if (length > max) return `Shorten the note to ${max} characters or fewer (now ${length}).`;
  return null;
}

/** A `validate` for `TextInput`: the same rule as `noteProblem` (blank is `isRequired`'s). */
export function changeNoteValidator(value: string): true | string {
  return noteProblem(value) ?? true;
}

/**
 * The note of one change, outside a form: its text, and `check`, which a confirmation calls
 * first. A note that will not do is refused: the problem shows under the field and focus moves
 * to it, so nothing is applied without a reason. `blank` words the problem of an empty note
 * for a field that is not a reason for a change.
 */
export function useChangeNote(max = NOTE_MAX, blank = BLANK_NOTE) {
  const [note, setNote] = useState("");
  const [shown, setShown] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  const problem = noteProblem(note, max, blank);
  return {
    note,
    setNote,
    max,
    inputRef: input,
    /** The problem to show: only once the operator has tried to go on, or has typed too much. */
    error: shown || note.trim().length > max ? problem : null,
    check(): boolean {
      if (problem === null) return true;
      setShown(true);
      input.current?.focus();
      return false;
    },
    reset() {
      setNote("");
      setShown(false);
    },
  };
}

type ChangeNote = ReturnType<typeof useChangeNote>;

/** The required "why" field of a change: label, limit and, when refused, the reason. */
export function ChangeNoteField({
  state,
  label = "Change note",
  description,
}: {
  state: ChangeNote;
  label?: string;
  description?: string;
}) {
  return (
    <TextField
      label={label}
      name="changeNote"
      value={state.note}
      onChange={state.setNote}
      inputRef={state.inputRef}
      isRequired
      description={
        description ??
        `Why you are making this change, 1 to ${state.max} characters. It is recorded with the version and in the audit log.`
      }
      errorMessage={state.error ?? undefined}
      autoComplete="off"
    />
  );
}
