import { useFormContext, useWatch } from "react-hook-form";
import { FieldGrid, FieldSection } from "@/components/Form";
import { ChoiceInput, PickerInput, TextInput } from "@/components/FormFields";
import type { EndpointOption } from "./ConnectorFields";
import { RetentionFields, RetryFields } from "./PolicyFields";
import { wholeNumber } from "./validators";

const PAIRING = [
  { id: "parallel", label: "Connect both sides independently" },
  { id: "wait_for_listener_then_pair", label: "Wait for the listening side, then pair" },
  { id: "receiver_then_sender", label: "Receiver first, then sender" },
];
const TAP_FRAMERS = [
  { id: "none", label: "None (bytes only)" },
  { id: "astm", label: "ASTM" },
  { id: "au", label: "AU" },
  { id: "hl7_mllp", label: "HL7 MLLP" },
];
const TAP_RETENTION_NOTE =
  "Applies to the messages the tap records. Recorded messages are purged beyond these limits. Leave a field empty for no limit.";
const RECEIVERS = [
  { id: "a", label: "Endpoint A" },
  { id: "b", label: "Endpoint B" },
];

/** The fields of a bridge channel: its two endpoints, session policy, tap and reconnects. */
export function BridgeFields({ endpoints }: { endpoints: readonly EndpointOption[] }) {
  const { control } = useFormContext();
  const pairing = useWatch({ control, name: "pairing" });
  return (
    <>
      <FieldSection title="Endpoints">
        <FieldGrid>
          <PickerInput name="endpointAId" label="Endpoint A" options={endpoints} isRequired />
          <PickerInput
            name="endpointBId"
            label="Endpoint B"
            options={endpoints}
            isRequired
            validate={(value, values) =>
              value === null ||
              value !== values.endpointAId ||
              "Endpoint B must differ from endpoint A."
            }
          />
        </FieldGrid>
      </FieldSection>
      <FieldSection title="Session">
        <FieldGrid>
          <ChoiceInput name="pairing" label="Pairing strategy" options={PAIRING} isRequired />
          {pairing === "receiver_then_sender" && (
            <ChoiceInput
              name="pairingReceiver"
              label="Receiver"
              options={RECEIVERS}
              isRequired
              description="The side that connects and receives first; the other side sends once it is connected."
            />
          )}
          <TextInput
            name="bufferBytes"
            label="Buffer size (bytes)"
            isRequired
            validate={wholeNumber()}
            description="Per direction. When a buffer is full the bridge waits and reports pressure; it never drops data."
          />
        </FieldGrid>
      </FieldSection>
      <FieldSection title="Message tap">
        <FieldGrid>
          <ChoiceInput
            name="tapFramer"
            label="Tap framer"
            options={TAP_FRAMERS}
            isRequired
            description="Counts and records messages without changing the bytes."
          />
        </FieldGrid>
        <RetentionFields prefix="tapRetention" note={TAP_RETENTION_NOTE} />
      </FieldSection>
      <FieldSection title="Reconnect policy">
        <RetryFields
          prefix="retry"
          attempts="optional"
          description="After a connection fails the bridge waits the initial delay, then longer each time up to the maximum, with jitter."
        />
      </FieldSection>
    </>
  );
}
