// The audit log (docs/spec/audit.md) and backups (docs/spec/backup.md) as the pages see them.
import type {
  AuditAction,
  AuditExportFormat,
  AuditObjectType,
  AuditOrigin,
  AuditOutcome,
  AuditProblemKind,
} from "./core";

/** One recorded event. `details` can hold operator text (notes, searched identifiers). */
export interface AuditEvent {
  seq: number;
  at: string;
  /** The OS account behind the connection; `system` for the engine itself. */
  actor: string;
  /** Unix uid or Windows SID; empty for the engine. */
  actorId: string;
  origin: AuditOrigin;
  action: AuditAction;
  objectType: AuditObjectType | null;
  objectId: string | null;
  outcome: AuditOutcome;
  details: Record<string, unknown>;
  hash: string;
  prevHash: string;
}

/** Every field narrows the result; unset fields do not. */
export interface AuditFilter {
  /** RFC 3339, inclusive. */
  since?: string;
  /** RFC 3339, exclusive. */
  until?: string;
  /** The account name or uid/SID, exactly. */
  actor?: string;
  action?: AuditAction;
  origin?: AuditOrigin;
  outcome?: AuditOutcome;
  objectType?: AuditObjectType;
  /** Exactly. */
  objectId?: string;
}

export interface AuditQuery {
  filter: AuditFilter;
  /** The previous page's `nextBeforeSeq`; null for the newest events. */
  beforeSeq: number | null;
  /** 1 to 1000. */
  limit: number;
}

/** Newest first. */
export interface AuditPage {
  events: AuditEvent[];
  /** Pass as `beforeSeq` for the next page; null on the last. */
  nextBeforeSeq: number | null;
}

export interface AuditExportRequest {
  format: AuditExportFormat;
  filter: AuditFilter;
}

/** Where an export went; `exportAudit` resolves with null when the operator cancelled. */
export interface AuditExportResult {
  /** The file the operator chose, or the browser's download name in the demo. */
  savedTo: string;
  rows: number;
}

interface AuditProblem {
  file: string;
  seq: number | null;
  kind: AuditProblemKind;
  message: string;
}

export interface AuditVerification {
  ok: boolean;
  files: number;
  events: number;
  /** The oldest event kept; above 1 when retention removed older files (recorded as `audit_prune`). */
  firstSeq: number | null;
  lastSeq: number | null;
  headHash: string | null;
  problems: AuditProblem[];
}

/** What a backup holds (the manifest's summary). */
export interface BackupSummary {
  directory: string;
  createdAt: string;
  files: number;
  bytes: number;
  /** Messages in the store snapshot; null when the state had no store. */
  messages: number | null;
  configVersion: number | null;
  auditHeadSeq: number | null;
  durationMs: number;
}

export interface BackupAttempt {
  at: string;
  directory: string;
  ok: boolean;
  error: string | null;
  summary: BackupSummary | null;
}

export interface BackupSchedule {
  /** Local time, `HH:MM`. */
  dailyAt: string;
  keep: number;
  nextAt: string | null;
}

export interface BackupStatus {
  /** Where scheduled backups go (`backup.directory`); null when none is set. */
  directory: string | null;
  schedule: BackupSchedule | null;
  /** `backup.stale_after_hours`; null when staleness is not checked. */
  staleAfterHours: number | null;
  lastAttempt: BackupAttempt | null;
  lastSuccess: BackupAttempt | null;
  stale: boolean;
}
