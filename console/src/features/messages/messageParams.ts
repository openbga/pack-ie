import type {
  DeliveryState,
  MessageDirection,
  MessageFilter,
  MessageQuery,
  MessageSortField,
  MessageState,
} from "@/api/types";
import { DELIVERY_STATES, MESSAGE_DIRECTIONS, MESSAGE_STATES } from "./messageWording";

/** Rows per page offered, and the one a visit starts with. */
export const PAGE_SIZES = [50, 100, 200] as const;
export const DEFAULT_PAGE_SIZE = 100;

/** Shortest text search the engine accepts. */
export const MIN_TEXT = 3;

// ── Quick filters ────────────────────────────────────────────────────────────────────────

export type QuickFilter = "attention" | "queued" | "duplicates" | "invalid_aborted";

interface QuickDefinition {
  label: string;
  /** What the filter asks of the engine. */
  criteria: Pick<MessageFilter, "states" | "deliveryStates" | "deliverySummaries">;
}

export const QUICK_FILTERS: Record<QuickFilter, QuickDefinition> = {
  attention: { label: "Needs attention", criteria: { deliverySummaries: ["attention"] } },
  queued: { label: "Queued", criteria: { deliveryStates: ["queued"] } },
  duplicates: { label: "Duplicates", criteria: { states: ["duplicate"] } },
  invalid_aborted: { label: "Invalid or aborted", criteria: { states: ["invalid", "aborted"] } },
};

export const QUICK_ORDER: readonly QuickFilter[] = [
  "attention",
  "queued",
  "duplicates",
  "invalid_aborted",
];

const isQuick = (value: string): value is QuickFilter => value in QUICK_FILTERS;

// ── Filters ──────────────────────────────────────────────────────────────────────────────

/**
 * The filters of the Messages list. A quick filter stands in for the message state and
 * delivery state choices (choosing one clears the other), so the two never contradict.
 */
export interface MessageFilters {
  /** Message channel ids; none means every channel. */
  channels: string[];
  direction?: MessageDirection;
  state?: MessageState;
  delivery?: DeliveryState;
  quick?: QuickFilter;
  /** An RFC 3339 time, or a relative one (`-24h`) resolved when the list is read. */
  from?: string;
  to?: string;
  /** Control, sample or patient id. Never in the URL: it can be a patient identifier. */
  id?: string;
  idExact?: boolean;
  /** Text in identifiers, the message type and stored bodies. Never in the URL either. */
  text?: string;
  /** The start of a message ID (hex digits, dashes optional). Kept in memory like the others. */
  messageId?: string;
}

/** The criteria that can hold patient data or content: never in the URL or storage. */
export const SENSITIVE_KEYS = ["id", "idExact", "text", "messageId"] as const;
export type SensitiveFilters = Pick<MessageFilters, (typeof SENSITIVE_KEYS)[number]>;

/** The time range a visit starts with (spec §6). */
const DEFAULT_RANGE = "-24h";

export const DEFAULT_FILTERS: MessageFilters = { channels: [], from: DEFAULT_RANGE };

export interface MessageSort {
  field: MessageSortField;
  descending: boolean;
}

/** Newest first, until a column is sorted. */
export const DEFAULT_SORT: MessageSort = { field: "receivedAt", descending: true };

/** What the page's URL holds: the filters but the sensitive ones, the sort and the page. */
export interface MessageView {
  filters: MessageFilters;
  sort: MessageSort;
  page: number;
}

const SORT_FIELDS: readonly MessageSortField[] = [
  "receivedAt",
  "channel",
  "state",
  "messageType",
  "sizeBytes",
];

export function isSortField(value: string): value is MessageSortField {
  return (SORT_FIELDS as readonly string[]).includes(value);
}

/** Relative times the quick ranges use, so a range keeps meaning "the last hour". */
export const RELATIVE_MS: Readonly<Record<string, number>> = {
  "-1h": 3_600_000,
  "-24h": 86_400_000,
  "-7d": 604_800_000,
  "-30d": 2_592_000_000,
};

/** A filter time as an RFC 3339 string at `now`; `undefined` for a value that is no time. */
export function resolveTime(value: string | undefined, now: number): string | undefined {
  if (value === undefined) return undefined;
  const relative = RELATIVE_MS[value];
  const time = relative === undefined ? new Date(value).getTime() : now - relative;
  return Number.isNaN(time) ? undefined : new Date(time).toISOString();
}

const oneOf = <T extends string>(values: readonly T[], value: string | null): T | undefined =>
  values.find((candidate) => candidate === value);

const text = (params: URLSearchParams, name: string) => params.get(name)?.trim() || undefined;

/** The time range of the URL: the default unless it says `range=any` or names times. */
function parseRange(params: URLSearchParams): Pick<MessageFilters, "from" | "to"> {
  const from = text(params, "from");
  const to = text(params, "to");
  if (from || to) return { ...(from ? { from } : {}), ...(to ? { to } : {}) };
  return params.get("range") === "any" ? {} : { from: DEFAULT_RANGE };
}

function parseFilters(params: URLSearchParams): MessageFilters {
  const filters: MessageFilters = {
    channels: params.getAll("channel").filter((channel) => channel.trim() !== ""),
    ...parseRange(params),
  };
  const direction = oneOf(MESSAGE_DIRECTIONS, params.get("direction"));
  const quick = params.get("quick");
  if (direction) filters.direction = direction;
  if (quick !== null && isQuick(quick)) {
    filters.quick = quick;
    return filters;
  }
  const state = oneOf(MESSAGE_STATES, params.get("state"));
  const delivery = oneOf(DELIVERY_STATES, params.get("delivery"));
  if (state) filters.state = state;
  if (delivery) filters.delivery = delivery;
  return filters;
}

/** Reads the view from the URL; anything unknown or malformed is ignored, never an error. */
export function parseMessageView(params: URLSearchParams): MessageView {
  const field = params.get("sort");
  const sort: MessageSort =
    field !== null && isSortField(field)
      ? { field, descending: params.get("desc") === "1" }
      : DEFAULT_SORT;
  const page = Number.parseInt(params.get("page") ?? "1", 10);
  return {
    filters: parseFilters(params),
    sort,
    page: Number.isInteger(page) && page > 1 ? page : 1,
  };
}

/** Whether the filters ask for the default time range. */
export const hasDefaultRange = ({ from, to }: MessageFilters): boolean =>
  from === DEFAULT_RANGE && to === undefined;

/**
 * The URL for a view: only what differs from the defaults, so a plain list has a plain URL.
 * The identifier, text and message ID filters can hold a patient identifier or message
 * content, so they are never written to the URL (see `useMessageView`).
 */
export function toSearchParams({ filters, sort, page }: MessageView): URLSearchParams {
  const params = new URLSearchParams();
  for (const channel of filters.channels) params.append("channel", channel);
  const set = (name: string, value: string | undefined) => {
    if (value) params.set(name, value);
  };
  set("direction", filters.direction);
  set("quick", filters.quick);
  set("state", filters.state);
  set("delivery", filters.delivery);
  if (filters.from || filters.to) {
    if (!hasDefaultRange(filters)) {
      set("from", filters.from);
      set("to", filters.to);
    }
  } else {
    params.set("range", "any");
  }
  if (sort.field !== DEFAULT_SORT.field || sort.descending !== DEFAULT_SORT.descending) {
    params.set("sort", sort.field);
    if (sort.descending) params.set("desc", "1");
  }
  if (page > 1) params.set("page", String(page));
  return params;
}

/** Whether any filter other than the time range is set. */
export function isNarrowed(filters: MessageFilters): boolean {
  return (
    filters.channels.length > 0 ||
    filters.direction !== undefined ||
    filters.state !== undefined ||
    filters.delivery !== undefined ||
    filters.quick !== undefined ||
    Boolean(filters.id) ||
    Boolean(filters.text) ||
    Boolean(filters.messageId)
  );
}

/** Whether the list differs from what a visit starts with. */
export function isFiltered(filters: MessageFilters): boolean {
  return isNarrowed(filters) || !hasDefaultRange(filters);
}

/** Whether the time range is open: every stored message, whenever it was received. */
export const isAnyTime = ({ from, to }: MessageFilters): boolean =>
  from === undefined && to === undefined;

/** Text of fewer than `MIN_TEXT` characters is not a search: the engine would refuse it. */
export const isSearchableText = (value: string | undefined): value is string =>
  value !== undefined && [...value.trim()].length >= MIN_TEXT;

/** Fewest and most hex digits of a message ID the engine takes as a prefix. */
const MESSAGE_ID_DIGITS = { min: 8, max: 32 };

/**
 * A message ID prefix as the engine reads it: hex digits and dashes, 8 to 32 digits. Anything
 * else is not a search yet (the engine would refuse it), like text of fewer than 3 characters.
 */
export function isMessageIdPrefix(value: string | undefined): value is string {
  if (value === undefined) return false;
  const digits = value.trim().replaceAll("-", "");
  return (
    /^[0-9a-fA-F]+$/.test(digits) &&
    digits.length >= MESSAGE_ID_DIGITS.min &&
    digits.length <= MESSAGE_ID_DIGITS.max
  );
}

/** The engine's filter without the state criteria: what a quick filter's count builds on. */
function baseFilter(filters: MessageFilters, now: number): MessageFilter {
  const filter: MessageFilter = {};
  const since = resolveTime(filters.from, now);
  const until = resolveTime(filters.to, now);
  if (since) filter.since = since;
  if (until) filter.until = until;
  if (filters.channels.length > 0) filter.channelIds = filters.channels;
  if (filters.direction) filter.directions = [filters.direction];
  if (filters.id?.trim()) {
    filter.identifier = { value: filters.id.trim(), prefix: !filters.idExact };
  }
  if (isSearchableText(filters.text)) filter.text = filters.text.trim();
  if (isMessageIdPrefix(filters.messageId)) filter.messageIdPrefix = filters.messageId.trim();
  return filter;
}

/** The engine's filter for the list: relative times are resolved at `now`. */
export function toMessageFilter(filters: MessageFilters, now: number): MessageFilter {
  const filter = baseFilter(filters, now);
  if (filters.quick) return { ...filter, ...QUICK_FILTERS[filters.quick].criteria };
  if (filters.state) filter.states = [filters.state];
  if (filters.delivery) filter.deliveryStates = [filters.delivery];
  return filter;
}

/** The engine's query for one page of a view. */
export function toMessageQuery(view: MessageView, pageSize: number, now: number): MessageQuery {
  return {
    filter: toMessageFilter(view.filters, now),
    sort: view.sort,
    offset: (view.page - 1) * pageSize,
    limit: pageSize,
  };
}

/**
 * The query that counts a quick filter: the current filters (the quick filter and the state
 * choices aside) plus the quick filter's criteria, one row asked for, since only `total` counts.
 */
export function quickCountQuery(
  filters: MessageFilters,
  quick: QuickFilter,
  now: number,
): MessageQuery {
  return {
    filter: { ...baseFilter(filters, now), ...QUICK_FILTERS[quick].criteria },
    sort: DEFAULT_SORT,
    offset: 0,
    limit: 1,
  };
}
