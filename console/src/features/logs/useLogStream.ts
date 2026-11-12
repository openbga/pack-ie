import { useEffect, useMemo, useReducer } from "react";
import { useEngineApi } from "@/api/context";
import { useEngineQuery } from "@/api/hooks";
import type { EngineError, LogEvent, LogFollowNotice, LogPage } from "@/api/types";
import { toEngineError } from "@/lib/errors";
import { type LogFilters, toLogQuery } from "./logParams";
import type { EventRow, LogRow } from "./logRows";

/** Events asked for at a time, at first and for each "Load older". */
const PAGE_SIZE = 200;
/** Rows kept in memory (CON-05); the oldest drop out of a full window. */
export const MAX_EVENTS = 10_000;

/** Why the list holds still: the operator paused it, scrolled away, or opened an event. */
export type PauseReason = "manual" | "scroll" | "selection";

/**
 * A row received by following. `n` counts arrivals, so a hold can tell what came after it
 * whatever `seq` says; `epoch` counts the engine restarts seen, each of which starts `seq` over.
 */
interface LiveEntry {
  n: number;
  epoch: number;
  row: LogRow;
}

interface StreamState {
  /** The filters these events answer; state for other filters is discarded. */
  key: string;
  /** Events and dividers received by following, newest first. */
  live: readonly LiveEntry[];
  /** Arrivals so far. */
  served: number;
  /** Engine restarts seen while following; the events before the first are the page's own. */
  epoch: number;
  /** Pages loaded with "Load older", newest first. */
  older: readonly LogEvent[];
  /** The next older page's cursor once one was loaded; `undefined` before, then `null` at the end. */
  olderCursor: number | null | undefined;
  loadingOlder: boolean;
  olderError: EngineError | null;
  /** While paused: the last arrival shown. Later ones wait, counted, until resumed. */
  frozenN: number | null;
  pauseReason: PauseReason | null;
}

const initial = (key: string): StreamState => ({
  key,
  live: [],
  served: 0,
  epoch: 0,
  older: [],
  olderCursor: undefined,
  loadingOlder: false,
  olderError: null,
  frozenN: null,
  pauseReason: null,
});

type Action = { key: string } & (
  | { type: "live"; events: readonly LogEvent[]; notice: LogFollowNotice | undefined }
  | { type: "pause"; reason: PauseReason }
  | { type: "resume"; reason: PauseReason; atTop: boolean }
  | { type: "olderStart" }
  | { type: "olderDone"; page: LogPage }
  | { type: "olderFail"; error: EngineError }
  | { type: "reset" }
);

const eventRows = new WeakMap<LogEvent, EventRow>();

/** The row of an event of the page's own epoch (before any restart); one object per event. */
function rowOf(event: LogEvent, epoch = 0): EventRow {
  if (epoch !== 0) return { kind: "event", key: `${epoch}:${event.seq}`, event };
  let row = eventRows.get(event);
  if (row === undefined) {
    row = { kind: "event", key: `0:${event.seq}`, event };
    eventRows.set(event, row);
  }
  return row;
}

/** The newest followed event of `epoch`, to tell repeats from news. */
function newestSeq(live: readonly LiveEntry[], epoch: number): number {
  for (const entry of live) {
    if (entry.epoch === epoch && entry.row.kind === "event") return entry.row.event.seq;
  }
  return Number.NEGATIVE_INFINITY;
}

/**
 * Followed events arrive oldest first; keep each once, newest first. A notice comes before the
 * events it made up for; a restart in it starts a new epoch, whose `seq` is not compared with
 * the earlier ones.
 */
function addLive(
  state: StreamState,
  events: readonly LogEvent[],
  notice: LogFollowNotice | undefined,
): StreamState {
  let { epoch, served } = state;
  const arrived: LiveEntry[] = [];
  if (notice !== undefined) {
    if (notice.missing === "restarted") epoch += 1;
    served += 1;
    arrived.push({ n: served, epoch, row: { kind: "notice", key: `notice:${served}`, notice } });
  }
  let known = epoch === state.epoch ? newestSeq(state.live, epoch) : Number.NEGATIVE_INFINITY;
  for (const event of events) {
    if (event.seq <= known) continue;
    known = event.seq;
    served += 1;
    arrived.push({ n: served, epoch, row: rowOf(event, epoch) });
  }
  if (arrived.length === 0) return state;
  return {
    ...state,
    epoch,
    served,
    live: [...arrived.reverse(), ...state.live].slice(0, MAX_EVENTS),
  };
}

function pause(state: StreamState, reason: PauseReason): StreamState {
  if (state.pauseReason === null) {
    return { ...state, pauseReason: reason, frozenN: state.served };
  }
  // Already held: only an explicit pause changes why, so scrolling back does not release it.
  return reason === "manual" ? { ...state, pauseReason: "manual" } : state;
}

function resume(state: StreamState, reason: PauseReason, atTop: boolean): StreamState {
  if (state.pauseReason === null) return state;
  if (reason !== "manual" && reason !== state.pauseReason) return state;
  // Closing an event while scrolled away leaves the list held until it is back at the top.
  if (reason !== "manual" && !atTop) return { ...state, pauseReason: "scroll" };
  return { ...state, frozenN: null, pauseReason: null };
}

function reduce(state: StreamState, action: Action): StreamState {
  const current = state.key === action.key ? state : initial(action.key);
  switch (action.type) {
    case "live":
      return addLive(current, action.events, action.notice);
    case "pause":
      return pause(current, action.reason);
    case "resume":
      return resume(current, action.reason, action.atTop);
    case "olderStart":
      return { ...current, loadingOlder: true, olderError: null };
    case "olderDone":
      return {
        ...current,
        loadingOlder: false,
        older: [...current.older, ...action.page.events],
        olderCursor: action.page.olderCursor,
      };
    case "olderFail":
      return { ...current, loadingOlder: false, olderError: action.error };
    case "reset":
      return initial(action.key);
  }
}

/**
 * The log as the page shows it: the newest events of the filters, followed live and extended
 * backwards on request, in one bounded window (newest first). Following can be held, so the
 * rows do not move under the operator; events that arrive meanwhile are counted, and shown on
 * resume. State belongs to one set of filters: changing them starts again.
 */
export function useLogStream(filters: LogFilters, following: boolean) {
  const api = useEngineApi();
  const key = JSON.stringify(filters);
  const base = useEngineQuery(
    ["queryLogs", filters],
    (engine) =>
      engine.queryLogs(toLogQuery(filters, Date.now()), { before: null, limit: PAGE_SIZE }),
    { staleTime: 0 },
  );
  const [stored, dispatch] = useReducer(reduce, key, initial);
  const state = stored.key === key ? stored : initial(key);

  // Follows the engine's stream, an outside source, for as long as these filters and following hold.
  useEffect(() => {
    if (!following) return;
    return api.followLogs(toLogQuery(filters, Date.now()), (events, notice) =>
      dispatch({ key, type: "live", events, notice }),
    );
  }, [api, filters, key, following]);

  const baseEvents = base.data?.events;
  const view = useMemo(() => {
    const fromBase = baseEvents ?? [];
    // A followed event the first answer already holds is not shown twice.
    const newestInBase = fromBase[0]?.seq ?? Number.NEGATIVE_INFINITY;
    const received = state.live.filter(
      ({ epoch, row }) => row.kind === "notice" || epoch > 0 || row.event.seq > newestInBase,
    );
    const frozen = state.frozenN;
    const shownLive = frozen === null ? received : received.filter(({ n }) => n <= frozen);
    const rows: LogRow[] = [
      ...shownLive.map(({ row }) => row),
      ...fromBase.map((event) => rowOf(event)),
      ...state.older.map((event) => rowOf(event)),
    ].slice(0, MAX_EVENTS);
    const held = received.slice(0, received.length - shownLive.length);
    return {
      rows,
      count: rows.filter((row) => row.kind === "event").length,
      held: held.filter(({ row }) => row.kind === "event").length,
    };
  }, [baseEvents, state.live, state.older, state.frozenN]);

  const { rows } = view;
  // After a restart the engine's `seq` start over, so the cursor of the earlier run is void.
  const restarted = state.epoch > 0;
  const cursor = restarted
    ? null
    : state.olderCursor === undefined
      ? (base.data?.olderCursor ?? null)
      : state.olderCursor;
  const windowFull = rows.length >= MAX_EVENTS;
  const paused = following && state.pauseReason !== null;

  return {
    base,
    /** Events and dividers, newest first. */
    rows,
    /** Events among the rows. */
    count: view.count,
    /** Events received while paused, not yet shown. */
    held: paused ? view.held : 0,
    paused,
    pauseReason: following ? state.pauseReason : null,
    canLoadOlder: cursor !== null && !windowFull,
    windowFull,
    atEnd: cursor === null,
    /** The engine restarted while following: its earlier events cannot be loaded. */
    restarted,
    loadingOlder: state.loadingOlder,
    olderError: state.olderError,
    pause: (reason: PauseReason) => dispatch({ key, type: "pause", reason }),
    resume: (reason: PauseReason, atTop: boolean) =>
      dispatch({ key, type: "resume", reason, atTop }),
    /** Forgets what followed and was loaded, and reads the newest events again. */
    refresh: () => {
      dispatch({ key, type: "reset" });
      return base.refetch();
    },
    loadOlder: async () => {
      if (cursor === null || state.loadingOlder) return;
      dispatch({ key, type: "olderStart" });
      try {
        const page = await api.queryLogs(toLogQuery(filters, Date.now()), {
          before: cursor,
          limit: PAGE_SIZE,
        });
        dispatch({ key, type: "olderDone", page });
      } catch (error) {
        dispatch({ key, type: "olderFail", error: toEngineError(error) });
      }
    },
  };
}
