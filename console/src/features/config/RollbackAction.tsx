import { useState } from "react";
import { useEngineMutation, useEngineQuery } from "@/api/hooks";
import type { ConfigDocument, ValidationResult } from "@/api/types";
import { Alert } from "@/components/Alert";
import { Button } from "@/components/Button";
import { Dialog } from "@/components/Dialog";
import { ErrorState } from "@/components/ErrorState";
import { LoadingState } from "@/components/LoadingState";
import { useToast } from "@/components/Toast";
import styles from "@/styles/layout.module.css";
import { CONFIG_QUERIES } from "./applyDocument";
import { ChangeNoteField, ROLLBACK_REASON_MAX, useChangeNote } from "./changeNote";
import { removedDestinations } from "./configDocument";
import { compareDocuments } from "./documentCompare";
import { channelNamer, describeApply } from "./outcomes";
import { PlanSummary, removesChannels } from "./PlanSummary";
import { describeComparison } from "./VersionDiff";

interface RollbackActionProps {
  version: number;
  current: ConfigDocument;
  target: ConfigDocument;
}

/** What the engine would do, from its validation: the plan, or why it would refuse. */
function Preview({ result }: { result: ValidationResult }) {
  const errors = result.issues.filter((issue) => issue.severity === "error");
  if (errors.length > 0) {
    return (
      <Alert tone="danger" title="The engine would refuse this rollback" compact>
        <ul className={styles.list}>
          {errors.map((issue) => (
            <li key={`${issue.path}|${issue.message}`}>{issue.message}</li>
          ))}
        </ul>
      </Alert>
    );
  }
  return <PlanSummary plan={result.plan} />;
}

/**
 * "Roll back to this version…": a confirmation that shows the engine's plan for the rollback
 * (what restarts, updates in place, starts, stops), then the rollback. It cannot be confirmed
 * until the plan is shown, so nobody rolls back blind.
 */
export function RollbackAction({ version, current, target }: RollbackActionProps) {
  const [isOpen, setOpen] = useState(false);
  const toast = useToast();
  const reason = useChangeNote(ROLLBACK_REASON_MAX);
  const preview = useEngineQuery(
    ["previewRollback", version, current.version],
    (api) => api.previewRollback(version),
    { enabled: isOpen, staleTime: 0 },
  );
  const rollback = useEngineMutation(
    (api, input: { version: number; reason: string }) =>
      api.rollbackConfig(input.version, input.reason),
    { invalidates: CONFIG_QUERIES },
  );
  const next = current.version + 1;
  const changes = describeComparison(compareDocuments(current, target));
  const plan = preview.data?.plan;
  const refused = preview.data?.ok === false;
  const waiting = preview.isFetching;
  const ready = !waiting && !preview.isError && preview.data !== undefined && !refused;
  const closesDeliveries =
    (plan !== undefined && removesChannels(plan)) ||
    removedDestinations(current, target).length > 0;
  function close() {
    setOpen(false);
    rollback.reset();
    reason.reset();
  }
  function confirm() {
    if (!ready || !reason.check()) return;
    rollback.mutate(
      { version, reason: reason.note.trim() },
      {
        onSuccess: (result) => {
          close();
          toast.show(
            describeApply(result, channelNamer(current.channels, target.channels), {
              done: `Rolled back to version ${version}`,
              partial: `Rolled back to version ${version}, but not everywhere`,
              lead: `Saved as version ${result.version}.`,
            }),
          );
        },
      },
    );
  }
  return (
    <>
      <Button onPress={() => setOpen(true)}>Roll back to this version…</Button>
      <Dialog
        title={`Roll back to version ${version}?`}
        isOpen={isOpen}
        onOpenChange={(open) => {
          if (!open) close();
        }}
        role="alertdialog"
        isDismissable={false}
        isKeyboardDismissDisabled={rollback.isPending}
        onSubmit={() => {
          if (!rollback.isPending) confirm();
        }}
        actions={
          <>
            <Button autoFocus isDisabled={rollback.isPending} onPress={close}>
              Cancel
            </Button>
            <Button
              type="submit"
              variant="primary"
              isDisabled={!ready}
              isPending={rollback.isPending}
            >
              {`Roll back to version ${version}`}
            </Button>
          </>
        }
      >
        <div className={styles.stack}>
          <p>
            Creates a new version, {next}, with the configuration of version {version}. History is
            kept: version {current.version} stays available.
          </p>
          {changes.length > 0 ? (
            <>
              <p>The comparison above lists what changes:</p>
              <ul className={styles.list}>
                {changes.map((line) => (
                  <li key={line}>{line}</li>
                ))}
              </ul>
            </>
          ) : (
            <p>The configuration is the same as the current one.</p>
          )}
          <h3 className={styles.subheading}>What the engine will do</h3>
          {waiting && <LoadingState label="Asking the engine what rolling back would do…" />}
          {!waiting && preview.isError && (
            <ErrorState
              title="The engine's plan could not be loaded, so Roll back is off"
              error={preview.error}
              onRetry={() => void preview.refetch()}
            />
          )}
          {!waiting && !preview.isError && preview.data && <Preview result={preview.data} />}
          {closesDeliveries && (
            <p>
              Unresolved deliveries of a channel or destination this removes are closed: queued,
              rejected, failed, timed out and outcome unknown ones become cancelled.
            </p>
          )}
          <ChangeNoteField
            state={reason}
            label="Reason for the rollback"
            description={`Why you are rolling back, 1 to ${ROLLBACK_REASON_MAX} characters. It is recorded as "rollback of ${version}: …" with the new version and in the audit log.`}
          />
          {rollback.error && (
            <ErrorState title="The rollback did not happen" error={rollback.error} />
          )}
        </div>
      </Dialog>
    </>
  );
}
