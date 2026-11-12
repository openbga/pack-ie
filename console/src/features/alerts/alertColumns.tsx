import { Link } from "react-router";
import type { Alert } from "@/api/types";
import type { DataGridColumn } from "@/components/DataGrid";
import { StatusBadge } from "@/components/StatusBadge";
import { Timestamp } from "@/components/Timestamp";
import { alertSeverityLabel, alertTone } from "@/lib/status";
import { enginePageOf } from "../shared-operate/alertTarget";
import { type FitSpec, fitColumns } from "../shared-operate/fitColumns";
import styles from "./alertColumns.module.css";

/** An alert with the names of what it concerns. */
export interface AlertRow extends Alert {
  channelLabel: string | null;
  endpointLabel: string | null;
}

export const SEVERITY_RANK = { critical: 2, warning: 1, info: 0 } as const;

function concernsText(alert: AlertRow): string {
  const parts = [
    alert.channelLabel === null ? null : `Channel ${alert.channelLabel}`,
    alert.endpointLabel === null ? null : `Endpoint ${alert.endpointLabel}`,
  ].filter((part) => part !== null);
  return parts.length === 0 ? "The engine" : parts.join(" · ");
}

/** An alert about the engine itself, with the page that deals with it when there is one. */
function EngineConcern({ alert }: { alert: AlertRow }) {
  const page = enginePageOf(alert);
  if (page === null) return "The engine";
  return (
    <span>
      The engine · <Link to={page.to}>{page.label}</Link>
    </span>
  );
}

/** The channel and endpoint an alert concerns on one line, each linking to its page. */
function Concerns({ alert }: { alert: AlertRow }) {
  if (alert.channelId === null && alert.endpointId === null) return <EngineConcern alert={alert} />;
  return (
    <span title={concernsText(alert)}>
      {alert.channelId !== null && (
        <>
          Channel <Link to={`/channels/${alert.channelId}`}>{alert.channelLabel}</Link>
        </>
      )}
      {alert.channelId !== null && alert.endpointId !== null && " · "}
      {alert.endpointId !== null && (
        <>
          Endpoint <Link to={`/endpoints/${alert.endpointId}`}>{alert.endpointLabel}</Link>
        </>
      )}
    </span>
  );
}

/** What happened, and under it, muted, what the engine says about it when it says more. */
function Summary({ alert }: { alert: AlertRow }) {
  return (
    <>
      <span className={styles.title}>{alert.title}</span>
      {alert.detail !== "" && " "}
      {alert.detail !== "" && (
        <span className={styles.detail} title={alert.detail}>
          {alert.detail}
        </span>
      )}
    </>
  );
}

type ColumnId =
  | "severity"
  | "title"
  | "next"
  | "state"
  | "concerns"
  | "first"
  | "last"
  | "occurrences";

/**
 * Minimum widths in pixels and the order columns leave in as the grid narrows (first to leave
 * first): Last seen, First seen, Concerns. Severity, Alert, Next step, State and Seen stay.
 * At 1280 pixels, 1024 of content, every column shows.
 */
const SPECS: readonly FitSpec<ColumnId>[] = [
  { id: "severity", min: 110 },
  { id: "title", min: 192 },
  { id: "next", min: 130 },
  { id: "state", min: 90 },
  { id: "concerns", min: 160, drop: 3 },
  { id: "first", min: 120, drop: 2 },
  { id: "last", min: 120, drop: 1 },
  { id: "occurrences", min: 80 },
];

/**
 * Severity, what happened (with its detail underneath), what to do about it straight after,
 * then state, what it concerns, when it began and last showed, and how often. Rows are as tall
 * as the summary and the next step need; what it concerns is one line, truncated, whole in its
 * title.
 */
const ALL_COLUMNS: readonly (DataGridColumn<AlertRow> & { id: ColumnId })[] = [
  {
    id: "severity",
    header: "Severity",
    width: 110,
    cell: (a) => (
      <StatusBadge tone={alertTone(a.severity)}>{alertSeverityLabel(a.severity)}</StatusBadge>
    ),
    sortValue: (a) => SEVERITY_RANK[a.severity],
  },
  {
    id: "title",
    header: "Alert",
    identifier: true,
    wrap: true,
    grow: 1.5,
    cell: (a) => <Summary alert={a} />,
    sortValue: (a) => a.title.toLowerCase(),
  },
  { id: "next", header: "Next step", wrap: true, grow: 1.5, cell: (a) => a.nextAction },
  {
    id: "state",
    header: "State",
    width: 90,
    cell: (a) => (a.state === "open" ? "Open" : "Cleared"),
    sortValue: (a) => a.state,
  },
  {
    id: "concerns",
    header: "Concerns",
    cell: (a) => <Concerns alert={a} />,
    sortValue: concernsText,
  },
  {
    id: "first",
    header: "First seen",
    width: 120,
    cell: (a) => <Timestamp value={a.firstSeenAt} compact />,
    sortValue: (a) => Date.parse(a.firstSeenAt),
  },
  {
    id: "last",
    header: "Last seen",
    width: 120,
    cell: (a) => <Timestamp value={a.lastSeenAt} compact />,
    sortValue: (a) => Date.parse(a.lastSeenAt),
  },
  {
    id: "occurrences",
    header: "Seen",
    width: 80,
    align: "end",
    cell: (a) => (a.occurrences === null ? "Not counted" : `${a.occurrences}×`),
    sortValue: (a) => a.occurrences ?? undefined,
  },
];

/** The columns for a grid of `width` pixels (null: not measured, all of them). */
export function alertColumnsFor(width: number | null): readonly DataGridColumn<AlertRow>[] {
  const shown = fitColumns(SPECS, width);
  return ALL_COLUMNS.filter((column) => shown.includes(column.id));
}
