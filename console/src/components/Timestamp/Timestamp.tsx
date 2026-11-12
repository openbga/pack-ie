import { useFormatTime } from "@/lib/format";

interface TimestampProps {
  /** An engine timestamp (RFC 3339). */
  value: string;
  /** Adds seconds, where events within one minute must be told apart. */
  seconds?: boolean;
  /** Adds milliseconds, where events within one second must be told apart. */
  millis?: boolean;
  /**
   * For narrow grid cells: no year and no zone (`10-12 09:03:05`). The full time, with its
   * zone, is the element's title.
   */
  compact?: boolean;
  /** Names the zone after the time; on unless `compact`. */
  zone?: boolean;
}

/** An engine timestamp in the display time zone, with the zone named unless `compact`. */
export function Timestamp({ value, seconds, millis, compact, zone }: TimestampProps) {
  const formatTime = useFormatTime();
  return (
    <time dateTime={value} title={compact ? formatTime(value, { seconds: true }) : undefined}>
      {formatTime(value, { seconds, millis, compact, zone })}
    </time>
  );
}
