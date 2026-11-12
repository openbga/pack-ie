import type { ConfigDocument, ConfigIssue, ValidationResult } from "@/api/types";
import { Alert } from "@/components/Alert";
import { Button } from "@/components/Button";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import styles from "@/styles/layout.module.css";
import { ChangeNoteField, type useChangeNote } from "./changeNote";
import { type RemovedDestination, removedDestinations } from "./configDocument";
import { PlanSummary, removesChannels } from "./PlanSummary";
import type { StagedApply } from "./useApplyFlow";

function IssueList({
  issues,
  onFocusPath,
}: {
  issues: readonly ConfigIssue[];
  onFocusPath: (path: string) => void;
}) {
  return (
    <ul className={styles.stack}>
      {issues.map((issue) => (
        <li key={`${issue.path}|${issue.message}`}>
          <Button variant="quiet" onPress={() => onFocusPath(issue.path)}>
            {issue.message}
          </Button>{" "}
          <span className={styles.code}>
            {issue.path}
            {issue.code ? ` · ${issue.code}` : ""}
          </span>
        </li>
      ))}
    </ul>
  );
}

interface CheckSummaryProps {
  result: ValidationResult | null;
  /** Moves focus to the form field an issue is about, when the form has one. */
  onFocusPath: (path: string) => void;
}

/**
 * What the engine's validation found, by severity, each issue linked to its field. Errors
 * block applying; warnings do not.
 */
export function CheckSummary({ result, onFocusPath }: CheckSummaryProps) {
  if (result === null) return null;
  const errors = result.issues.filter((issue) => issue.severity === "error");
  const warnings = result.issues.filter((issue) => issue.severity === "warning");
  if (errors.length === 0 && warnings.length === 0) {
    return (
      <Alert tone="success" title="No problems found">
        The configuration is valid. Save and apply creates a new version.
      </Alert>
    );
  }
  return (
    <section aria-label="Validation results" className={styles.stack}>
      {errors.length > 0 && (
        <div className={styles.stack}>
          <h3 className={styles.subheading}>Errors ({errors.length})</h3>
          <p className={styles.hint}>
            These block applying, so Save and apply stays off until you fix them and Validate finds
            no errors. The running configuration stays active meanwhile.
          </p>
          <IssueList issues={errors} onFocusPath={onFocusPath} />
        </div>
      )}
      {warnings.length > 0 && (
        <Alert tone="warning" title={`Warnings (${warnings.length})`}>
          <p>Warnings do not block applying.</p>
          <IssueList issues={warnings} onFocusPath={onFocusPath} />
        </Alert>
      )}
    </section>
  );
}

interface ApplyReviewDialogProps {
  /** The configuration currently running, as the page loaded it. */
  current: ConfigDocument;
  /** What will be applied, with the engine's plan; the dialog is open while it is set. */
  staged: StagedApply | null;
  /** The reason for the change, required to apply. */
  changeNote: ReturnType<typeof useChangeNote>;
  isPending: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

/** What closing deliveries means when channels or destinations are removed. */
function RemovalNote({
  removed,
  channelsRemoved,
}: {
  removed: readonly RemovedDestination[];
  channelsRemoved: boolean;
}) {
  if (removed.length === 0 && !channelsRemoved) return null;
  return (
    <div className={styles.stack}>
      <p>
        Unresolved deliveries of a removed channel or destination are closed: queued, rejected,
        failed, timed out and outcome unknown ones become cancelled and are not delivered or
        retried.
      </p>
      {removed.length > 0 && (
        <ul className={styles.list}>
          {removed.map((each) => (
            <li key={`${each.channelId}|${each.destinationLabel}`}>
              Destination {each.destinationLabel} of {each.channelLabel} is removed.
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/** States, before applying, what will happen in operator terms. */
export function ApplyReviewDialog({
  current,
  staged,
  changeNote,
  isPending,
  onConfirm,
  onCancel,
}: ApplyReviewDialogProps) {
  const next = current.version + 1;
  const plan = staged ? staged.plan : null;
  const removed = staged ? removedDestinations(current, staged.document) : [];
  return (
    <ConfirmDialog
      title={`Apply configuration version ${next}?`}
      isOpen={staged !== null}
      onOpenChange={(isOpen) => {
        if (!isOpen) onCancel();
      }}
      confirmLabel={`Apply version ${next}`}
      variant="primary"
      isPending={isPending}
      onConfirm={onConfirm}
    >
      <div className={styles.stack}>
        <p>
          Applies configuration version {next}. Only channels that changed are restarted; every
          other channel keeps running.
        </p>
        {plan && <PlanSummary plan={plan} />}
        <RemovalNote removed={removed} channelsRemoved={plan !== null && removesChannels(plan)} />
        {staged && staged.warnings.length > 0 && (
          <Alert tone="warning" title={`Warnings (${staged.warnings.length})`} compact>
            <ul className={styles.list}>
              {staged.warnings.map((warning) => (
                <li key={`${warning.path}|${warning.message}`}>
                  {warning.message}{" "}
                  <span className={styles.code}>
                    {warning.path}
                    {warning.code ? ` · ${warning.code}` : ""}
                  </span>
                </li>
              ))}
            </ul>
          </Alert>
        )}
        <ChangeNoteField state={changeNote} />
        <p className={styles.hint}>
          If the engine rejects the configuration (a problem it finds, or a change made elsewhere),
          nothing is applied and the running configuration stays active. If a channel then cannot
          take its change, the new version is still active and that channel keeps its previous
          definition; the next apply retries it, and the result shows each channel's outcome.
          Settings › Versions can restore any earlier version.
        </p>
      </div>
    </ConfirmDialog>
  );
}
