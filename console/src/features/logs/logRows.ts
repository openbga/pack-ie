import type { LogEvent, LogFollowNotice } from "@/api/types";
import { formatCount } from "@/lib/format";

/** One log event in the list. `key` is unique across engine restarts, which restart `seq`. */
export interface EventRow {
  kind: "event";
  key: string;
  event: LogEvent;
}

/** A divider: the connection was lost and came back, with what that cost. */
interface NoticeRow {
  kind: "notice";
  key: string;
  notice: LogFollowNotice;
}

export type LogRow = EventRow | NoticeRow;

/** The divider's words; `clock` formats a time as it is shown in the list. */
export function noticeText(notice: LogFollowNotice, clock: (at: string) => string): string {
  const times = `Connection lost ${clock(notice.lostAt)} — reconnected ${clock(notice.backAt)}`;
  if (notice.missing === "restarted") {
    return `${times}. Engine restarted — earlier events are no longer available.`;
  }
  const filled =
    notice.filled === 0
      ? "no events were missed"
      : `${formatCount(notice.filled)} ${notice.filled === 1 ? "event" : "events"} filled in`;
  const gap =
    notice.missing === "gap" ? " Some events during the outage are no longer available." : "";
  return `${times}, ${filled}.${gap}`;
}
