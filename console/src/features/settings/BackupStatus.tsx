import type { UseQueryResult } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import type {
  BackupAttempt,
  BackupSchedule,
  EngineError,
  BackupStatus as Status,
} from "@/api/types";
import { Alert } from "@/components/Alert";
import { Facts } from "@/components/Facts";
import { QueryContent } from "@/components/QueryContent";
import { Section } from "@/components/Section";
import { StatusBadge } from "@/components/StatusBadge";
import { Timestamp } from "@/components/Timestamp";
import { formatAge } from "@/lib/format";
import styles from "@/styles/layout.module.css";

/** The engine's own next step for the `backup_stale` alert (docs/spec/diagnostics.md section 6). */
const STALE_NEXT_ACTION =
  "Check the backup directory (free space, permissions, mounted) and the last backup error (Settings → Backup or `packie backup status`), then run `packie backup create`. The alert clears after the next successful backup.";

/** How often the panel re-reads the status and re-words its ages. */
export const BACKUP_REFRESH_MS = 30_000;

/** The time, re-read while the panel is open so "9 h ago" does not stand still. */
function useNow(intervalMs: number): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), intervalMs);
    return () => clearInterval(timer);
  }, [intervalMs]);
  return now;
}

function overall(status: Status): { tone: "success" | "warning"; label: string } {
  if (status.lastSuccess === null) return { tone: "warning", label: "No backup yet" };
  if (status.stale) return { tone: "warning", label: "Stale" };
  return { tone: "success", label: "Up to date" };
}

function scheduleText(schedule: BackupSchedule | null) {
  if (schedule === null) return "No schedule";
  return (
    <>
      Every day at {schedule.dailyAt}, keeps the newest {schedule.keep}
      {schedule.nextAt !== null && (
        <>
          {" "}
          · next run <Timestamp value={schedule.nextAt} />
        </>
      )}
    </>
  );
}

function limitText(hours: number | null): string {
  return hours === null ? "Not checked" : `Alerts after ${hours} hours without a successful backup`;
}

function staleDescription(status: Status, now: number): string {
  const limit =
    status.staleAfterHours === null ? "" : ` The limit is ${status.staleAfterHours} hours.`;
  if (status.lastSuccess === null) {
    return `No backup has succeeded since the engine started.${limit}`;
  }
  return `The newest successful backup was ${formatAge(status.lastSuccess.at, now)}.${limit}`;
}

function sameAttempt(a: BackupAttempt, b: BackupAttempt | null): boolean {
  return b !== null && a.at === b.at && a.directory === b.directory;
}

function lastAttemptValue(attempt: BackupAttempt, now: number) {
  return (
    <>
      <Timestamp value={attempt.at} /> ({formatAge(attempt.at, now)}){" "}
      {attempt.ok ? (
        <StatusBadge tone="success">Succeeded</StatusBadge>
      ) : (
        <StatusBadge tone="danger">Failed</StatusBadge>
      )}
      {!attempt.ok && <p>{attempt.error ?? "The engine gave no reason."}</p>}
    </>
  );
}

function StatusFacts({ status, now }: { status: Status; now: number }) {
  const { tone, label } = overall(status);
  const success = status.lastSuccess;
  const attempt = status.lastAttempt;
  return (
    <div className={styles.stack}>
      {status.stale && (
        <Alert tone="warning" title="Backups are stale">
          <p>{staleDescription(status, now)}</p>
          <p>{STALE_NEXT_ACTION}</p>
        </Alert>
      )}
      <Facts
        items={[
          { label: "State", value: <StatusBadge tone={tone}>{label}</StatusBadge> },
          {
            label: "Backup directory",
            value:
              status.directory === null ? (
                <>
                  Not set
                  <p className={styles.hint}>
                    Scheduled backups need backup.directory in bootstrap.json on the lab computer.
                  </p>
                </>
              ) : (
                <span className={styles.code}>{status.directory}</span>
              ),
          },
          { label: "Schedule", value: scheduleText(status.schedule) },
          { label: "Stale limit", value: limitText(status.staleAfterHours) },
          {
            label: "Last success",
            value:
              success === null ? (
                "None yet"
              ) : (
                <>
                  <Timestamp value={success.at} /> ({formatAge(success.at, now)})
                  <p className={styles.code}>{success.directory}</p>
                </>
              ),
          },
          ...(attempt !== null && !sameAttempt(attempt, success)
            ? [{ label: "Last attempt", value: lastAttemptValue(attempt, now) }]
            : []),
        ]}
      />
    </div>
  );
}

/** What the engine reports about backups: directory, schedule, stale limit, last attempts. */
export function BackupStatusSection({ query }: { query: UseQueryResult<Status, EngineError> }) {
  const now = useNow(BACKUP_REFRESH_MS);
  return (
    <Section title="Backup status">
      <QueryContent query={query} what="backup status" loadingRows={4}>
        {(data) => <StatusFacts status={data} now={now} />}
      </QueryContent>
    </Section>
  );
}
