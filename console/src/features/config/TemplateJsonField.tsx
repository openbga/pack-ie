import { TextArea, TextField } from "react-aria-components";
import { Controller, useFormContext } from "react-hook-form";
import { FieldHelp } from "@/components/Form/FieldHelp";
import { FieldLabel } from "@/components/Form/FieldLabel";
import formStyles from "@/components/Form/Form.module.css";
import styles from "./config.module.css";
import { jsonObjectProblem } from "./validators";

/**
 * The column template of a delimited connector (docs/spec/configuration.md §7.4): a JSON
 * object in a monospaced box, with a line that says at once whether it can be read.
 */
export function TemplateJsonField({ name }: { name: string }) {
  const { control } = useFormContext();
  return (
    <Controller
      control={control}
      name={name}
      rules={{ validate: (value: string) => jsonObjectProblem(value) ?? true }}
      render={({ field, fieldState }) => {
        const problem = jsonObjectProblem(field.value);
        return (
          <TextField
            name={field.name}
            value={field.value}
            onChange={field.onChange}
            onBlur={field.onBlur}
            isRequired
            isInvalid={fieldState.error ? true : undefined}
            validationBehavior="aria"
            className={`${formStyles.field} ${styles.wide}`}
          >
            <FieldLabel isRequired>Column template (JSON)</FieldLabel>
            <TextArea
              ref={field.ref}
              className={`${formStyles.control} ${styles.template}`}
              spellCheck={false}
              autoCapitalize="off"
              autoCorrect="off"
              rows={10}
            />
            <FieldHelp
              description="The delimiter, header row and columns of the file, written as a JSON object. A destination also needs a file_name."
              errorMessage={fieldState.error?.message}
            />
            {!fieldState.error && (
              <p role="status" className={problem === null ? styles.valid : styles.note}>
                {problem === null ? "Valid JSON object." : problem}
              </p>
            )}
          </TextField>
        );
      }}
    />
  );
}
