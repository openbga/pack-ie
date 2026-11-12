import { Controller, type FieldValues, useFormContext } from "react-hook-form";
import { Select } from "@/components/Select";

interface ChoiceInputProps {
  /** The form path; also the key for engine field errors. */
  name: string;
  label: string;
  options: readonly { id: string; label: string }[];
  isRequired?: boolean;
  /** Nothing to choose yet, such as a location before its client is chosen. */
  isDisabled?: boolean;
  description?: string;
  /** The label is read by screen readers only, such as in a row whose columns name it. */
  isLabelHidden?: boolean;
  /** Called after the form value changes, such as to clear fields that depend on it. */
  onChange?: (value: string | null) => void;
  /**
   * An additional check, such as against another field (`values` are the form's); returns the
   * message. A blank choice is `isRequired`'s to report.
   */
  validate?: (value: string | null, values: FieldValues) => true | string;
}

/**
 * A choice bound to the surrounding react-hook-form form.
 */
export function ChoiceInput({
  name,
  label,
  options,
  isRequired,
  isDisabled,
  description,
  isLabelHidden,
  onChange,
  validate,
}: ChoiceInputProps) {
  const { control } = useFormContext();
  return (
    <Controller
      control={control}
      name={name}
      rules={{
        required: isRequired ? `Choose ${label.toLowerCase()}.` : false,
        validate: (value: string | null, values: FieldValues) => validate?.(value, values) ?? true,
      }}
      render={({ field, fieldState }) => (
        <Select
          label={label}
          name={field.name}
          inputRef={field.ref}
          options={options}
          value={field.value}
          onChange={(value) => {
            field.onChange(value);
            onChange?.(value);
          }}
          isRequired={isRequired}
          isDisabled={isDisabled}
          isLabelHidden={isLabelHidden}
          description={description}
          errorMessage={fieldState.error?.message}
        />
      )}
    />
  );
}
