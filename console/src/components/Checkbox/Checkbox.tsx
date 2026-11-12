import type { ReactNode } from "react";
import { Checkbox as AriaCheckbox } from "react-aria-components";
import styles from "./Checkbox.module.css";
import { CheckMark } from "./CheckMark";

interface CheckboxProps {
  /** The visible label. */
  children: ReactNode;
  isSelected: boolean;
  onChange: (isSelected: boolean) => void;
  /** Some, not all, of a group is selected. */
  isIndeterminate?: boolean;
  isDisabled?: boolean;
  name?: string;
  /** Takes focus when it appears, such as a confirmation the engine just asked for. */
  autoFocus?: boolean;
}

export function Checkbox({ children, ...rest }: CheckboxProps) {
  return (
    <AriaCheckbox {...rest} className={styles.checkbox}>
      {({ isSelected, isIndeterminate }) => (
        <>
          <CheckMark state={markState(isSelected, isIndeterminate)} />
          {children}
        </>
      )}
    </AriaCheckbox>
  );
}

function markState(isSelected: boolean, isIndeterminate: boolean) {
  if (isIndeterminate) return "mixed";
  return isSelected ? "checked" : "unchecked";
}
