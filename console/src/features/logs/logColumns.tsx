import { useMemo } from "react";
import type { LogEvent } from "@/api/types";
import type { DataGridColumn } from "@/components/DataGrid";
import { StatusBadge } from "@/components/StatusBadge";
import { Timestamp } from "@/components/Timestamp";
import { useFormatTime } from "@/lib/format";
import { logLevelTone } from "@/lib/status";
import styles from "./LogsPage.module.css";
import { LEVEL_LABELS } from "./logLevels";
import { type LogRow, noticeText } from "./logRows";

/** Ids to labels for the channels and endpoints events name. */
export interface LogLabels {
  channels: ReadonlyMap<string, string>;
  endpoints: ReadonlyMap<string, string>;
}

const NONE = "None";

function Level({ event }: { event: LogEvent }) {
  return <StatusBadge tone={logLevelTone(event.level)}>{LEVEL_LABELS[event.level]}</StatusBadge>;
}

/** Errors and warnings read heavier than the rest of the stream. */
function Message({ event }: { event: LogEvent }) {
  const urgent = event.level === "error" || event.level === "warn";
  return <span className={urgent ? styles.urgent : undefined}>{event.message}</span>;
}

/** The divider that says the connection was lost: a notice, in words, never color alone. */
function NoticeMessage({ row }: { row: Extract<LogRow, { kind: "notice" }> }) {
  const formatTime = useFormatTime();
  return (
    <span className={styles.notice}>
      {noticeText(row.notice, (at) => formatTime(at, { timeOnly: true, seconds: true }))}
    </span>
  );
}

const named = (labels: ReadonlyMap<string, string>, id: string | null) =>
  id === null ? NONE : (labels.get(id) ?? id);

/** An event's cell; a divider has none. */
function forEvent(cell: (event: LogEvent) => React.ReactNode) {
  return (row: LogRow) => (row.kind === "event" ? cell(row.event) : null);
}

/**
 * The columns of the log: time to the millisecond, level (a badge, so never color alone), the
 * event name, channel, endpoint and the message, which ends in "…" when long and shows in full
 * on hover and in the event's panel. A divider row has its text in the message column. Keep
 * `labels` stable between renders, or every row redraws.
 */
export function useLogColumns(labels: LogLabels): readonly DataGridColumn<LogRow>[] {
  return useMemo(
    () => [
      {
        id: "time",
        header: "Time",
        identifier: true,
        width: 200,
        cell: (row) => (
          <Timestamp
            value={row.kind === "event" ? row.event.at : row.notice.backAt}
            millis
            zone={false}
          />
        ),
      },
      {
        id: "level",
        header: "Level",
        width: 112,
        cell: (row) =>
          row.kind === "event" ? (
            <Level event={row.event} />
          ) : (
            <StatusBadge tone="info">Notice</StatusBadge>
          ),
      },
      {
        id: "event",
        header: "Event",
        width: 220,
        cell: forEvent((event) => <span className={styles.eventName}>{event.event ?? ""}</span>),
      },
      {
        id: "channel",
        header: "Channel",
        cell: forEvent((event) => named(labels.channels, event.channelId)),
      },
      {
        id: "endpoint",
        header: "Endpoint",
        cell: forEvent((event) => named(labels.endpoints, event.endpointId)),
      },
      {
        id: "message",
        header: "Message",
        grow: 4,
        cell: (row) =>
          row.kind === "event" ? <Message event={row.event} /> : <NoticeMessage row={row} />,
      },
    ],
    [labels],
  );
}
