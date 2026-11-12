import type {
  AuditAction,
  AuditEvent,
  AuditExportFormat,
  AuditFilter,
  AuditObjectType,
  AuditOrigin,
  AuditOutcome,
  AuditPage,
  AuditQuery,
  AuditVerification,
  ErrorCode,
  FieldError,
} from "../types";
import { type Actor, seedInputs } from "./auditSeed";
import { hashString, Rng } from "./prng";
import { DAY, HOUR, iso } from "./util";

/** The OS account the mock console runs as: what `Hello` reports and what its actions are filed under. */
export const MOCK_ACTOR = { name: "LAB\\jdoe", uid: "S-1-5-21-1001" } as const;

const COLLEAGUE = { name: "LAB\\mlopez", uid: "S-1-5-21-1002" } as const;
const ENGINE = { name: "system", uid: "" } as const;
const ZEROS = "0".repeat(64);

/** What recording an event needs; the rest is filled in from the actor and the clock. */
export interface AuditInput {
  action: AuditAction;
  outcome?: AuditOutcome;
  objectType?: AuditObjectType | null;
  objectId?: string | null;
  details?: Record<string, unknown>;
}

type Fail = (code: ErrorCode, message: string, fieldErrors?: FieldError[]) => never;

/** A 64-character stand-in for SHA-256: stable, and different for any change of input. */
function fakeHash(text: string): string {
  let out = "";
  for (let round = 0; out.length < 64; round++) {
    out += hashString(`${round}:${text}`).toString(16).padStart(8, "0");
  }
  return out.slice(0, 64);
}

function csvCell(value: string | number | null): string {
  if (value === null) return "";
  const text = String(value);
  return /[",\n]/.test(text) ? `"${text.replaceAll('"', '""')}"` : text;
}

const CSV_COLUMNS = [
  "seq",
  "at",
  "actor_uid",
  "actor_name",
  "origin",
  "action",
  "object_type",
  "object_id",
  "outcome",
  "details",
  "prev_hash",
  "hash",
] as const;

function matches(event: AuditEvent, filter: AuditFilter): boolean {
  return (
    (filter.since === undefined || event.at >= filter.since) &&
    (filter.until === undefined || event.at < filter.until) &&
    (filter.actor === undefined ||
      event.actor === filter.actor ||
      event.actorId === filter.actor) &&
    (filter.action === undefined || event.action === filter.action) &&
    (filter.origin === undefined || event.origin === filter.origin) &&
    (filter.outcome === undefined || event.outcome === filter.outcome) &&
    (filter.objectType === undefined || event.objectType === filter.objectType) &&
    (filter.objectId === undefined || event.objectId === filter.objectId)
  );
}

/** Commands are recorded twice: before they run and with their outcome. */
const COMMANDS = new Set<AuditAction>([
  "config_apply",
  "config_rollback",
  "config_verify",
  "channel_pause",
  "channel_resume",
  "channel_restart",
  "message_resend",
  "delivery_close",
  "secret_set",
  "secret_delete",
  "backup_create",
]);

/**
 * The mock engine's audit log (docs/spec/audit.md): seeded with weeks of activity, appended to by
 * what the mock console does, fail-closed when it is made unavailable, with a chain that can be
 * broken or pruned for the journeys.
 */
export class MockAudit {
  /** Oldest first. */
  private events: AuditEvent[] = [];
  private writable = true;
  private brokenAt: number | null = null;

  constructor(
    seed: number,
    private readonly now: () => number,
    private readonly fail: Fail,
  ) {
    const rng = new Rng(seed ^ 0xa0d17);
    const seeded = seedInputs(rng, [MOCK_ACTOR, COLLEAGUE]);
    const start = now() - 45 * DAY;
    this.append(
      ENGINE,
      "engine",
      { action: "engine_start", objectType: "engine", details: { version: "0.1.0" } },
      start,
    );
    const span = 45 * DAY - 3 * HOUR;
    seeded.forEach(({ who, origin, input }, index) => {
      const at = start + Math.floor(((index + 1) / (seeded.length + 1)) * span);
      this.recordSeeded(who, origin, input, at);
    });
    this.append(
      ENGINE,
      "engine",
      { action: "audit_anchor", objectType: "audit", details: { reason: "daily" } },
      now() - 2 * HOUR,
    );
  }

  private recordSeeded(who: Actor, origin: AuditOrigin, input: AuditInput, at: number) {
    if (COMMANDS.has(input.action)) {
      this.append(who, origin, { ...input, outcome: "requested" }, at);
      this.append(who, origin, { ...input, outcome: "succeeded" }, at + 400);
    } else {
      this.append(who, origin, { ...input, outcome: "succeeded" }, at);
    }
  }

  private append(who: Actor, origin: AuditOrigin, input: AuditInput, atMs: number): AuditEvent {
    const last = this.events.at(-1);
    const seq = (last?.seq ?? 0) + 1;
    const prevHash = last?.hash ?? ZEROS;
    const event: AuditEvent = {
      seq,
      at: iso(atMs),
      actor: who.name,
      actorId: who.uid,
      origin,
      action: input.action,
      objectType: input.objectType ?? null,
      objectId: input.objectId ?? null,
      outcome: input.outcome ?? "succeeded",
      details: input.details ?? {},
      prevHash,
      hash: "",
    };
    event.hash = fakeHash(
      `${prevHash}|${seq}|${event.at}|${who.name}|${input.action}|${event.objectId}`,
    );
    this.events.push(event);
    return event;
  }

  // ── Recording what the console does ─────────────────────────────────────────────────

  /** The newest event's `seq`. */
  get headSeq(): number {
    return this.events.at(-1)?.seq ?? 0;
  }

  get available(): boolean {
    return this.writable;
  }

  /** The journeys' switch: while off, audited commands and content views are refused. */
  setAvailable(available: boolean): void {
    this.writable = available;
  }

  /**
   * Records an action of the mock console. Refuses with `unavailable` while the log cannot be
   * written, so the command or content view does nothing (fail closed, audit.md section 7).
   */
  record(input: AuditInput): AuditEvent {
    if (!this.writable) this.fail("unavailable", "The audit log cannot be written.");
    return this.append(MOCK_ACTOR, "console", input, this.now());
  }

  /**
   * Runs an audited command: `requested` first, then its outcome. A command that throws is
   * recorded `rejected`, a precondition failing as the engine does.
   */
  command<T>(input: AuditInput, run: () => T): T {
    this.record({ ...input, outcome: "requested" });
    try {
      const result = run();
      if (this.writable)
        this.append(MOCK_ACTOR, "console", { ...input, outcome: "succeeded" }, this.now());
      return result;
    } catch (error) {
      if (this.writable)
        this.append(MOCK_ACTOR, "console", { ...input, outcome: "rejected" }, this.now());
      throw error;
    }
  }

  // ── Review ──────────────────────────────────────────────────────────────────────────

  /** Listing is not itself recorded (audit.md section 3). */
  query({ filter, beforeSeq, limit }: AuditQuery): AuditPage {
    const found: AuditEvent[] = [];
    let next: number | null = null;
    for (let i = this.events.length - 1; i >= 0; i--) {
      const event = this.events[i] as AuditEvent;
      if (beforeSeq !== null && event.seq >= beforeSeq) continue;
      if (!matches(event, filter)) continue;
      if (found.length === limit) {
        next = (found.at(-1) as AuditEvent).seq;
        break;
      }
      found.push(event);
    }
    return { events: found.map((event) => ({ ...event })), nextBeforeSeq: next };
  }

  /** Every matching event, oldest first, as the export file's text. */
  exportText(filter: AuditFilter, format: AuditExportFormat): { content: string; rows: number } {
    this.record({ action: "audit_export", objectType: "audit", details: { format } });
    const rows = this.events.filter((event) => matches(event, filter));
    const lines =
      format === "csv"
        ? [
            CSV_COLUMNS.join(","),
            ...rows.map((event) =>
              [
                event.seq,
                event.at,
                event.actorId,
                event.actor,
                event.origin,
                event.action,
                event.objectType,
                event.objectId,
                event.outcome,
                JSON.stringify(event.details),
                event.prevHash,
                event.hash,
              ]
                .map(csvCell)
                .join(","),
            ),
          ]
        : rows.map((event) => JSON.stringify(event));
    return { content: `${lines.join("\n")}\n`, rows: rows.length };
  }

  verify(): AuditVerification {
    this.record({ action: "audit_verify", objectType: "audit" });
    const first = this.events[0];
    const last = this.events.at(-1);
    const broken = this.brokenAt;
    return {
      ok: broken === null,
      files: 2,
      events: this.events.length,
      firstSeq: first?.seq ?? null,
      lastSeq: last?.seq ?? null,
      headHash: last?.hash ?? null,
      problems:
        broken === null
          ? []
          : [
              {
                file: "audit.sqlite3",
                seq: broken,
                kind: "hash_mismatch",
                message: `The stored hash of event ${broken} does not match its content.`,
              },
            ],
    };
  }

  // ── Journey hooks ───────────────────────────────────────────────────────────────────

  /** Makes verification report an edited event at `seq`. */
  breakChain(seq: number): void {
    this.brokenAt = seq;
  }

  /** Retention removed the oldest `count` events; the chain starts later, explained by `audit_prune`. */
  prune(count: number): void {
    const removed = this.events.splice(0, count);
    const lastRemoved = removed.at(-1);
    if (lastRemoved === undefined) return;
    this.append(
      ENGINE,
      "engine",
      {
        action: "audit_prune",
        objectType: "audit",
        details: { last_seq: lastRemoved.seq, last_hash: lastRemoved.hash, retention_days: 2190 },
      },
      this.now(),
    );
  }
}
