import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useEngineMutation } from "@/api/hooks";
import type { BackupSummary, EngineError } from "@/api/types";
import { Alert } from "@/components/Alert";
import { Button } from "@/components/Button";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { ErrorState } from "@/components/ErrorState";
import { Facts } from "@/components/Facts";
import { LoadingState } from "@/components/LoadingState";
import { Section } from "@/components/Section";
import { Timestamp } from "@/components/Timestamp";
import { useToast } from "@/components/Toast";
import { formatBytes, formatCount, formatDuration } from "@/lib/format";
import styles from "@/styles/layout.module.css";

const AUDIT_REFUSAL =
  "Not backed up: the audit log cannot be written, and backups are audited. Fix the audit log first (see Alerts), then try again.";

/** The refusal when the audit log is down is told in words; any other failure in the engine's. */
function failure(error: EngineError): { title: string; error: EngineError } {
  if (error.code === "unavailable") {
    return { title: "The backup was not taken", error: { ...error, message: AUDIT_REFUSAL } };
  }
  return { title: "The backup failed", error };
}

function durationText(ms: number): string {
  if (ms < 1000) return `${ms} ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)} s`;
  return formatDuration(Math.round(ms / 1000));
}

function BackupResult({ summary }: { summary: BackupSummary }) {
  return (
    <section aria-label="Backup result" className={styles.stack}>
      <Alert tone="success" title="Backup complete">
        A consistent copy was written to a new directory. The status above is up to date.
      </Alert>
      <Facts
        items={[
          { label: "Directory", value: <span className={styles.code}>{summary.directory}</span> },
          { label: "Taken", value: <Timestamp value={summary.createdAt} seconds /> },
          { label: "Files", value: formatCount(summary.files) },
          { label: "Size", value: formatBytes(summary.bytes) },
          {
            label: "Messages",
            value:
              summary.messages === null ? "None: no message store" : formatCount(summary.messages),
          },
          {
            label: "Configuration version",
            value: summary.configVersion === null ? "None" : String(summary.configVersion),
          },
          {
            label: "Audit head sequence",
            value: summary.auditHeadSeq === null ? "No audit log" : String(summary.auditHeadSeq),
          },
          { label: "Duration", value: durationText(summary.durationMs) },
        ]}
      />
    </section>
  );
}

/** Takes a backup now, after saying what that does; no progress stream, no cancel. */
export function BackupNowSection() {
  const toast = useToast();
  const [confirming, setConfirming] = useState(false);
  const client = useQueryClient();
  const backup = useEngineMutation((api) => api.createBackup(), {
    invalidates: ["getBackupStatus", "getAuditEvents"],
  });
  function run() {
    backup.mutate(undefined, {
      // A failed attempt is in the status too (docs/spec/backup.md section 2).
      onError: () => void client.invalidateQueries({ queryKey: ["getBackupStatus"] }),
      onSuccess: (summary) =>
        toast.show({
          tone: "success",
          title: "Backup complete",
          description: `${formatCount(summary.files)} files, ${formatBytes(summary.bytes)}`,
        }),
    });
  }
  const failed = backup.error === null ? null : failure(backup.error);
  return (
    <Section
      title="Back up now"
      actions={
        <Button variant="primary" isPending={backup.isPending} onPress={() => setConfirming(true)}>
          Back up now
        </Button>
      }
    >
      <p className={styles.hint}>
        Takes a backup outside the schedule, into a new directory under the backup directory.
        Message traffic is not interrupted.
      </p>
      {backup.isPending && (
        <>
          <LoadingState label="Backing up…" />
          <p className={styles.hint}>A backup that has started cannot be cancelled.</p>
        </>
      )}
      {failed && (
        <ErrorState title={failed.title} error={failed.error} onRetry={run} isRetrying={false} />
      )}
      {backup.data && !backup.isPending && <BackupResult summary={backup.data} />}
      <ConfirmDialog
        title="Back up now?"
        isOpen={confirming}
        onOpenChange={setConfirming}
        confirmLabel="Start backup"
        variant="primary"
        onConfirm={() => {
          setConfirming(false);
          run();
        }}
      >
        <div className={styles.stack}>
          <p>
            A consistent copy of the message store, the audit log and the configuration is written
            into a new directory under the backup directory. Message traffic is not interrupted. The
            backup is recorded in the audit log.
          </p>
          <Alert tone="info" compact>
            The copy holds message content (PHI), so the backup directory belongs on an encrypted
            volume. Secret values are never included.
          </Alert>
        </div>
      </ConfirmDialog>
    </Section>
  );
}
