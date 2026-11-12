import { ChevronDown } from "lucide-react";
import type { Ref } from "react";
import {
  Select as AriaSelect,
  Button,
  ListBox,
  ListBoxItem,
  Popover,
  SelectValue,
} from "react-aria-components";
import { FieldHelp } from "@/components/Form/FieldHelp";
import { FieldLabel } from "@/components/Form/FieldLabel";
import formStyles from "@/components/Form/Form.module.css";
import styles from "./Select.module.css";

interface SelectOption {
  id: string;
  label: string;
}

interface SelectProps {
  label: string;
  /** Compact collection filter; form fields keep their stacked label. */
  layout?: "inline";
  /**
   * The label is read by screen readers only, for a filter whose options name it ("All
   * connections", "All outcomes") or a row whose columns do.
   */
  isLabelHidden?: boolean;
  name: string;
  options: readonly SelectOption[];
  value: string | null;
  onChange: (value: string | null) => void;
  placeholder?: string;
  isRequired?: boolean;
  isDisabled?: boolean;
  description?: string;
  errorMessage?: string | undefined;
  /** The trigger button, so a form can focus the field when it is invalid. */
  inputRef?: Ref<HTMLButtonElement>;
}

export function Select({
  label,
  layout,
  isLabelHidden,
  options,
  onChange,
  description,
  errorMessage,
  isRequired,
  placeholder = "Select…",
  inputRef,
  ...rest
}: SelectProps) {
  return (
    <AriaSelect
      {...rest}
      data-layout={layout}
      validationBehavior="aria"
      placeholder={placeholder}
      isRequired={isRequired}
      isInvalid={errorMessage === undefined ? undefined : true}
      onChange={(key) => onChange(key === null ? null : String(key))}
      className={formStyles.field}
    >
      <FieldLabel isRequired={isRequired} isHidden={isLabelHidden}>
        {label}
      </FieldLabel>
      <Button ref={inputRef} className={`${formStyles.control} ${styles.trigger}`}>
        <SelectValue className={styles.value} />
        <ChevronDown aria-hidden="true" className={styles.chevron} />
      </Button>
      <FieldHelp description={description} errorMessage={errorMessage} />
      <Popover className={styles.popover}>
        <ListBox items={options} className={styles.listBox}>
          {(option) => (
            <ListBoxItem id={option.id} className={styles.option}>
              {option.label}
            </ListBoxItem>
          )}
        </ListBox>
      </Popover>
    </AriaSelect>
  );
}
