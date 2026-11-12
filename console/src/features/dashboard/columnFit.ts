import { type FitSpec, fitColumns } from "../shared-operate/fitColumns";

/** The dashboard table's columns, in display order. */
export type ChannelColumnId =
  | "channel"
  | "state"
  | "endpoints"
  | "traffic"
  | "activity"
  | "queue"
  | "session"
  | "retry"
  | "error";

/**
 * Fixed widths in pixels of the columns that do not stretch; the table uses the same numbers,
 * so the fit below is exact. Channel and Last error share what is left.
 */
export const FIXED_WIDTH = {
  state: 190,
  traffic: 190,
  activity: 190,
  queue: 150,
  session: 130,
  retry: 130,
} as const;

/** Narrowest the stretching columns may get, in pixels. Endpoints never gets less. */
const MIN_WIDTH: Record<ChannelColumnId, number> = {
  channel: 200,
  state: FIXED_WIDTH.state,
  endpoints: 280,
  traffic: FIXED_WIDTH.traffic,
  activity: FIXED_WIDTH.activity,
  queue: FIXED_WIDTH.queue,
  session: FIXED_WIDTH.session,
  retry: FIXED_WIDTH.retry,
  error: 220,
};

/** The most Endpoints takes when there is room: its names stay whole on long labels. */
const MAX_ENDPOINTS_WIDTH = 420;

/**
 * The order columns are dropped in as the table narrows, first dropped first (operator-console.md
 * section 3): Retry, Last error, Session, Last activity, Traffic, Queue. Channel, State and
 * Endpoints are never dropped.
 */
export const DROP_ORDER: readonly ChannelColumnId[] = [
  "retry",
  "error",
  "session",
  "activity",
  "traffic",
  "queue",
];

/** Every column, in display order. */
export const COLUMN_ORDER: readonly ChannelColumnId[] = [
  "channel",
  "state",
  "endpoints",
  "traffic",
  "activity",
  "queue",
  "session",
  "retry",
  "error",
];

const SPECS: readonly FitSpec<ChannelColumnId>[] = COLUMN_ORDER.map((id) => {
  const place = DROP_ORDER.indexOf(id);
  return place === -1 ? { id, min: MIN_WIDTH[id] } : { id, min: MIN_WIDTH[id], drop: place + 1 };
});

/** The columns that fit in `width` pixels; `null` (width not measured) keeps them all. */
export function visibleColumns(width: number | null): readonly ChannelColumnId[] {
  return fitColumns(SPECS, width);
}

/**
 * The Endpoints column's width: its minimum, plus half of what the shown columns leave over,
 * up to a maximum. A fixed width, not a share, so it never narrows below its minimum and
 * names and states are not cut mid-word. `null` (not measured) gives the minimum.
 */
export function endpointsWidth(width: number | null, shown: readonly ChannelColumnId[]): number {
  const minimum = MIN_WIDTH.endpoints;
  if (width === null) return minimum;
  const used = shown.reduce((sum, id) => sum + MIN_WIDTH[id], 0);
  return Math.min(MAX_ENDPOINTS_WIDTH, minimum + Math.max(0, Math.floor((width - used) / 2)));
}
