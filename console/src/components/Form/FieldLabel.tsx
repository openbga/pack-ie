import { Label } from "react-aria-components";
import styles from "./Form.module.css";

interface FieldLabelProps {
  children: string;
  isRequired?: boolean;
  /** Read by screen readers only: the field's value names it, such as "All connections". */
  isHidden?: boolean;
}

/** A field's visible label, with the required marker. */
export function FieldLabel({ children, isRequired, isHidden }: FieldLabelProps) {
  return (
    <Label className={isHidden ? `${styles.label} visually-hidden` : styles.label}>
      {children}
      {isRequired && (
        <span className={styles.required} aria-hidden="true">
          {" "}
          *
        </span>
      )}
    </Label>
  );
}
