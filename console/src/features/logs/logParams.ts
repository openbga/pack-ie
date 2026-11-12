import type { LogLevel, LogQuery } from "@/api/types";
import { LOG_LEVELS } from "@/lib/status";

/** Live shows what is logged from now on, with no time bound; the others look back from now. */
export type LogRange = "live" | "15m" | "1h" | "24h" | "custom";

/** The Logs filters, as the URL holds them. */
export interface LogFilters {
  /** This level and anything more severe. */
  level: LogLevel;
  channel?: string;
  endpoint?: string;
  /** A catalog event name, matched whole. */
  event?: string;
  text?: string;
  range: LogRange;
  /** Custom range only: RFC 3339 times. */
  from?: string;
  to?: string;
}

const DEFAULT_LEVEL: LogLevel = "info";

export const DEFAULT_FILTERS: LogFilters = { level: DEFAULT_LEVEL, range: "live" };

const RANGE_MS: Readonly<Record<Exclude<LogRange, "live" | "custom">, number>> = {
  "15m": 900_000,
  "1h": 3_600_000,
  "24h": 86_400_000,
};

function isLevel(value: string): value is LogLevel {
  return (LOG_LEVELS as readonly string[]).includes(value);
}

function isRange(value: string): value is LogRange {
  return value === "live" || value === "custom" || Object.hasOwn(RANGE_MS, value);
}

const text = (params: URLSearchParams, name: string) => params.get(name)?.trim() || undefined;

/** Reads the filters from the URL; anything unknown is ignored, never an error. */
export function parseLogFilters(params: URLSearchParams): LogFilters {
  const level = params.get("level");
  const range = params.get("range");
  const filters: LogFilters = {
    level: level !== null && isLevel(level) ? level : DEFAULT_LEVEL,
    range: range !== null && isRange(range) ? range : "live",
  };
  const channel = text(params, "channel");
  const endpoint = text(params, "endpoint");
  const event = text(params, "event");
  const search = text(params, "text");
  if (channel) filters.channel = channel;
  if (endpoint) filters.endpoint = endpoint;
  if (event) filters.event = event;
  if (search) filters.text = search;
  if (filters.range === "custom") {
    const from = text(params, "from");
    const to = text(params, "to");
    if (from) filters.from = from;
    if (to) filters.to = to;
  }
  return filters;
}

/** The URL for the filters: only what differs from the defaults. */
export function toLogSearchParams(filters: LogFilters): URLSearchParams {
  const params = new URLSearchParams();
  if (filters.level !== DEFAULT_LEVEL) params.set("level", filters.level);
  if (filters.channel) params.set("channel", filters.channel);
  if (filters.endpoint) params.set("endpoint", filters.endpoint);
  if (filters.event) params.set("event", filters.event);
  if (filters.text) params.set("text", filters.text);
  if (filters.range !== "live") params.set("range", filters.range);
  if (filters.range === "custom") {
    if (filters.from) params.set("from", filters.from);
    if (filters.to) params.set("to", filters.to);
  }
  return params;
}

/** Whether the filters differ from the defaults, which Clear filters restores. */
export function isFiltered(filters: LogFilters): boolean {
  return (
    filters.level !== DEFAULT_LEVEL ||
    filters.range !== "live" ||
    filters.channel !== undefined ||
    filters.endpoint !== undefined ||
    filters.event !== undefined ||
    filters.text !== undefined
  );
}

const validTime = (value: string | undefined) =>
  value !== undefined && !Number.isNaN(new Date(value).getTime()) ? value : undefined;

/** The engine's query for the filters; a range counts back from `now`. */
export function toLogQuery(filters: LogFilters, now: number): LogQuery {
  const query: LogQuery = { minLevel: filters.level };
  if (filters.range === "custom") {
    const from = validTime(filters.from);
    const to = validTime(filters.to);
    if (from) query.from = from;
    if (to) query.to = to;
  } else if (filters.range !== "live") {
    query.from = new Date(now - RANGE_MS[filters.range]).toISOString();
  }
  if (filters.channel) query.channelId = filters.channel;
  if (filters.endpoint) query.endpointId = filters.endpoint;
  if (filters.event) query.event = filters.event;
  if (filters.text) query.text = filters.text;
  return query;
}

/** Following needs a range with no end: one that ends in the past has nothing more to show. */
export function canFollow(filters: LogFilters): boolean {
  return !(filters.range === "custom" && filters.to);
}
