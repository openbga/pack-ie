import { createContext, useCallback, useContext } from "react";

/**
 * An IANA time zone to show times in, or `null` for this computer's own zone (the engine runs
 * on the same computer). Tests provide one so output does not depend on the machine.
 */
export const LabTimeZoneContext = createContext<string | null>(null);

interface TimeOptions {
  /** Adds seconds, where events within one minute must be told apart (the audit trail). */
  seconds?: boolean;
  /** Adds milliseconds (and seconds), where events within one second must be told apart. */
  millis?: boolean;
  /** Leaves out the date when it is the same day as `today` in the zone: `14:03 CDT`. */
  today?: Date;
  /**
   * For grid cells: `10-12 09:03`, or just `09:03` for `today`, with no year and no zone. Show
   * the full text (`formatTimestamp` without `compact`) as the cell's title.
   */
  compact?: boolean;
  /** Names the zone after the time; on unless `compact` or `timeOnly`. */
  zone?: boolean;
  /** Only the time of day, `14:03:05`, for a line that says when something happened. */
  timeOnly?: boolean;
}

const formatters = new Map<string, Intl.DateTimeFormat>();

function formatterFor(timeZone: string | undefined, detail: "minute" | "second" | "milli") {
  const key = `${timeZone}|${detail}`;
  let formatter = formatters.get(key);
  if (formatter === undefined) {
    formatter = new Intl.DateTimeFormat("en-US", {
      timeZone,
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
      second: detail === "minute" ? undefined : "2-digit",
      fractionalSecondDigits: detail === "milli" ? 3 : undefined,
      hourCycle: "h23",
      timeZoneName: "short",
    });
    formatters.set(key, formatter);
  }
  return formatter;
}

function zonedParts(
  date: Date,
  timeZone: string | undefined,
  detail: "minute" | "second" | "milli",
) {
  const parts = formatterFor(timeZone, detail).formatToParts(date);
  const part = (type: Intl.DateTimeFormatPartTypes) =>
    parts.find((each) => each.type === type)?.value ?? "";
  let time = `${part("hour")}:${part("minute")}`;
  if (detail !== "minute") time += `:${part("second")}`;
  if (detail === "milli") time += `.${part("fractionalSecond")}`;
  return {
    day: `${part("year")}-${part("month")}-${part("day")}`,
    time,
    zoneName: part("timeZoneName"),
  };
}

function zonedText(date: Date, timeZone: string | undefined, options: TimeOptions): string {
  const { seconds = false, millis = false, today, compact = false, timeOnly = false } = options;
  const detail = millis ? "milli" : seconds ? "second" : "minute";
  const { day, time, zoneName } = zonedParts(date, timeZone, detail);
  if (timeOnly) return time;
  const sameDay = today !== undefined && zonedParts(today, timeZone, detail).day === day;
  if (compact) return sameDay ? time : `${day.slice(5)} ${time}`;
  const withZone = (options.zone ?? true) ? `${time} ${zoneName}` : time;
  return sameDay ? withZone : `${day} ${withZone}`;
}

/**
 * An engine timestamp (RFC 3339) as `2026-10-12 09:03 CDT` in `timeZone`, always naming the
 * zone so it is never ambiguous. A zone this web engine does not know falls back to UTC; a
 * value that does not parse is shown as received.
 */
export function formatTimestamp(
  timestamp: string,
  timeZone: string | null,
  options: TimeOptions = {},
): string {
  const date = new Date(timestamp);
  if (Number.isNaN(date.getTime())) return timestamp;
  try {
    return zonedText(date, timeZone ?? undefined, options);
  } catch {
    return zonedText(date, "UTC", options);
  }
}

/** Formats engine timestamps in the display time zone. */
export function useFormatTime(): (timestamp: string, options?: TimeOptions) => string {
  const timeZone = useContext(LabTimeZoneContext);
  return useCallback(
    (timestamp: string, options?: TimeOptions) => formatTimestamp(timestamp, timeZone, options),
    [timeZone],
  );
}

const units = ["B", "KB", "MB", "GB", "TB"];

/** `1.4 MB`; whole bytes below 1 KB. Binary units, one decimal. */
export function formatBytes(bytes: number): string {
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return unit === 0 ? `${bytes} B` : `${value.toFixed(1)} ${units[unit]}`;
}

/** `12,345` with thousands separators. */
export function formatCount(count: number): string {
  return new Intl.NumberFormat("en-US").format(count);
}

/** `3 min ago`, `2 h ago`, `5 d ago`; `just now` under 5 seconds. */
export function formatAge(timestamp: string, now: number = Date.now()): string {
  const seconds = Math.max(0, Math.round((now - new Date(timestamp).getTime()) / 1000));
  if (Number.isNaN(seconds)) return timestamp;
  if (seconds < 5) return "just now";
  if (seconds < 60) return `${seconds} s ago`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)} min ago`;
  if (seconds < 86_400) return `${Math.floor(seconds / 3600)} h ago`;
  return `${Math.floor(seconds / 86_400)} d ago`;
}

/** `3 d 4 h`, `12 min`; for uptime and durations. */
export function formatDuration(seconds: number): string {
  const days = Math.floor(seconds / 86_400);
  const hours = Math.floor((seconds % 86_400) / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  if (days > 0) return `${days} d ${hours} h`;
  if (hours > 0) return `${hours} h ${minutes} min`;
  return `${Math.max(minutes, 0)} min`;
}
