import { Alert } from "@/components/Alert";
import { FieldGrid } from "@/components/Form";
import { TextInput } from "@/components/FormFields";
import styles from "./config.module.css";
import { optionalWholeNumber, wholeNumber } from "./validators";

interface RetryFieldsProps {
  /** The form path of the policy: `retry` or `destinations.0.retry`. */
  prefix: string;
  /** Bridges retry for ever when the attempts are left empty; destinations need a number. */
  attempts: "required" | "optional";
  description: string;
}

/** Retry policy fields (`retry.*`). */
export function RetryFields({ prefix, attempts, description }: RetryFieldsProps) {
  return (
    <>
      <p className={styles.note}>{description}</p>
      <FieldGrid>
        <TextInput
          name={`${prefix}.initialMs`}
          label="Initial backoff (ms)"
          isRequired
          validate={wholeNumber()}
        />
        <TextInput
          name={`${prefix}.maxMs`}
          label="Maximum backoff (ms)"
          isRequired
          validate={wholeNumber()}
        />
        {attempts === "required" ? (
          <TextInput
            name={`${prefix}.maxAttempts`}
            label="Maximum attempts"
            isRequired
            validate={wholeNumber()}
            description="After this many attempts a delivery is marked failed and waits for an operator."
          />
        ) : (
          <TextInput
            name={`${prefix}.maxAttempts`}
            label="Maximum attempts"
            validate={optionalWholeNumber}
            description="Leave empty to keep retrying for ever."
          />
        )}
      </FieldGrid>
    </>
  );
}

const MESSAGE_RETENTION_NOTE =
  "Messages with deliveries that are not yet acknowledged or closed are never purged, whatever these limits say. Leave a field empty for no limit.";

/**
 * Retention fields: either limit may be empty for no limit (MSG-08). `retention.*` of a message
 * channel by default; a bridge's tap keeps what it records under `tapRetention.*`, with its own note.
 */
export function RetentionFields({
  prefix = "retention",
  note = MESSAGE_RETENTION_NOTE,
}: {
  prefix?: string;
  note?: string;
}) {
  return (
    <>
      <Alert tone="info" compact>
        {note}
      </Alert>
      <FieldGrid>
        <TextInput name={`${prefix}.days`} label="Keep for (days)" validate={optionalWholeNumber} />
        <TextInput
          name={`${prefix}.maxMessages`}
          label="Keep at most (messages)"
          validate={optionalWholeNumber}
        />
      </FieldGrid>
    </>
  );
}
