import { TextArea, TextField } from "react-aria-components";
import { Controller, useFormContext } from "react-hook-form";
import { FieldHelp } from "@/components/Form/FieldHelp";
import { FieldLabel } from "@/components/Form/FieldLabel";
import formStyles from "@/components/Form/Form.module.css";
import styles from "./ReasonField.module.css";

/** The most characters a resend reason may have (message-channels.md §9). */
const RESEND_REASON_MAX = 500;

/** The characters of a reason as the engine counts them, without surrounding spaces. */
const lengthOf = (text: string) => [...text.trim()].length;

/**
 * The optional reason of a resend, bound to the surrounding form's `name` value: a few lines of
 * text with a character count. Empty means none is recorded.
 */
export function ReasonField({ name }: { name: string }) {
  const { control } = useFormContext();
  return (
    <Controller
      control={control}
      name={name}
      rules={{
        validate: (value: string) =>
          lengthOf(value) <= RESEND_REASON_MAX || `Use ${RESEND_REASON_MAX} characters or fewer.`,
      }}
      render={({ field, fieldState }) => (
        <TextField
          name={field.name}
          value={field.value}
          onChange={field.onChange}
          onBlur={field.onBlur}
          isInvalid={fieldState.error ? true : undefined}
          validationBehavior="aria"
          className={formStyles.field}
        >
          <FieldLabel>Reason (optional)</FieldLabel>
          <TextArea
            ref={field.ref}
            className={`${formStyles.control} ${styles.area}`}
            rows={3}
            autoComplete="off"
          />
          <FieldHelp
            description={`Recorded with the new attempt and visible to everyone who opens this message. ${lengthOf(field.value)} of ${RESEND_REASON_MAX} characters.`}
            errorMessage={fieldState.error?.message}
          />
        </TextField>
      )}
    />
  );
}
