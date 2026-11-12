import { type ReactNode, useContext, useId, useState } from "react";
import { FormValidationContext } from "react-aria-components";
import { Button } from "@/components/Button";
import { FieldGrid } from "@/components/Form";
import { Inset } from "@/components/Inset";
import styles from "./FieldList.module.css";

interface FieldListProps {
  /** The form path of the list, such as `diagnoses`. */
  name: string;
  legend: string;
  /** What the rows or their order mean, under the legend. */
  description?: string;
  /** Shown when the list is empty, such as "No diagnoses on the requisition." */
  emptyText: string;
  /** The rows, with react-hook-form's `useFieldArray` keys. */
  items: readonly { id: string }[];
  /** The legend of one row, such as "Diagnosis 2". */
  itemLabel: (index: number) => string;
  addLabel: string;
  onAdd: () => void;
  /** Gives each row Up and Down buttons, for lists where the order means something. */
  onMove?: (from: number, to: number) => void;
  onRemove: (index: number) => void;
  renderItem: (index: number) => ReactNode;
  /** More buttons beside the add button, for actions on the whole list. */
  extraActions?: ReactNode;
}

/**
 * A repeatable group of fields: as many rows as the requisition supplies, none assumed. The
 * engine's error for the whole list, such as "Add at least one analyte.", shows under the
 * legend as the surrounding `Form` names it, and the list takes the form's error focus. With
 * `onMove` the rows can be reordered from the keyboard, and the row that moved is announced.
 */
export function FieldList({
  name,
  legend,
  description,
  emptyText,
  items,
  itemLabel,
  addLabel,
  onAdd,
  onMove,
  onRemove,
  renderItem,
  extraActions,
}: FieldListProps) {
  const errorId = useId();
  const [announcement, setAnnouncement] = useState("");
  const engineError = useContext(FormValidationContext)[name];
  const errorMessage = Array.isArray(engineError) ? engineError.join(" ") : engineError;
  function move(index: number, to: number) {
    if (onMove === undefined || to < 0 || to >= items.length) return;
    onMove(index, to);
    setAnnouncement(`${itemLabel(index)} moved to position ${to + 1} of ${items.length}.`);
  }
  return (
    <fieldset
      className={styles.list}
      aria-describedby={errorMessage ? errorId : undefined}
      aria-invalid={errorMessage ? true : undefined}
      tabIndex={errorMessage ? -1 : undefined}
    >
      <legend className={styles.legend}>{legend}</legend>
      {description && <p className={styles.note}>{description}</p>}
      {errorMessage && (
        <p id={errorId} className={styles.error}>
          {errorMessage}
        </p>
      )}
      {onMove && (
        <p role="status" className="visually-hidden">
          {announcement}
        </p>
      )}
      {items.length === 0 && <p className={styles.note}>{emptyText}</p>}
      {items.map((item, index) => {
        const label = itemLabel(index).toLowerCase();
        return (
          <Inset key={item.id} as="fieldset">
            <legend className={styles.legend}>{itemLabel(index)}</legend>
            <FieldGrid>{renderItem(index)}</FieldGrid>
            <div className={styles.rowActions}>
              {onMove && (
                <>
                  <Button variant="quiet" onPress={() => move(index, index - 1)}>
                    Move {label} up
                  </Button>
                  <Button variant="quiet" onPress={() => move(index, index + 1)}>
                    Move {label} down
                  </Button>
                </>
              )}
              <Button variant="quiet" onPress={() => onRemove(index)}>
                Remove {label}
              </Button>
            </div>
          </Inset>
        );
      })}
      <div className={styles.rowActions}>
        <Button onPress={onAdd}>{addLabel}</Button>
        {extraActions}
      </div>
    </fieldset>
  );
}
