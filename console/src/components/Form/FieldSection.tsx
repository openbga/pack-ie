import type { ReactNode } from "react";
import styles from "./Form.module.css";

/**
 * A titled group of related fields in a long form. Put its short fields in a `FieldGrid` so
 * they sit side by side.
 */
export function FieldSection({ title, children }: { title: string; children: ReactNode }) {
  return (
    <fieldset className={styles.fieldSection}>
      <legend className={styles.fieldSectionTitle}>{title}</legend>
      {children}
    </fieldset>
  );
}
