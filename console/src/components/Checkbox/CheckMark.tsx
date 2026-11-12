import { Check, Minus } from "lucide-react";
import styles from "./Checkbox.module.css";

/** The checkbox square, also used for row selection in `DataGrid`. Decorative only. */
export function CheckMark({ state }: { state: "checked" | "mixed" | "unchecked" }) {
  return (
    <span className={styles.mark} data-state={state} aria-hidden="true">
      {state === "mixed" && <Minus />}
      {state === "checked" && <Check />}
    </span>
  );
}
