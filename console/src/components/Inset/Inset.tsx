import type { ReactNode } from "react";
import styles from "./Inset.module.css";

interface InsetProps {
  /** `fieldset` for a group of fields (put its `legend` first), `li` inside a list. */
  as?: "div" | "fieldset" | "li";
  children: ReactNode;
}

/** A bordered, sunken box that sets one item apart inside a section, such as one row of fields. */
export function Inset({ as: Element = "div", children }: InsetProps) {
  return <Element className={styles.inset}>{children}</Element>;
}
