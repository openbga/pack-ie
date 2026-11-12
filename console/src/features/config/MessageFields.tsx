import { useState } from "react";
import { useFieldArray, useFormContext } from "react-hook-form";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { FieldList } from "@/components/FieldList";
import { FieldGrid, FieldSection } from "@/components/Form";
import layout from "@/styles/layout.module.css";
import { ConnectorFields, type EndpointOption } from "./ConnectorFields";
import { type ChannelFormValues, emptyDestination } from "./channelForm";
import styles from "./config.module.css";
import { nextId } from "./configDocument";
import { DestinationFields } from "./DestinationFields";
import { FilterEditor } from "./FilterEditor";
import { FormCheckbox } from "./FormCheckbox";
import { RetentionFields } from "./PolicyFields";
import { DeliveriesClosed, useDestinationClosed } from "./UnresolvedDeliveries";

/** The destinations the channel already has in the running configuration. */
export interface SavedDestinations {
  channelId: string;
  destinationIds: readonly string[];
}

/**
 * Removing a destination that is already configured closes its unresolved deliveries once the
 * change is applied, so it asks first and says so; one added in this form goes without asking.
 */
function RemoveSavedDestination({
  saved,
  index,
  connectorId,
  onClose,
  onRemove,
}: {
  saved: SavedDestinations;
  index: number;
  connectorId: string;
  onClose: () => void;
  onRemove: () => void;
}) {
  const { getValues } = useFormContext<ChannelFormValues>();
  const closedOf = useDestinationClosed(saved.channelId);
  const label = getValues(`destinations.${index}.label`);
  return (
    <ConfirmDialog
      title={`Remove destination ${index + 1}${label ? ` (${label})` : ""}?`}
      isOpen
      onOpenChange={(isOpen) => {
        if (!isOpen) onClose();
      }}
      confirmLabel="Remove destination"
      onConfirm={onRemove}
    >
      <div className={layout.stack}>
        <DeliveriesClosed noun="destination" counts={closedOf(connectorId)} />
        <p>Nothing changes until you save and apply; Cancel on the form keeps it as it is.</p>
      </div>
    </ConfirmDialog>
  );
}

function Destinations({
  endpoints,
  saved,
}: {
  endpoints: readonly EndpointOption[];
  saved: SavedDestinations | null;
}) {
  const { control, formState } = useFormContext<ChannelFormValues>();
  const [removing, setRemoving] = useState<number | null>(null);
  const { fields, append, remove } = useFieldArray({
    control,
    name: "destinations",
    rules: { required: "Add at least one destination." },
  });
  const missing = formState.errors.destinations?.root?.message;
  return (
    <FieldSection title="Destinations">
      <FieldList
        name="destinations"
        legend="Deliver to"
        emptyText="No destinations yet."
        items={fields}
        itemLabel={(index) => `Destination ${index + 1}`}
        addLabel="Add destination"
        onAdd={() =>
          append(
            emptyDestination(
              nextId(
                "d",
                fields.map((field) => field.connectorId),
              ),
            ),
          )
        }
        onRemove={(index) => {
          const connectorId = fields[index]?.connectorId;
          if (connectorId !== undefined && saved?.destinationIds.includes(connectorId)) {
            setRemoving(index);
          } else {
            remove(index);
          }
        }}
        renderItem={(index) => <DestinationFields index={index} endpoints={endpoints} />}
      />
      {missing && (
        <p role="alert" className={styles.error}>
          {missing}
        </p>
      )}
      {saved && removing !== null && (
        <RemoveSavedDestination
          saved={saved}
          index={removing}
          connectorId={fields[removing]?.connectorId ?? ""}
          onClose={() => setRemoving(null)}
          onRemove={() => {
            remove(removing);
            setRemoving(null);
          }}
        />
      )}
    </FieldSection>
  );
}

/** The fields of a message channel: source, source filters, destinations, retention. */
export function MessageFields({
  endpoints,
  saved,
}: {
  endpoints: readonly EndpointOption[];
  saved: SavedDestinations | null;
}) {
  return (
    <>
      <FieldSection title="Source">
        <FieldGrid>
          <ConnectorFields prefix="source" endpoints={endpoints} side="source" />
          <FormCheckbox
            name="source.duplicateDetection"
            label="Detect duplicate messages"
            description="A repeat of a message (same source identity and content) is recorded and shown, not processed twice."
          />
        </FieldGrid>
      </FieldSection>
      <FieldSection title="Source filters">
        <FilterEditor
          name="filters"
          owner="Source"
          description="A message continues only when all of these match. With no filters, every message continues."
        />
      </FieldSection>
      <Destinations endpoints={endpoints} saved={saved} />
      <FieldSection title="Retention">
        <RetentionFields />
      </FieldSection>
    </>
  );
}
