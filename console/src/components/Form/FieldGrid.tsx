import type { ReactNode } from "react";
import styles from "./Form.module.css";

/** Fields side by side where they fit, one per row on narrow windows. */
export function FieldGrid({ children }: { children: ReactNode }) {
  return <div className={styles.fieldGrid}>{children}</div>;
}
