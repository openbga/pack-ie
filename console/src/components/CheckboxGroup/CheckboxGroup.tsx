import { useId } from "react";
import { Checkbox } from "@/components/Checkbox";
import styles from "./CheckboxGroup.module.css";

interface CheckboxGroupOption {
  id: string;
  label: string;
  /** Lines shown under the label, such as what a choice allows. */
  details?: readonly string[];
}

interface CheckboxGroupProps {
  legend: string;
  options: readonly CheckboxGroupOption[];
  /** The selected option IDs. */
  value: readonly string[];
  onChange: (value: string[]) => void;
  isRequired?: boolean;
  errorMessage?: string | undefined;
  /**
   * `columns` for a long list in a wide dialog: a titled section like `FieldSection`, with the
   * options side by side.
   */
  layout?: "list" | "columns";
}

/** A labelled group of checkboxes that selects a list of IDs. */
export function CheckboxGroup({
  legend,
  options,
  value,
  onChange,
  isRequired,
  errorMessage,
  layout = "list",
}: CheckboxGroupProps) {
  const errorId = useId();
  return (
    <fieldset
      className={styles.group}
      aria-describedby={errorMessage ? errorId : undefined}
      data-invalid={errorMessage ? true : undefined}
      data-layout={layout}
    >
      <legend className={styles.legend}>
        {legend}
        {isRequired && (
          <span className={styles.required} aria-hidden="true">
            {" "}
            *
          </span>
        )}
      </legend>
      <div className={styles.options}>
        {options.map((option) => (
          <div key={option.id} className={styles.option}>
            <Checkbox
              isSelected={value.includes(option.id)}
              onChange={(isSelected) => onChange(toggle(value, option.id, isSelected))}
            >
              {option.label}
            </Checkbox>
            {option.details?.map((detail) => (
              <p key={detail} className={styles.detail}>
                {detail}
              </p>
            ))}
          </div>
        ))}
      </div>
      {errorMessage && (
        <p id={errorId} className={styles.error}>
          {errorMessage}
        </p>
      )}
    </fieldset>
  );
}

function toggle(selected: readonly string[], id: string, isSelected: boolean): string[] {
  const others = selected.filter((each) => each !== id);
  return isSelected ? [...others, id] : others;
}
