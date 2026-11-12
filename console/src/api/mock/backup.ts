import type { BackupAttempt, BackupStatus, BackupSummary, ErrorCode, FieldError } from "../types";
import { DAY, HOUR, iso, MIB } from "./util";

type Fail = (code: ErrorCode, message: string, fieldErrors?: FieldError[]) => never;

const DIRECTORY = "D:\\PACK-IE\\Backups";
const STALE_AFTER_HOURS = 26;
const DAILY_AT = "02:00";
const KEEP = 14;
const BACKUP_MS = 1800;

/** What the engine reports about backups (docs/spec/backup.md): a schedule, a stale limit and the last attempts. */
export class MockBackup {
  private lastAttempt: BackupAttempt | null;
  private lastSuccess: BackupAttempt | null;
  private failNext: string | null = null;
  private counter = 1;

  constructor(
    private readonly now: () => number,
    private readonly messageCount: number,
    private readonly configVersion: () => number,
    private readonly auditHeadSeq: () => number,
    private readonly fail: Fail,
  ) {
    const at = now() - 9 * HOUR;
    const success = {
      at: iso(at),
      directory: `${DIRECTORY}\\packie-backup-${iso(at).replace(/[-:]/g, "").slice(0, 15)}Z`,
      ok: true,
      error: null,
      summary: this.summaryAt(at),
    };
    this.lastAttempt = success;
    this.lastSuccess = success;
  }

  private summaryAt(atMs: number): BackupSummary {
    const messages = this.messageCount;
    return {
      directory: `${DIRECTORY}\\packie-backup-${iso(atMs).replace(/[-:]/g, "").slice(0, 15)}Z`,
      createdAt: iso(atMs),
      files: 12 + Math.ceil(messages / 400),
      bytes: 6 * MIB + messages * 1_800,
      messages,
      configVersion: this.configVersion(),
      auditHeadSeq: this.auditHeadSeq(),
      durationMs: BACKUP_MS,
    };
  }

  /** Stale while the newest success is older than the limit, as the engine counts it. */
  private isStale(): boolean {
    const newest = this.lastSuccess;
    return newest === null || this.now() - Date.parse(newest.at) > STALE_AFTER_HOURS * HOUR;
  }

  /** Whether the `backup_stale` alert is open, and what it says. */
  staleDetail(): { newestSuccessAt: string | null; limitHours: number } | null {
    return this.isStale()
      ? { newestSuccessAt: this.lastSuccess?.at ?? null, limitHours: STALE_AFTER_HOURS }
      : null;
  }

  status(): BackupStatus {
    const nextRun = new Date(this.now());
    const [hours, minutes] = DAILY_AT.split(":").map(Number) as [number, number];
    nextRun.setHours(hours, minutes, 0, 0);
    if (nextRun.getTime() <= this.now()) nextRun.setTime(nextRun.getTime() + DAY);
    return {
      directory: DIRECTORY,
      schedule: { dailyAt: DAILY_AT, keep: KEEP, nextAt: iso(nextRun.getTime()) },
      staleAfterHours: STALE_AFTER_HOURS,
      lastAttempt: this.lastAttempt,
      lastSuccess: this.lastSuccess,
      stale: this.isStale(),
    };
  }

  /** Takes a backup; it fails when a journey asked it to, recording the error in the status. */
  create(): BackupSummary {
    const at = this.now();
    if (this.failNext !== null) {
      const error = this.failNext;
      this.failNext = null;
      this.lastAttempt = { at: iso(at), directory: DIRECTORY, ok: false, error, summary: null };
      this.fail("internal", `The backup failed: ${error}`);
    }
    const summary = {
      ...this.summaryAt(at),
      directory: `${DIRECTORY}\\packie-backup-${iso(at).replace(/[-:]/g, "").slice(0, 15)}Z-${this.counter}`,
    };
    this.counter += 1;
    const attempt = { at: iso(at), directory: summary.directory, ok: true, error: null, summary };
    this.lastAttempt = attempt;
    this.lastSuccess = attempt;
    return summary;
  }

  // ── Journey hooks ───────────────────────────────────────────────────────────────────

  /** The newest success is `hours` old. */
  ageNewestSuccess(hours: number): void {
    const at = this.now() - hours * HOUR;
    if (this.lastSuccess === null) return;
    const aged = { ...this.lastSuccess, at: iso(at) };
    this.lastSuccess = aged;
    if (this.lastAttempt?.ok) this.lastAttempt = aged;
  }

  /** The next backup fails with `error` (a full disk, say). */
  failNextBackup(error: string): void {
    this.failNext = error;
  }
}
