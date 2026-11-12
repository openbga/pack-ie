import { Controller, useFormContext } from "react-hook-form";
import { Checkbox } from "@/components/Checkbox";
import styles from "./config.module.css";

interface FormCheckboxProps {
  name: string;
  label: string;
  /** One sentence under the box on what ticking it does. */
  description?: string;
  /** A check against other fields; returns the message when the box cannot be as it is. */
  validate?: (checked: boolean) => true | string;
}

/** A checkbox bound to a boolean in the surrounding react-hook-form form. */
export function FormCheckbox({ name, label, description, validate }: FormCheckboxProps) {
  const { control } = useFormContext();
  return (
    <Controller
      control={control}
      name={name}
      rules={{ validate: (value: boolean) => validate?.(value === true) ?? true }}
      render={({ field, fieldState }) => (
        <div className={styles.checkField}>
          <Checkbox name={field.name} isSelected={field.value === true} onChange={field.onChange}>
            {label}
          </Checkbox>
          {description && <p className={styles.note}>{description}</p>}
          {fieldState.error && (
            <p role="alert" className={styles.error}>
              {fieldState.error.message}
            </p>
          )}
        </div>
      )}
    />
  );
}
