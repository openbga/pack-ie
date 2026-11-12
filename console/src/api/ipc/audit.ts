// The audit log and backup status (docs/spec/audit.md, backup.md): the engine's DTOs to the
// console's and the console's queries to the engine's. Pure.

import type {
  BackupAttempt as RawAttempt,
  AuditEvent as RawEvent,
  AuditExportRequest as RawExport,
  AuditFilter as RawFilter,
  AuditPage as RawPage,
  AuditQuery as RawQuery,
  BackupStatusReport as RawStatus,
  BackupSummary as RawSummary,
  AuditVerification as RawVerification,
} from "@/ipc/generated";
import type {
  AuditEvent,
  AuditExportRequest,
  AuditFilter,
  AuditPage,
  AuditQuery,
  AuditVerification,
  BackupAttempt,
  BackupStatus,
  BackupSummary,
} from "../types";

function auditFilterToIpc(filter: AuditFilter): RawFilter {
  return {
    since: filter.since ?? null,
    until: filter.until ?? null,
    actor: filter.actor ?? null,
    action: filter.action ?? null,
    origin: filter.origin ?? null,
    outcome: filter.outcome ?? null,
    object_type: filter.objectType ?? null,
    object_id: filter.objectId ?? null,
  };
}

export function auditQueryToIpc(query: AuditQuery): RawQuery {
  return {
    filter: auditFilterToIpc(query.filter),
    before_seq: query.beforeSeq,
    limit: query.limit,
  };
}

export function auditExportToIpc(
  request: AuditExportRequest,
): Omit<RawExport, "after_seq" | "export_id"> {
  return { filter: auditFilterToIpc(request.filter), format: request.format };
}

function eventFromIpc(raw: RawEvent): AuditEvent {
  return {
    seq: raw.seq,
    at: raw.at,
    actor: raw.actor_name,
    actorId: raw.actor_uid,
    origin: raw.origin,
    action: raw.action,
    objectType: raw.object_type,
    objectId: raw.object_id,
    outcome: raw.outcome,
    details: raw.details,
    hash: raw.hash,
    prevHash: raw.prev_hash,
  };
}

export function auditPageFromIpc(raw: RawPage): AuditPage {
  return { events: raw.events.map(eventFromIpc), nextBeforeSeq: raw.next_before_seq };
}

export function verificationFromIpc(raw: RawVerification): AuditVerification {
  return {
    ok: raw.ok,
    files: raw.files,
    events: raw.events,
    firstSeq: raw.first_seq,
    lastSeq: raw.last_seq,
    headHash: raw.head_hash,
    problems: raw.problems.map((problem) => ({
      file: problem.file,
      seq: problem.seq,
      kind: problem.kind,
      message: problem.message,
    })),
  };
}

export function backupSummaryFromIpc(raw: RawSummary): BackupSummary {
  return {
    directory: raw.directory,
    createdAt: raw.created_at,
    files: raw.files,
    bytes: raw.bytes,
    messages: raw.messages,
    configVersion: raw.config_version,
    auditHeadSeq: raw.audit_head_seq,
    durationMs: raw.duration_ms,
  };
}

function attemptFromIpc(raw: RawAttempt | null): BackupAttempt | null {
  if (raw === null) return null;
  return {
    at: raw.at,
    directory: raw.directory,
    ok: raw.ok,
    error: raw.error,
    summary: raw.summary === null ? null : backupSummaryFromIpc(raw.summary),
  };
}

export function backupStatusFromIpc(raw: RawStatus): BackupStatus {
  return {
    directory: raw.directory,
    schedule:
      raw.schedule === null
        ? null
        : { dailyAt: raw.schedule.daily_at, keep: raw.schedule.keep, nextAt: raw.schedule.next_at },
    staleAfterHours: raw.stale_after_hours,
    lastAttempt: attemptFromIpc(raw.last_attempt),
    lastSuccess: attemptFromIpc(raw.last_success),
    stale: raw.stale,
  };
}
