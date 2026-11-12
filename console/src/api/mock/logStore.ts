import type { Unsubscribe } from "../engine";
import type { LogEvent, LogFollowNotice, LogLevel, LogPage, LogQuery } from "../types";
import { type LogChannel, type LogDraft, LogGenerator, targetOf } from "./logGenerator";
import type { Rng } from "./prng";
import { iso } from "./util";

const LEVEL_RANK: Record<LogLevel, number> = { trace: 0, debug: 1, info: 2, warn: 3, error: 4 };

type OnEvents = (events: LogEvent[], notice?: LogFollowNotice) => void;

interface Follower {
  query: LogQuery;
  onEvents: OnEvents;
  pending: LogEvent[];
  /** The largest `seq` this follower was given, or the log's newest when it began. */
  lastSeq: number;
  /** Began while the connection was down: nothing was interrupted, so nothing is made up. */
  joinedDuringOutage: boolean;
}

/** The connection to the engine is down; what the followers must be told when it returns. */
interface Outage {
  lostAt: string;
  /** The newest `seq` when it began. */
  seqAtLoss: number;
  /** Which run of the engine's log it began in (a restart starts the next). */
  epoch: number;
}

/** What the simulation logs without naming an event: `[text in the message, event]`. */
const INFERRED_EVENTS: readonly (readonly [string, string])[] = [
  ["alert opened", "alert.raised"],
  ["alert cleared", "alert.cleared"],
  ["by operator", "operator.command"],
  ["configuration version", "config.applied"],
  ["endpoint recovered", "endpoint.retry_succeeded"],
  ["queued again", "delivery.resend_requested"],
];

function inferEvent(message: string, fields: Record<string, string>): string | null {
  const hit = INFERRED_EVENTS.find(([text]) => message.includes(text));
  if (hit) return hit[1];
  return fields.state === undefined ? null : "endpoint.state_changed";
}

/** An event of the simulation; `event` and `target` are derived from the message when absent. */
type AppendDraft = Omit<LogDraft, "at" | "event" | "target"> & {
  event?: string | null;
  target?: string;
};

interface CompiledQuery {
  minRank: number;
  fromMs: number;
  toMs: number;
  text: string;
  query: LogQuery;
}

function compile(query: LogQuery): CompiledQuery {
  const from = query.from ? Date.parse(query.from) : Number.NaN;
  const to = query.to ? Date.parse(query.to) : Number.NaN;
  return {
    minRank: query.minLevel ? LEVEL_RANK[query.minLevel] : 0,
    fromMs: Number.isNaN(from) ? Number.NEGATIVE_INFINITY : from,
    toMs: Number.isNaN(to) ? Number.POSITIVE_INFINITY : to,
    text: query.text?.toLowerCase() ?? "",
    query,
  };
}

const has = (text: string, needle: string) => text.toLowerCase().includes(needle);

/**
 * The engine's filter (packie-diagnostics `LogFilter::matches`): `from` is inclusive, `to`
 * exclusive; the event name matches whole, not as a prefix; the text is looked for in the
 * message, target and every field name and value.
 */
function matches(q: CompiledQuery, event: LogEvent): boolean {
  if (LEVEL_RANK[event.level] < q.minRank) return false;
  if (q.query.channelId && event.channelId !== q.query.channelId) return false;
  if (q.query.endpointId && event.endpointId !== q.query.endpointId) return false;
  if (q.query.event && event.event !== q.query.event) return false;
  if (q.text === "") return true;
  if (has(event.message, q.text) || has(event.target, q.text)) return true;
  return Object.entries(event.fields).some(
    ([name, value]) => has(name, q.text) || has(value, q.text),
  );
}

/**
 * The in-memory ring of log events: pre-generated history, events from the simulation, and
 * a ticker that produces live events while someone follows the log.
 */
export class LogStore {
  private events: LogEvent[] = [];
  private nextSeq = 1;
  private readonly followers = new Set<Follower>();
  private epoch = 0;
  private outage: Outage | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private readonly generator: LogGenerator;

  constructor(
    private readonly rng: Rng,
    private readonly now: () => number,
    private readonly ringSize: number,
    channels: LogChannel[],
    messageCount: number,
  ) {
    this.generator = new LogGenerator(rng, channels, messageCount);
    this.preGenerate();
  }

  setChannels(channels: LogChannel[]): void {
    this.generator.setChannels(channels);
  }

  private preGenerate(): void {
    const count = Math.max(0, this.ringSize);
    const gaps = Array.from({ length: count }, () => this.rng.int(120, 680));
    let at = this.now() - gaps.reduce((sum, gap) => sum + gap, 0) - 1000;
    for (const gap of gaps) {
      at += gap;
      this.push(this.generator.next(at));
    }
  }

  private push(draft: LogDraft): LogEvent {
    const event: LogEvent = { seq: this.nextSeq, ...draft };
    this.nextSeq += 1;
    this.events.push(event);
    while (this.events.length > this.ringSize && this.events.length > 0) this.events.shift();
    return event;
  }

  /** Adds an event from the simulation (a state change, a command, a config apply). */
  append(draft: AppendDraft): void {
    const { event: named, target, ...rest } = draft;
    const name = named === undefined ? inferEvent(draft.message, draft.fields) : named;
    const event = this.push({
      at: iso(this.now()),
      ...rest,
      event: name,
      target: target ?? targetOf(name),
    });
    this.offer(event);
  }

  /** The engine logs `count` more events, as it does between ticks. */
  publish(count: number): void {
    for (let k = 0; k < count; k++) this.offer(this.push(this.generator.next(this.now())));
  }

  private offer(event: LogEvent): void {
    if (this.outage !== null) return;
    for (const follower of this.followers) {
      if (matches(compile(follower.query), event)) follower.pending.push(event);
    }
  }

  /** The console loses the engine: followers get nothing until `reconnect`. */
  disconnect(): void {
    if (this.outage !== null) return;
    this.deliverPending();
    this.outage = { lostAt: iso(this.now()), seqAtLoss: this.nextSeq - 1, epoch: this.epoch };
  }

  /**
   * The connection returns. Each follower that was interrupted gets what it missed, once and
   * oldest first, with a notice; a restart (`restart()` during the outage) or a ring that
   * dropped events of the outage is said in the notice.
   */
  reconnect(): void {
    const outage = this.outage;
    this.outage = null;
    if (outage === null) return;
    const backAt = iso(this.now());
    const restarted = this.epoch !== outage.epoch;
    const first = this.events[0]?.seq ?? this.nextSeq;
    const lost = !restarted && this.nextSeq - 1 > outage.seqAtLoss && first > outage.seqAtLoss + 1;
    for (const follower of this.followers) {
      if (follower.joinedDuringOutage) {
        follower.joinedDuringOutage = false;
        follower.lastSeq = this.nextSeq - 1;
        continue;
      }
      const after = restarted ? 0 : follower.lastSeq;
      const compiled = compile(follower.query);
      const events = this.events.filter((event) => event.seq > after && matches(compiled, event));
      follower.lastSeq = this.nextSeq - 1;
      let missing: LogFollowNotice["missing"] = "none";
      if (restarted) missing = "restarted";
      else if (lost) missing = "gap";
      follower.onEvents(events, { lostAt: outage.lostAt, backAt, filled: events.length, missing });
    }
  }

  /** The engine restarts: its log starts again from `seq` 1 with a `node.started` event. */
  restart(): void {
    this.events = [];
    this.nextSeq = 1;
    this.epoch += 1;
    this.append({
      level: "info",
      channelId: null,
      endpointId: null,
      messageId: null,
      message: "node started",
      fields: {},
      event: "node.started",
      target: "packie::node",
    });
  }

  /** The page of `query`, newest first; `from` is inclusive and `to` exclusive, as the engine's. */
  query(query: LogQuery, page: { before: number | null; limit: number }): LogPage {
    const compiled = compile(query);
    const first = this.events[0]?.seq ?? this.nextSeq;
    let index = this.events.length - 1;
    if (page.before !== null) index = Math.min(index, page.before - first - 1);
    const limit = Math.max(0, Math.floor(page.limit));
    const found: LogEvent[] = [];
    let more = false;
    for (; index >= 0; index--) {
      const event = this.events[index] as LogEvent;
      const at = Date.parse(event.at);
      if (at < compiled.fromMs) break;
      if (at >= compiled.toMs || !matches(compiled, event)) continue;
      if (found.length >= limit) {
        more = true;
        break;
      }
      found.push(event);
    }
    const last = found[found.length - 1];
    return { events: found, olderCursor: more && last ? last.seq : null };
  }

  follow(query: LogQuery, onEvents: OnEvents): Unsubscribe {
    const follower: Follower = {
      query,
      onEvents,
      pending: [],
      lastSeq: this.nextSeq - 1,
      joinedDuringOutage: this.outage !== null,
    };
    this.followers.add(follower);
    this.schedule();
    return () => {
      this.followers.delete(follower);
      if (this.followers.size === 0) this.stop();
    };
  }

  private schedule(): void {
    if (this.timer !== null) return;
    const delay = this.rng.int(500, 1000);
    this.timer = setTimeout(() => {
      this.timer = null;
      this.step(delay);
      if (this.followers.size > 0) this.schedule();
    }, delay);
  }

  private step(elapsedMs: number): void {
    const rate = this.rng.int(2, 10);
    const count = Math.max(1, Math.round((rate * elapsedMs) / 1000));
    const end = this.now();
    for (let k = 0; k < count; k++) {
      const at = end - Math.floor(((count - 1 - k) * elapsedMs) / count);
      this.offer(this.push(this.generator.next(at)));
    }
    this.deliverPending();
  }

  private deliverPending(): void {
    for (const follower of this.followers) {
      const batch = follower.pending;
      const newest = batch.at(-1);
      if (newest === undefined) continue;
      follower.pending = [];
      follower.lastSeq = newest.seq;
      follower.onEvents(batch);
    }
  }

  private stop(): void {
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
  }

  dispose(): void {
    this.followers.clear();
    this.stop();
  }
}
