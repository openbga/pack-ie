import { Check } from "lucide-react";
import type { ReactNode } from "react";
import { ToggleButton } from "react-aria-components";
import styles from "./ToggleChip.module.css";

interface ToggleChipProps {
  isSelected: boolean;
  isDisabled?: boolean;
  onChange: (isSelected: boolean) => void;
  children: ReactNode;
}

/** A pill that is on or off, such as a quick filter or Follow; a check marks it when on. */
export function ToggleChip({ isSelected, isDisabled, onChange, children }: ToggleChipProps) {
  return (
    <ToggleButton
      className={styles.chip}
      isSelected={isSelected}
      isDisabled={isDisabled}
      onChange={onChange}
    >
      {isSelected && <Check aria-hidden="true" className={styles.check} />}
      {children}
    </ToggleButton>
  );
}
