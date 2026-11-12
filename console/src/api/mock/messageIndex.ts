import type {
  DeliveryState,
  DeliverySummary,
  MessageDirection,
  MessageFilter,
  MessageQuery,
  MessageSortField,
  MessageState,
  MessageSummary,
} from "../types";
import { summarize } from "./deliveries";
import { KIND_STATE, stateRank } from "./messageKinds";
import type { MessageUniverse } from "./messageModel";
import { eachInSteps, finish, letPageRun, type Steps, sortInSteps } from "./steps";

/** Where the index reads the deliveries of a message, which operator actions may have changed. */
export interface DeliverySource {
  deliveryStatesAt(i: number, kind: number): DeliveryState[];
}

const MAX_KEY = 2 ** 30 - 1;
const INDEX_RADIX = 4_194_304;
/** A text search looks at no more than the newest messages of the range, like an engine with a scan budget. */
const TEXT_SCAN_CAP = 50_000;

interface Filter {
  lo: number;
  hi: number;
  slots: ReadonlySet<number> | null;
  directions: ReadonlySet<MessageDirection> | null;
  states: ReadonlySet<MessageState> | null;
  deliveryStates: ReadonlySet<DeliveryState> | null;
  summaries: ReadonlySet<DeliverySummary> | null;
  identifier: { needle: string; prefix: boolean } | null;
  text: string | null;
  /** The start of a message id in its stored form; see `storedIdOf`. */
  idPrefix: string | null;
}

/** Fewest and most hex digits of a message id prefix, as the engine takes them. */
const ID_PREFIX_DIGITS = { min: 8, max: 32 };

/**
 * A message id prefix the way the engine reads it (`packie_ipc::id_prefix`): dashes dropped, any
 * case, 8 to 32 hex digits, written back in the stored form with dashes at the UUID positions.
 * `error` is the engine's refusal.
 */
export function idPrefixOf(text: string): { prefix: string } | { error: string } {
  const digits = text.trim().replaceAll("-", "").toLowerCase();
  if (!/^[0-9a-f]*$/.test(digits)) return { error: "a message id has only hex digits and dashes" };
  if (digits.length < ID_PREFIX_DIGITS.min || digits.length > ID_PREFIX_DIGITS.max) {
    return {
      error: `give at least ${ID_PREFIX_DIGITS.min} and at most ${ID_PREFIX_DIGITS.max} hex digits of the message id`,
    };
  }
  return {
    prefix: [...digits]
      .map((digit, i) => ([8, 12, 16, 20].includes(i) ? `-${digit}` : digit))
      .join(""),
  };
}

/**
 * The stored (UUID) form of a mock message's id. The mock numbers its messages (`msg-7`), the
 * engine gives them UUIDs; the prefix filter reads the number as the last 12 hex digits, and a
 * message an operator created (`msg-new-2`) as the last digits of another group.
 */
function storedIdOf(id: string): string {
  const made = /^msg-new-(\d+)$/.exec(id);
  const generated = /^msg-(\d+)$/.exec(id);
  const group = made ? "8001" : "8000";
  const number = Number(made?.[1] ?? generated?.[1] ?? 0);
  return `00000000-0000-4000-${group}-${number.toString(16).padStart(12, "0")}`;
}

const toSet = <T>(values: readonly T[] | undefined): ReadonlySet<T> | null =>
  values === undefined || values.length === 0 ? null : new Set(values);

/** Whether the filter looks at the deliveries of a message at all. */
const needsDeliveries = (filter: Filter) =>
  filter.deliveryStates !== null || filter.summaries !== null;

/** Server-side filtering and sorting of the generated message history, with the last result cached. */
export class MessageIndex {
  private cacheKey = "";
  private cacheVersion = -1;
  private cached: Int32Array = new Int32Array(0);

  constructor(
    private readonly universe: MessageUniverse,
    private readonly deliveries: DeliverySource,
  ) {}

  /** Indexes of the matching messages in the requested order. */
  run(query: MessageQuery, version: number): Int32Array {
    const key = JSON.stringify([query.filter, query.sort]);
    if (key === this.cacheKey && version === this.cacheVersion) return this.cached;
    this.cached = finish(this.search(query));
    this.cacheKey = key;
    this.cacheVersion = version;
    return this.cached;
  }

  /**
   * The same answer as `run`, found in steps with the page's own work in between, so that a
   * search of the whole history does not hold the page still. A change to the messages meanwhile
   * (`version` moves on) starts the search again, so the answer is of one state.
   */
  async runInSteps(query: MessageQuery, version: () => number): Promise<Int32Array> {
    const key = JSON.stringify([query.filter, query.sort]);
    for (;;) {
      const started = version();
      if (key === this.cacheKey && started === this.cacheVersion) return this.cached;
      const steps = this.search(query);
      for (let next = steps.next(); ; next = steps.next()) {
        if (next.done === true) {
          if (version() !== started) break;
          this.cached = next.value;
          this.cacheKey = key;
          this.cacheVersion = started;
          return this.cached;
        }
        await letPageRun();
      }
    }
  }

  private *search(query: MessageQuery): Steps<Int32Array> {
    const matches = yield* this.scan(query.filter);
    return yield* this.order(matches, query.sort.field, query.sort.descending);
  }

  private firstIndex(isPast: (i: number) => boolean): number {
    let low = 0;
    let high = this.universe.count;
    while (low < high) {
      const mid = (low + high) >> 1;
      if (isPast(mid)) high = mid;
      else low = mid + 1;
    }
    return low;
  }

  private buildFilter(filter: MessageFilter): Filter | null {
    const u = this.universe;
    const since = filter.since ? Date.parse(filter.since) : Number.NaN;
    const until = filter.until ? Date.parse(filter.until) : Number.NaN;
    const channels = toSet(filter.channelIds);
    const slots =
      channels === null
        ? null
        : new Set(u.profiles.flatMap((p, slot) => (channels.has(p.channelId) ? [slot] : [])));
    if (slots?.size === 0) return null;
    const needle = filter.identifier?.value.trim().toLowerCase() ?? "";
    const text = filter.text?.trim().toLowerCase() ?? "";
    return {
      lo: Number.isNaN(until) ? 0 : this.firstIndex((i) => u.timeAt(i) < until),
      hi: Number.isNaN(since) ? u.count : this.firstIndex((i) => u.timeAt(i) < since),
      slots,
      directions: toSet(filter.directions),
      states: toSet(filter.states),
      deliveryStates: toSet(filter.deliveryStates),
      summaries: toSet(filter.deliverySummaries),
      identifier: needle === "" ? null : { needle, prefix: filter.identifier?.prefix ?? false },
      text: text === "" ? null : text,
      idPrefix: idPrefixFilter(filter),
    };
  }

  private *scan(raw: MessageFilter): Steps<Int32Array> {
    const filter = this.buildFilter(raw);
    if (filter === null || filter.hi <= filter.lo) return new Int32Array(0);
    const end = filter.text === null ? filter.hi : Math.min(filter.hi, filter.lo + TEXT_SCAN_CAP);
    const out = new Int32Array(end - filter.lo);
    let found = 0;
    yield* eachInSteps(end - filter.lo, (k) => {
      if (this.accepts(filter, filter.lo + k)) out[found++] = filter.lo + k;
    });
    return out.slice(0, found);
  }

  private accepts(filter: Filter, i: number): boolean {
    const u = this.universe;
    const kind = u.kindOf(i);
    if (filter.states !== null && !filter.states.has(KIND_STATE[kind] ?? "routed")) return false;
    if (filter.directions !== null && !filter.directions.has(u.directionOf(i, kind))) return false;
    if (filter.slots !== null && !filter.slots.has(u.slotOf(u.contentOf(i, kind)))) return false;
    if (needsDeliveries(filter) && !this.deliveriesMatch(filter, i, kind)) return false;
    if (filter.identifier !== null && !this.identifierMatches(filter.identifier, i, kind)) {
      return false;
    }
    if (filter.idPrefix !== null && !storedIdOf(`msg-${i}`).startsWith(filter.idPrefix)) {
      return false;
    }
    return filter.text === null || this.textMatches(filter.text, i, kind);
  }

  private deliveriesMatch(filter: Filter, i: number, kind: number): boolean {
    const states = this.deliveries.deliveryStatesAt(i, kind);
    if (filter.deliveryStates !== null && !states.some((s) => filter.deliveryStates?.has(s))) {
      return false;
    }
    return filter.summaries === null || filter.summaries.has(summarize(states));
  }

  private identifierMatches(
    filter: NonNullable<Filter["identifier"]>,
    i: number,
    kind: number,
  ): boolean {
    const { controlId, sampleId, patientId } = this.universe.identifiersOf(i, kind);
    return [controlId, sampleId, patientId].some((id) => {
      const value = id?.toLowerCase();
      if (value === undefined) return false;
      return filter.prefix ? value.startsWith(filter.needle) : value === filter.needle;
    });
  }

  private textMatches(needle: string, i: number, kind: number): boolean {
    const u = this.universe;
    if ((u.typeOf(i, kind) ?? "").toLowerCase().includes(needle)) return true;
    return u.originalText(i, kind).toLowerCase().includes(needle);
  }

  private sortKey(field: MessageSortField, i: number): number {
    const u = this.universe;
    const kind = u.kindOf(i);
    switch (field) {
      case "channel":
        return u.labelRank[u.slotOf(u.contentOf(i, kind))] ?? 0;
      case "state":
        return stateRank(kind);
      case "messageType":
        return u.typeRankOf(i, kind);
      case "sizeBytes":
        return u.sizeOf(i, kind);
      case "receivedAt":
        return i;
    }
  }

  /** Orders matches; ties always fall back to newest first. */
  private *order(
    matches: Int32Array,
    field: MessageSortField,
    descending: boolean,
  ): Steps<Int32Array> {
    if (field === "receivedAt") return descending ? matches : matches.slice().reverse();
    const packed = new Float64Array(matches.length);
    yield* eachInSteps(matches.length, (k) => {
      const i = matches[k] as number;
      const key = this.sortKey(field, i);
      packed[k] = (descending ? MAX_KEY - key : key) * INDEX_RADIX + i;
    });
    const sorted = yield* sortInSteps(packed);
    const out = new Int32Array(matches.length);
    yield* eachInSteps(sorted.length, (k) => {
      const value = sorted[k] as number;
      out[k] = value - Math.floor(value / INDEX_RADIX) * INDEX_RADIX;
    });
    return out;
  }
}

// ── Messages the generator does not own (those an operator created) ─────────────────────

/** The stored-form prefix the filter asks for; `null` for none (or one the store refused). */
function idPrefixFilter(filter: MessageFilter): string | null {
  if (filter.messageIdPrefix === undefined || filter.messageIdPrefix.trim() === "") return null;
  const parsed = idPrefixOf(filter.messageIdPrefix);
  return "prefix" in parsed ? parsed.prefix : null;
}

function matchesIdentifier(row: MessageSummary, filter: MessageFilter): boolean {
  const needle = filter.identifier?.value.trim().toLowerCase() ?? "";
  if (needle === "") return true;
  const prefix = filter.identifier?.prefix ?? false;
  return [row.controlId, row.sampleId, row.patientId].some((id) => {
    const value = id?.toLowerCase();
    if (value === undefined) return false;
    return prefix ? value.startsWith(needle) : value === needle;
  });
}

function matchesIdPrefix(row: MessageSummary, filter: MessageFilter): boolean {
  const prefix = idPrefixFilter(filter);
  return prefix === null || storedIdOf(row.id).startsWith(prefix);
}

function matchesText(row: MessageSummary, filter: MessageFilter, bodyText: () => string): boolean {
  const needle = filter.text?.trim().toLowerCase() ?? "";
  if (needle === "") return true;
  return (
    (row.messageType ?? "").toLowerCase().includes(needle) ||
    bodyText().toLowerCase().includes(needle)
  );
}

/** The same filter as `MessageIndex`, applied to one row. */
export function matchesSummary(
  row: MessageSummary,
  filter: MessageFilter,
  bodyText: () => string,
): boolean {
  const at = Date.parse(row.receivedAt);
  const since = filter.since ? Date.parse(filter.since) : Number.NaN;
  const until = filter.until ? Date.parse(filter.until) : Number.NaN;
  const states = row.deliveries.map((chip) => chip.state);
  const listed = <T>(values: readonly T[] | undefined, value: T) =>
    values === undefined || values.length === 0 || values.includes(value);
  return (
    !(at < since) &&
    !(at >= until) &&
    listed(filter.channelIds, row.channelId) &&
    listed(filter.directions, row.direction) &&
    listed(filter.states, row.state) &&
    listed(filter.deliverySummaries, row.deliverySummary) &&
    (!filter.deliveryStates?.length || states.some((s) => filter.deliveryStates?.includes(s))) &&
    matchesIdentifier(row, filter) &&
    matchesIdPrefix(row, filter) &&
    matchesText(row, filter, bodyText)
  );
}

/** The order the index gives rows: by the sort column, ties newest first. */
export function compareSummaries(
  a: MessageSummary,
  b: MessageSummary,
  sort: MessageQuery["sort"],
): number {
  const newest = Date.parse(b.receivedAt) - Date.parse(a.receivedAt);
  const direction = sort.descending ? -1 : 1;
  switch (sort.field) {
    case "receivedAt":
      return -direction * newest;
    case "channel":
      return direction * compareText(a.channelLabel, b.channelLabel) || newest;
    case "state":
      return direction * compareText(a.state, b.state) || newest;
    case "messageType":
      return direction * compareText(a.messageType ?? "", b.messageType ?? "") || newest;
    case "sizeBytes":
      return direction * (a.sizeBytes - b.sizeBytes) || newest;
  }
}

const compareText = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);
