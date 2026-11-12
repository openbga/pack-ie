import type {
  ControllerFieldState,
  ControllerRenderProps,
  FieldValues,
  Path,
} from "react-hook-form";

/** Maps a react-hook-form controller onto a text field's props. */
export function textFieldProps<Values extends FieldValues, Name extends Path<Values>>({
  field,
  fieldState,
}: {
  field: ControllerRenderProps<Values, Name>;
  fieldState: ControllerFieldState;
}) {
  return {
    name: field.name,
    value: field.value,
    onChange: field.onChange,
    onBlur: field.onBlur,
    inputRef: field.ref,
    errorMessage: fieldState.error?.message,
  };
}
