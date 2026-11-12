import { useState } from "react";
import { useEngineMutation } from "@/api/hooks";
import { Button } from "@/components/Button";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { ErrorState } from "@/components/ErrorState";
import { useToast } from "@/components/Toast";
import styles from "@/styles/layout.module.css";
import { CONFIG_QUERIES } from "./applyDocument";
import { ChangeNoteField, useChangeNote } from "./changeNote";

const BLANK_REFERENCE = "Enter a verification reference, such as a validation record number.";

/**
 * "Mark as verified…": signs a version off after its change, with the operator's reference
 * (configuration.md section 10). What verified means is the laboratory's own procedure; the
 * engine records the account, the time and the reference, and a version can be signed off more
 * than once.
 */
export function VerifyVersion({ version }: { version: number }) {
  const [isOpen, setOpen] = useState(false);
  const toast = useToast();
  const reference = useChangeNote(undefined, BLANK_REFERENCE);
  const verify = useEngineMutation(
    (api, input: { version: number; note: string }) =>
      api.verifyConfigVersion(input.version, input.note),
    { invalidates: CONFIG_QUERIES },
  );
  function close() {
    setOpen(false);
    verify.reset();
    reference.reset();
  }
  function confirm() {
    if (!reference.check()) return;
    verify.mutate(
      { version, note: reference.note.trim() },
      {
        onSuccess: () => {
          close();
          toast.show({ tone: "success", title: `Version ${version} marked as verified` });
        },
      },
    );
  }
  return (
    <>
      <Button onPress={() => setOpen(true)}>Mark as verified…</Button>
      <ConfirmDialog
        title={`Mark version ${version} as verified?`}
        isOpen={isOpen}
        onOpenChange={(open) => {
          if (!open) close();
        }}
        confirmLabel={`Mark version ${version} as verified`}
        variant="primary"
        isPending={verify.isPending}
        onConfirm={confirm}
      >
        <div className={styles.stack}>
          <p>
            Signs this version off as working as intended after the change. The engine records your
            account, the time and your reference with the version and in the audit log. Nothing
            about the running configuration changes.
          </p>
          <ChangeNoteField
            state={reference}
            label="Verification reference"
            description="Your reference, such as a validation record number, 1 to 500 characters."
          />
          {verify.error && (
            <ErrorState title="The version was not marked as verified" error={verify.error} />
          )}
        </div>
      </ConfirmDialog>
    </>
  );
}
