import type { Event, LogFilter, LogEvent as RawLogEvent } from "@/ipc/generated";
import type { Unsubscribe } from "../engine";
import type { LogEvent, LogFollowNotice, LogPage, LogQuery } from "../types";
import type { ConnectionPayload } from "./bridge";
import type { IpcClient } from "./client";

/** Events arriving within this many milliseconds reach `followLogs` callers together. */
const BATCH_MS = 150;

export function logEventFromIpc(event: RawLogEvent): LogEvent {
  return {
    seq: event.seq,
    at: event.at,
    level: event.level,
    channelId: event.channel_id,
    endpointId: event.endpoint_id,
    messageId: event.message_id,
    message: event.message,
    target: event.target,
    event: event.event,
    fields: event.fields,
  };
}

/**
 * The engine's filter for `query`; `before` pages back to events with a smaller `seq`, `after`
 * forward to those with a larger one.
 */
function logFilter(
  query: LogQuery,
  limit: number | null,
  before: number | null = null,
  after: number | null = null,
): LogFilter {
  return {
    min_level: query.minLevel ?? null,
    since: query.from ?? null,
    until: query.to ?? null,
    channel_id: query.channelId ?? null,
    endpoint_id: query.endpointId ?? null,
    text: query.text ?? null,
    event: query.event ?? null,
    after_seq: after,
    before_seq: before,
    limit,
  };
}

/**
 * Pages through the engine's log ring, newest first. `get_logs` answers with the newest
 * `limit` events matching (older than `before_seq` when given), oldest first; the cursor of a
 * full page is its oldest `seq`. The engine can record two events at nearly the same moment in
 * the opposite order to their `seq`, so the page is put in `seq` order here: the cursor is then
 * the smallest `seq` shown and the list never reads out of order.
 */
export class LogPager {
  constructor(private readonly client: IpcClient) {}

  async query(query: LogQuery, page: { before: number | null; limit: number }): Promise<LogPage> {
    const { events } = await this.client.request(
      { type: "get_logs", filter: logFilter(query, page.limit, page.before) },
      "logs",
    );
    const newestFirst = events
      .slice(-page.limit)
      .map(logEventFromIpc)
      .sort((a, b) => b.seq - a.seq);
    const oldest = newestFirst.at(-1);
    if (oldest === undefined) return { events: [], olderCursor: null };
    return {
      events: newestFirst,
      olderCursor: newestFirst.length >= page.limit ? oldest.seq : null,
    };
  }
}

/** What `followLogs` reads after an outage, at most: the size of the page's window. */
const BACKFILL_LIMIT = 10_000;
/** Reads again this often when the engine answers badly while connected, before saying so. */
const BACKFILL_ATTEMPTS = 3;

interface Backfill {
  /** Oldest first. */
  events: LogEvent[];
  missing: LogFollowNotice["missing"];
  restarted: boolean;
}

/** Whichever of two RFC 3339 times is later; either may be absent. */
function later(a: string | undefined, b: string): string {
  return a !== undefined && Date.parse(a) > Date.parse(b) ? a : b;
}

/**
 * One `followLogs` subscription. The Rust side keeps the subscription across engine reconnects,
 * but events published while the engine was away never reach it. So the follower tracks the
 * connection and, when it returns, reads what was missed from the engine's log (`after_seq`
 * the last event delivered) and delivers it once, oldest first, with a notice. Events that
 * arrive while it reads are held and merged, so none is repeated or out of order.
 *
 * The engine's sequence belongs to one run: a restart is found from the snapshot's start time
 * (or a sequence that went backwards) and delivered as such. Events lost to the ring are found
 * from the ring itself: it holds consecutive events, so if none from before the outage is left
 * in it, some of the outage's are gone too.
 */
class LogFollow {
  private batch: LogEvent[] = [];
  private timer: ReturnType<typeof setTimeout> | null = null;
  private live = true;
  private stops: Unsubscribe[] = [];
  /** The largest `seq` delivered; later events at or below it are repeats. */
  private delivered: number | null = null;
  private everConnected = false;
  private connected = false;
  /** When the connection was lost, while an outage has not been made up for. */
  private outageSince: string | null = null;
  private recovering = false;
  /** Live events that arrived while the missed ones were being read. */
  private held: LogEvent[] | null = null;
  /** The engine's start time, to tell a restart. */
  private startedAt: string | null = null;

  constructor(
    private readonly client: IpcClient,
    private readonly query: LogQuery,
    private readonly onEvents: (events: LogEvent[], notice?: LogFollowNotice) => void,
  ) {}

  start(): Unsubscribe {
    this.stops.push(
      this.client.subscribe({ type: "logs", filter: logFilter(this.query, null) }, (event) =>
        this.receive(event),
      ),
      this.client.watchConnection((payload) => this.connection(payload)),
    );
    return () => {
      this.live = false;
      if (this.timer !== null) clearTimeout(this.timer);
      for (const stop of this.stops.splice(0)) stop();
    };
  }

  private receive(event: Event): void {
    if (event.type !== "log") return;
    const converted = logEventFromIpc(event.event);
    if (this.held !== null) this.held.push(converted);
    else this.accept(converted);
  }

  private accept(event: LogEvent): void {
    if (this.delivered !== null && event.seq <= this.delivered) return;
    this.delivered = event.seq;
    this.batch.push(event);
    this.timer ??= setTimeout(() => this.flush(), BATCH_MS);
  }

  private flush(): void {
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
    const events = this.batch;
    this.batch = [];
    if (this.live && events.length > 0) this.onEvents(events);
  }

  private connection(payload: ConnectionPayload): void {
    this.connected = payload.state === "connected";
    if (!this.connected) {
      // An outage is one that interrupts a stream that was running.
      if (this.everConnected) this.outageSince ??= payload.since;
      return;
    }
    if (!this.everConnected) {
      this.everConnected = true;
      void this.engineStart().then((startedAt) => {
        this.startedAt ??= startedAt;
      });
    } else if (this.outageSince !== null) {
      void this.recover(payload.since);
    }
  }

  private engineStart(): Promise<string | null> {
    return this.client
      .request({ type: "get_snapshot" }, "snapshot")
      .then(({ snapshot }) => snapshot.node.started_at)
      .catch(() => null);
  }

  private async recover(backAt: string): Promise<void> {
    if (this.recovering) return;
    this.recovering = true;
    this.held = [];
    this.flush();
    const lostAt = this.outageSince ?? backAt;
    const found = await this.readMissed(lostAt);
    const held = this.held;
    this.held = null;
    this.recovering = false;
    if (!this.live) return;
    if (found === null && !this.connected) {
      // Gone again before anything was read: the next return starts over.
      for (const event of held) this.accept(event);
      return;
    }
    if (this.connected) this.outageSince = null;
    // The engine answered nothing usable while connected: say that events may be missing.
    const made: Backfill = found ?? { events: [], missing: "gap", restarted: false };
    this.deliverRecovered(made, held, { lostAt, backAt });
  }

  /** What the outage cost, or null when it could not be read. */
  private async readMissed(lostAt: string): Promise<Backfill | null> {
    for (let attempt = 0; attempt < BACKFILL_ATTEMPTS; attempt++) {
      try {
        return await this.backfill(lostAt);
      } catch {
        // The connection went again: its return starts another recovery.
        if (!this.connected) return null;
      }
    }
    return null;
  }

  private deliverRecovered(
    made: Backfill,
    held: readonly LogEvent[],
    times: { lostAt: string; backAt: string },
  ): void {
    if (made.restarted) this.delivered = null;
    const events: LogEvent[] = [];
    let filled = 0;
    for (const [index, event] of [...made.events, ...held].entries()) {
      if (this.delivered !== null && event.seq <= this.delivered) continue;
      this.delivered = event.seq;
      events.push(event);
      if (index < made.events.length) filled += 1;
    }
    this.onEvents(events, { ...times, filled, missing: made.missing });
  }

  /** Reads what the outage cost; throws when the engine cannot be asked. */
  private async backfill(lostAt: string): Promise<Backfill> {
    const [startedAt, newest, ringHolds] = await Promise.all([
      this.engineStart(),
      this.probe({ limit: 1 }).then((events) => events.at(-1)?.seq ?? null),
      this.probe({ until: lostAt, limit: 1 }).then((events) => events.length > 0),
    ]);
    const restarted =
      (this.startedAt !== null && startedAt !== null && startedAt !== this.startedAt) ||
      (this.delivered !== null && newest !== null && newest < this.delivered);
    this.startedAt = startedAt ?? this.startedAt;
    const fresh = restarted ? null : this.delivered;
    // With nothing delivered yet there is no sequence to continue from: ask by time.
    const query =
      fresh === null && !restarted
        ? { ...this.query, from: later(this.query.from, lostAt) }
        : this.query;
    const { events } = await this.client.request(
      { type: "get_logs", filter: logFilter(query, BACKFILL_LIMIT, null, fresh) },
      "logs",
    );
    const lost = events.length >= BACKFILL_LIMIT || (newest !== null && !ringHolds);
    let missing: Backfill["missing"] = "none";
    if (restarted) missing = "restarted";
    else if (lost) missing = "gap";
    return { events: events.map(logEventFromIpc), missing, restarted };
  }

  /** Events of the whole log (no filters but these), the newest `limit`, oldest first. */
  private async probe(extra: Partial<LogFilter>): Promise<RawLogEvent[]> {
    const filter: LogFilter = { ...logFilter({}, null), ...extra };
    const { events } = await this.client.request({ type: "get_logs", filter }, "logs");
    return events;
  }
}

/** Streams new events matching `query`, batched, oldest first, across engine reconnects. */
export function followLogs(
  client: IpcClient,
  query: LogQuery,
  onEvents: (events: LogEvent[], notice?: LogFollowNotice) => void,
): Unsubscribe {
  return new LogFollow(client, query, onEvents).start();
}
