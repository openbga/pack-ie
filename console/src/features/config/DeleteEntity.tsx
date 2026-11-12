import { useState } from "react";
import { Link } from "react-aria-components";
import type { ApplyResult, ConfigDocument } from "@/api/types";
import { Button } from "@/components/Button";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { Dialog } from "@/components/Dialog";
import { ErrorState } from "@/components/ErrorState";
import { useToast } from "@/components/Toast";
import styles from "@/styles/layout.module.css";
import { appliedToast, useApplyDocument } from "./applyDocument";
import { ChangeNoteField, useChangeNote } from "./changeNote";
import type { EndpointReference } from "./configDocument";
import { DeliveriesClosed, useChannelClosed } from "./UnresolvedDeliveries";

/** The record being edited, as the delete action needs it. */
export interface EditedEntity {
  id: string;
  label: string;
  /** Channels that use it; an endpoint with any cannot be deleted. Empty for channels. */
  references: readonly EndpointReference[];
  /** The configuration without this record. */
  without: ConfigDocument;
}

interface DeleteEntityProps {
  noun: "endpoint" | "channel";
  /** The configuration as loaded, whose version the delete is applied on. */
  document: ConfigDocument;
  entity: EditedEntity;
  onDeleted: () => void;
}

function InUseDialog({
  noun,
  entity,
  onClose,
}: {
  noun: string;
  entity: EditedEntity;
  onClose: () => void;
}) {
  const channels = [...new Map(entity.references.map((each) => [each.channelId, each])).values()];
  return (
    <Dialog
      title={`${entity.label} is in use`}
      isOpen
      onOpenChange={(isOpen) => {
        if (!isOpen) onClose();
      }}
      actions={<Button onPress={onClose}>Close</Button>}
    >
      <div className={styles.stack}>
        <p>
          This {noun} cannot be deleted while channels use it. Remove it from these channels first,
          or delete those channels:
        </p>
        <ul className={styles.list}>
          {channels.map((reference) => (
            <li key={reference.channelId}>
              <Link href={`/channels/${reference.channelId}/edit`}>{reference.channelLabel}</Link> (
              {entity.references
                .filter((each) => each.channelId === reference.channelId)
                .map((each) => each.role)
                .join(", ")}
              )
            </li>
          ))}
        </ul>
      </div>
    </Dialog>
  );
}

/** "Delete endpoint…" or "Delete channel…": explains the consequences, then applies. */
export function DeleteEntity({ noun, document, entity, onDeleted }: DeleteEntityProps) {
  const [isOpen, setOpen] = useState(false);
  const toast = useToast();
  const remove = useApplyDocument();
  const changeNote = useChangeNote();
  const closed = useChannelClosed(entity.id);
  const next = document.version + 1;
  function close() {
    setOpen(false);
    remove.reset();
    changeNote.reset();
  }
  function confirm() {
    if (!changeNote.check()) return;
    remove.mutate(
      { document: entity.without, note: changeNote.note },
      {
        onSuccess: (result: ApplyResult) => {
          toast.show(appliedToast(result, entity.without.channels, document.channels));
          onDeleted();
        },
      },
    );
  }
  return (
    <>
      <Button variant="danger" onPress={() => setOpen(true)}>
        Delete {noun}…
      </Button>
      {isOpen && entity.references.length > 0 && (
        <InUseDialog noun={noun} entity={entity} onClose={close} />
      )}
      {entity.references.length === 0 && (
        <ConfirmDialog
          title={`Delete ${noun} ${entity.label}?`}
          isOpen={isOpen}
          onOpenChange={(open) => {
            if (!open) close();
          }}
          confirmLabel={`Delete ${noun}`}
          isPending={remove.isPending}
          onConfirm={confirm}
        >
          <div className={styles.stack}>
            <p>
              Removes this {noun} from the configuration as version {next}.{" "}
              {noun === "channel"
                ? "The channel stops; other channels keep running."
                : "No channel uses it, so no channel is restarted."}
            </p>
            {noun === "channel" && <DeliveriesClosed noun="channel" counts={closed} />}
            <p className={styles.hint}>
              The current version stays in the history; Settings › Versions can restore it.
            </p>
            <ChangeNoteField state={changeNote} />
            {remove.error && (
              <ErrorState title={`The ${noun} was not deleted`} error={remove.error} />
            )}
          </div>
        </ConfirmDialog>
      )}
    </>
  );
}
