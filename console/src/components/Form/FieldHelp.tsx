import { FieldError, Text } from "react-aria-components";
import styles from "./Form.module.css";

interface FieldHelpProps {
  description?: string | undefined;
  /** A client-side message; without it, the field shows the engine's error for its name. */
  errorMessage?: string | undefined;
}

/** A field's description and error text, linked to the input for assistive technology. */
export function FieldHelp({ description, errorMessage }: FieldHelpProps) {
  return (
    <>
      {description && (
        <Text slot="description" className={styles.description}>
          {description}
        </Text>
      )}
      <FieldError className={styles.error}>{errorMessage}</FieldError>
    </>
  );
}
