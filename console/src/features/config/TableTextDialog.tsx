import { useState } from "react";
import { TextArea, TextField } from "react-aria-components";
import { useFormContext } from "react-hook-form";
import { Button } from "@/components/Button";
import { Dialog } from "@/components/Dialog";
import { FieldHelp } from "@/components/Form/FieldHelp";
import { FieldLabel } from "@/components/Form/FieldLabel";
import formStyles from "@/components/Form/Form.module.css";
import styles from "./config.module.css";
import type { CodeTableFormValues } from "./ruleForm";
import { compareRows, rowsToText, textToRows } from "./tableText";

interface TableTextDialogProps {
  /** The form path of the table's rows: `destinations.0.codeTables.1.rows`. */
  name: string;
  /** Names the table in labels: `destination 1 table 2`. */
  tableLabel: string;
  /** Replaces the rows with the ones the text holds. */
  onApply: (rows: CodeTableFormValues["rows"]) => void;
}

/**
 * Edits one code table as text: copy the lines out, or paste a spreadsheet's two columns in. The
 * summary says what applying would add, change and remove before anything changes.
 */
export function TableTextDialog({ name, tableLabel, onApply }: TableTextDialogProps) {
  const { getValues } = useFormContext();
  const [open, setOpen] = useState(false);
  const [text, setText] = useState("");
  const current = (): CodeTableFormValues["rows"] => getValues(name) ?? [];
  const parsed = textToRows(text);
  const changes = compareRows(current(), parsed.rows);
  const blocked = parsed.problems.length > 0;
  const close = () => setOpen(false);
  return (
    <>
      <Button
        onPress={() => {
          setText(rowsToText(current()));
          setOpen(true);
        }}
      >
        {`Edit ${tableLabel} as text`}
      </Button>
      {open && (
        <Dialog
          title={`Edit ${tableLabel} as text`}
          description="One code per line, the code and its text separated by a comma or a tab. Paste two spreadsheet columns here to fill the table at once."
          isOpen
          onOpenChange={(isOpen) => {
            if (!isOpen) close();
          }}
          size="wide"
          onSubmit={() => {
            if (blocked) return;
            onApply(parsed.rows);
            close();
          }}
          actions={
            <>
              <Button onPress={close}>Cancel</Button>
              <Button type="submit" variant="primary" isDisabled={blocked}>
                Replace rows
              </Button>
            </>
          }
        >
          <TextField value={text} onChange={setText} className={formStyles.field}>
            <FieldLabel>Rows</FieldLabel>
            <TextArea
              className={`${formStyles.control} ${styles.template}`}
              spellCheck={false}
              autoCapitalize="off"
              autoCorrect="off"
              rows={14}
            />
            <FieldHelp description='A text that holds a comma or starts with a quote goes in quotes, "like, this". Spaces around the separator are dropped.' />
          </TextField>
          {blocked ? (
            <ul role="alert" className={styles.problems}>
              {parsed.problems.slice(0, 10).map((problem) => (
                <li key={`${problem.line}-${problem.message}`}>
                  Line {problem.line}: {problem.message}
                </li>
              ))}
              {parsed.problems.length > 10 && <li>And {parsed.problems.length - 10} more.</li>}
            </ul>
          ) : (
            <p role="status" className={styles.valid}>
              {`${parsed.rows.length} ${parsed.rows.length === 1 ? "row" : "rows"}: ${changes.added} new, ${changes.changed} changed, ${changes.removed} removed, ${changes.unchanged} unchanged.`}
            </p>
          )}
        </Dialog>
      )}
    </>
  );
}
