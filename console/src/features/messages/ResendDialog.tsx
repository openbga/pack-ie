import { useState } from "react";
import { FormProvider, useForm } from "react-hook-form";
import { useEngineMutation, useEngineQuery } from "@/api/hooks";
import type { ResendMode, ResendResult } from "@/api/types";
import { Button } from "@/components/Button";
import { Dialog } from "@/components/Dialog";
import { Form } from "@/components/Form";
import { useToast } from "@/components/Toast";
import { ResendChoices, type ResendValues } from "./ResendChoices";
import { outcomeToast, ResendResultView } from "./ResendResult";
import {
  defaultDestinations,
  needsRiskConfirmation,
  needsSupersededConfirmation,
  offeredModes,
  type ResendSubject,
} from "./resendRules";

/** Fetches again what a resend changed: the record and the list. */
export const INVALIDATES = ["getMessage", "listMessages"] as const;

interface ResendDialogProps {
  subject: ResendSubject;
  /** The destinations to start with; by default those whose delivery needs attention. */
  initialDestinations?: string[];
  onClose: () => void;
}

/**
 * Resend a message (spec §6, message-channels.md §9): the destinations (those needing
 * attention start selected), the mode with what it does, and the duplicate-risk confirmation
 * when a selected delivery may already have been processed. The engine judges each destination
 * again; its answer is listed per destination, with its own words for a refusal. A message that a
 * newer revision supersedes is resent only after the operator confirms it (compliance P10): the
 * dialog learns of the revision from the record, and from the engine's refusal when one arrived
 * since.
 */
export function ResendDialog({ subject, initialDestinations, onClose }: ResendDialogProps) {
  const toast = useToast();
  const modes = offeredModes(subject);
  const hasDeliveries = subject.destinations.length > 0;
  const form = useForm<ResendValues>({
    defaultValues: {
      destinations: initialDestinations ?? defaultDestinations(subject),
      mode: modes[0] ?? "new_message",
      confirm: false,
      confirmSuperseded: false,
      reason: "",
    },
  });
  const [answer, setAnswer] = useState<{ mode: ResendMode; result: ResendResult } | null>(null);
  const resend = useEngineMutation(
    (api, request: Parameters<typeof api.resendMessage>[0]) => api.resendMessage(request),
    { invalidates: INVALIDATES },
  );
  // Facts only: the record is read, not audited. The detail page has read it already.
  const record = useEngineQuery(["getMessage", subject.messageId], (api) =>
    api.getMessage(subject.messageId),
  );
  const revisedBy = record.data?.revisedBy ?? null;
  // Once the engine has refused it, the confirmation stays, whatever the record says.
  const [refused, setRefused] = useState(false);
  const superseded = needsSupersededConfirmation(revisedBy, refused);
  const selected = form.watch("destinations");
  const mode = form.watch("mode");
  const risk = hasDeliveries && needsRiskConfirmation(subject, selected);
  const supersededConfirmed = form.watch("confirmSuperseded");

  function submit(values: ResendValues) {
    const reason = values.reason.trim();
    resend.mutate(
      {
        messageId: subject.messageId,
        destinations: hasDeliveries ? values.destinations : "all",
        mode: values.mode,
        confirmDuplicateRisk: risk && values.confirm,
        ...(reason === "" ? {} : { reason }),
        ...(superseded && values.confirmSuperseded ? { confirmSuperseded: true } : {}),
      },
      {
        onSuccess: (result) => {
          setAnswer({ mode: values.mode, result });
          toast.show(outcomeToast(result));
        },
        // A revision that arrived while the dialog was open: read the record again to link it.
        onError: (error) => {
          if (error.rejection !== "superseded") return;
          setRefused(true);
          void record.refetch();
        },
      },
    );
  }

  return (
    <Dialog
      title={`Resend ${subject.identifier}`}
      description={`Message of ${subject.channelLabel}`}
      isOpen
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      isDismissable={!resend.isPending && !form.formState.isDirty}
    >
      <FormProvider {...form}>
        <Form
          aria-label={`Resend ${subject.identifier}`}
          onSubmit={(event) => void form.handleSubmit(submit)(event)}
          error={answer === null ? resend.error : null}
          errorTitle="The message was not resent"
          actions={
            answer === null ? (
              <>
                <Button
                  type="submit"
                  variant="primary"
                  isPending={resend.isPending}
                  isDisabled={superseded && !supersededConfirmed}
                >
                  {mode === "new_message" ? "Create new message" : "Resend"}
                </Button>
                <Button isDisabled={resend.isPending} onPress={onClose}>
                  Cancel
                </Button>
              </>
            ) : (
              <>
                <Button variant="primary" onPress={onClose}>
                  Done
                </Button>
                {answer.result.destinations.some((d) => d.rejected !== null) && (
                  <Button onPress={() => setAnswer(null)}>Back to the choices</Button>
                )}
              </>
            )
          }
        >
          {answer === null ? (
            <ResendChoices
              subject={subject}
              modes={modes}
              risk={risk}
              superseded={superseded ? { newerId: revisedBy } : null}
            />
          ) : (
            <ResendResultView mode={answer.mode} result={answer.result} />
          )}
        </Form>
      </FormProvider>
    </Dialog>
  );
}
