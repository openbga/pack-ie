import type { ReactNode, Ref } from "react";
import { TextField as AriaTextField, Input } from "react-aria-components";
import { FieldHelp } from "@/components/Form/FieldHelp";
import { FieldLabel } from "@/components/Form/FieldLabel";
import formStyles from "@/components/Form/Form.module.css";

interface TextFieldProps {
  label: string;
  /** Also the key for engine field errors shown by `Form`. */
  name: string;
  value: string;
  onChange: (value: string) => void;
  onBlur?: () => void;
  type?: "text" | "password";
  isRequired?: boolean;
  isDisabled?: boolean;
  description?: string;
  errorMessage?: string | undefined;
  autoComplete?: string;
  autoCapitalize?: string;
  autoCorrect?: string;
  spellCheck?: boolean;
  inputMode?: "text" | "numeric";
  autoFocus?: boolean;
  inputRef?: Ref<HTMLInputElement>;
  /** A small button inside the field, at its end, such as a date picker. */
  adornment?: ReactNode;
}

export function TextField({
  label,
  description,
  errorMessage,
  inputRef,
  isRequired,
  autoCapitalize,
  autoCorrect,
  spellCheck,
  adornment,
  ...rest
}: TextFieldProps) {
  const input = (
    <Input
      ref={inputRef}
      className={formStyles.control}
      autoCapitalize={autoCapitalize}
      autoCorrect={autoCorrect}
      spellCheck={spellCheck}
    />
  );
  return (
    <AriaTextField
      {...rest}
      validationBehavior="aria"
      isRequired={isRequired}
      isInvalid={errorMessage === undefined ? undefined : true}
      className={formStyles.field}
    >
      <FieldLabel isRequired={isRequired}>{label}</FieldLabel>
      {adornment ? (
        <div className={formStyles.adorned}>
          {input}
          {adornment}
        </div>
      ) : (
        input
      )}
      <FieldHelp description={description} errorMessage={errorMessage} />
    </AriaTextField>
  );
}
