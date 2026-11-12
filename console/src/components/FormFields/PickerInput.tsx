import { Controller, type FieldValues, useFormContext } from "react-hook-form";
import { type RecordOption, RecordPicker } from "@/components/RecordPicker";

interface PickerInputProps {
  /** The form path of the chosen record's id; also the key for engine field errors. */
  name: string;
  label: string;
  options: readonly RecordOption[];
  isRequired?: boolean;
  isDisabled?: boolean;
  description?: string;
  /** Called after the form value changes, such as to clear fields that depend on it. */
  onChange?: (value: string | null) => void;
  /** An additional check, such as against another field; returns the message. */
  validate?: (value: string | null, values: FieldValues) => true | string;
}

/**
 * A searchable `RecordPicker` bound to the surrounding react-hook-form form: the counterpart
 * of `ChoiceInput` for lists with more than 15 options, or ones that keep growing.
 */
export function PickerInput({ name, label, onChange, validate, ...picker }: PickerInputProps) {
  const { control } = useFormContext();
  return (
    <Controller
      control={control}
      name={name}
      rules={{
        required: picker.isRequired ? `Choose ${label.toLowerCase()}.` : false,
        validate: (value: string | null, values: FieldValues) => validate?.(value, values) ?? true,
      }}
      render={({ field, fieldState }) => (
        <RecordPicker
          {...picker}
          label={label}
          name={field.name}
          inputRef={field.ref}
          value={field.value}
          onChange={(value) => {
            field.onChange(value);
            onChange?.(value);
          }}
          errorMessage={fieldState.error?.message}
        />
      )}
    />
  );
}
