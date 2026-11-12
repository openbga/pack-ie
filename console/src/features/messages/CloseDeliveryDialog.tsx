import { useForm } from "react-hook-form";
import { useEngineMutation } from "@/api/hooks";
import type { Delivery } from "@/api/types";
import { Alert } from "@/components/Alert";
import { FormDialog } from "@/components/FormDialog";
import { TextInput } from "@/components/FormFields";
import { useToast } from "@/components/Toast";
import { ackEvidence, isUncertain } from "./messageWording";
import { INVALIDATES } from "./ResendDialog";

/** The most characters a closing reason may have (message-channels.md §9). */
const REASON_MAX = 500;

interface CloseValues {
  reason: string;
}

interface CloseDeliveryDialogProps {
  delivery: Pick<Delivery, "id" | "destinationLabel" | "state" | "ackLevel" | "attemptLog">;
  onClose: () => void;
}

/**
 * What closing does to a delivery, as true as it is known. Closing never contacts the
 * destination. For a delivery that failed, was rejected or never went out, the message was not
 * delivered. For one that timed out or whose outcome is unknown the earlier attempt may have
 * reached the destination, so it says what acknowledgement there was and never "not delivered".
 */
function Consequence({ delivery }: { delivery: CloseDeliveryDialogProps["delivery"] }) {
  const { destinationLabel } = delivery;
  if (isUncertain(delivery.state)) {
    return (
      <p>
        Closing stops further attempts and does not contact {destinationLabel}. The earlier attempt
        may have reached it: {ackEvidence(delivery)}. The delivery leaves Needs attention and the
        reason is kept with it.
      </p>
    );
  }
  return (
    <p>
      No more attempts are made and the delivery leaves Needs attention. The message is not
      delivered to {destinationLabel}. The reason is kept with the delivery.
    </p>
  );
}

/**
 * Closes one delivery that needs attention, with the operator's reason (MSG-04, spec §5). It
 * says what closing does and does not do (see `Consequence`).
 */
export function CloseDeliveryDialog({ delivery, onClose }: CloseDeliveryDialogProps) {
  const toast = useToast();
  const form = useForm<CloseValues>({ defaultValues: { reason: "" } });
  const close = useEngineMutation(
    (api, input: { deliveryId: string; reason: string }) =>
      api.closeDelivery(input.deliveryId, input.reason),
    { invalidates: INVALIDATES },
  );
  const length = [...form.watch("reason").trim()].length;
  return (
    <FormDialog
      title={`Close delivery to ${delivery.destinationLabel}`}
      description="Stops this delivery from needing attention."
      submitLabel="Close delivery"
      form={form}
      onSubmit={(event) =>
        void form.handleSubmit(({ reason }) =>
          close.mutate(
            { deliveryId: delivery.id, reason: reason.trim() },
            {
              onSuccess: () => {
                toast.show({
                  tone: "success",
                  title: `Delivery to ${delivery.destinationLabel} closed`,
                  description: "The destination was not contacted.",
                });
                onClose();
              },
            },
          ),
        )(event)
      }
      command={{ error: close.error, isPending: close.isPending }}
      errorTitle="The delivery was not closed"
      onClose={onClose}
    >
      <Alert tone="info" compact>
        <p>Closing does not contact the destination.</p>
        <Consequence delivery={delivery} />
      </Alert>
      <TextInput
        name="reason"
        label="Reason"
        isRequired
        description={`${length} of ${REASON_MAX} characters`}
        validate={(value) =>
          [...value].length <= REASON_MAX || `Use ${REASON_MAX} characters or fewer.`
        }
      />
    </FormDialog>
  );
}
