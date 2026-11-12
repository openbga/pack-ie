import { TextInput } from "@/components/FormFields";
import { Inset } from "@/components/Inset";
import { ConnectorFields, type EndpointOption } from "./ConnectorFields";
import styles from "./config.module.css";
import { FilterEditor } from "./FilterEditor";
import { FormCheckbox } from "./FormCheckbox";
import { MappingEditor } from "./MappingEditor";
import { RetryFields } from "./PolicyFields";

const MAX_LABEL = 100;

/**
 * One destination: its label, where and how it delivers, which messages it takes, how they
 * are mapped, and how failures are retried.
 */
export function DestinationFields({
  index,
  endpoints,
}: {
  index: number;
  endpoints: readonly EndpointOption[];
}) {
  const prefix = `destinations.${index}`;
  const owner = `Destination ${index + 1}`;
  return (
    <>
      <TextInput
        name={`${prefix}.label`}
        label="Label"
        isRequired
        description="The name shown for this destination in deliveries and alerts."
        validate={(value) => value.length <= MAX_LABEL || `Use at most ${MAX_LABEL} characters.`}
      />
      <FormCheckbox
        name={`${prefix}.enabled`}
        label="Enabled"
        description="A disabled destination gets no new deliveries; the ones already queued wait."
      />
      <ConnectorFields
        prefix={prefix}
        endpoints={endpoints}
        side="destination"
        destinationIndex={index}
      />
      <div className={styles.nested}>
        <FilterEditor
          name={`${prefix}.filters`}
          owner={owner}
          description="A delivery to this destination is made only when all of these match. With no filters, every message that passed the source filters is delivered."
        />
      </div>
      <MappingEditor root={prefix} owner={owner} />
      <div className={styles.nested}>
        <Inset as="fieldset">
          <legend className={styles.legend}>Retry policy of {owner.toLowerCase()}</legend>
          <RetryFields
            prefix={`${prefix}.retry`}
            attempts="required"
            description="A failed delivery is retried with growing delays until it is acknowledged or the attempts run out."
          />
        </Inset>
      </div>
    </>
  );
}
