import { type FocusEvent, useMemo, useRef, useState } from "react";
import { useNavigate } from "react-router";
import type { DataGridColumn } from "@/components/DataGrid";
import { DataGrid } from "@/components/DataGrid";
import { useElementWidth } from "@/lib/useElementWidth";
import styles from "./ChannelTable.module.css";
import {
  ActivityCell,
  ChannelCell,
  EndpointsCell,
  ErrorCell,
  QueueCell,
  RetryCell,
  SessionCell,
  StateCell,
  TrafficCell,
} from "./channelCells";
import { type ChannelColumnId, endpointsWidth, FIXED_WIDTH, visibleColumns } from "./columnFit";
import type { ChannelRow } from "./rank";

type Column = DataGridColumn<ChannelRow> & { id: ChannelColumnId };

/**
 * Every column wraps: bridges show a line per side, so rows are one to three lines tall.
 * Endpoints has the width the fit gives it (never under its minimum); the cells inside it do
 * not wrap, their names truncate.
 */
const columnsFor = (endpoints: number): readonly Column[] => [
  {
    id: "channel",
    header: "Channel",
    identifier: true,
    wrap: true,
    grow: 1.2,
    cell: (r) => <ChannelCell row={r} />,
  },
  {
    id: "state",
    header: "State",
    wrap: true,
    width: FIXED_WIDTH.state,
    cell: (r) => <StateCell row={r} />,
  },
  {
    id: "endpoints",
    header: "Endpoints",
    wrap: true,
    width: endpoints,
    cell: (r) => <EndpointsCell channel={r.channel} />,
  },
  {
    id: "traffic",
    header: "Traffic",
    wrap: true,
    width: FIXED_WIDTH.traffic,
    cell: (r) => <TrafficCell channel={r.channel} />,
  },
  {
    id: "activity",
    header: "Last activity",
    wrap: true,
    width: FIXED_WIDTH.activity,
    cell: (r) => <ActivityCell channel={r.channel} />,
  },
  {
    id: "queue",
    header: "Queue",
    wrap: true,
    width: FIXED_WIDTH.queue,
    cell: (r) => <QueueCell row={r} />,
  },
  {
    id: "session",
    header: "Session",
    wrap: true,
    width: FIXED_WIDTH.session,
    cell: (r) => <SessionCell channel={r.channel} />,
  },
  {
    id: "retry",
    header: "Retry",
    wrap: true,
    width: FIXED_WIDTH.retry,
    cell: (r) => <RetryCell channel={r.channel} />,
  },
  {
    id: "error",
    header: "Last error",
    wrap: true,
    grow: 1.5,
    cell: (r) => <ErrorCell channel={r.channel} />,
  },
];

/** Rows in `order` first (ids that are gone are skipped), then any new rows, in rank order. */
export function applyOrder(rows: readonly ChannelRow[], order: readonly string[]): ChannelRow[] {
  const byId = new Map(rows.map((row) => [row.channel.id, row]));
  const held = order.flatMap((id) => byId.get(id) ?? []);
  const known = new Set(order);
  return [...held, ...rows.filter((row) => !known.has(row.channel.id))];
}

interface Hold {
  order: string[];
  pointer: boolean;
  focus: boolean;
}

/**
 * Rows follow the ranking, which changes as channels do, but never under the operator's hand:
 * while the pointer is over the table or focus is in it, rows keep their places (their data
 * still updates), so the focused or clicked row stays the same channel. Leaving releases it.
 */
function useHeldOrder(rows: readonly ChannelRow[]) {
  const [hold, setHold] = useState<Hold | null>(null);
  const ids = rows.map((row) => row.channel.id);
  function set(source: "pointer" | "focus", on: boolean) {
    setHold((current) => {
      const next: Hold = {
        order: current?.order ?? ids,
        pointer: current?.pointer ?? false,
        focus: current?.focus ?? false,
        [source]: on,
      };
      return next.pointer || next.focus ? next : null;
    });
  }
  return {
    rows: useMemo(() => (hold === null ? rows : applyOrder(rows, hold.order)), [rows, hold]),
    handlers: {
      onPointerEnter: () => set("pointer", true),
      onPointerLeave: () => set("pointer", false),
      onFocus: () => set("focus", true),
      onBlur: (event: FocusEvent<HTMLElement>) => {
        if (!event.currentTarget.contains(event.relatedTarget)) set("focus", false);
      },
    },
  };
}

const getRowId = (row: ChannelRow) => row.channel.id;

/**
 * The dashboard's one channel table (operator-console.md section 3): rows in rank order, a
 * row opens its channel, and columns drop out from the right as the table narrows (Retry,
 * Last error, Session, Last activity, Traffic, Queue), never Channel, State or Endpoints.
 */
export function ChannelTable({ rows }: { rows: readonly ChannelRow[] }) {
  const navigate = useNavigate();
  const frame = useRef<HTMLDivElement>(null);
  const width = useElementWidth(frame);
  const shown = useMemo(() => visibleColumns(width), [width]);
  const columns = useMemo(
    () => columnsFor(endpointsWidth(width, shown)).filter((column) => shown.includes(column.id)),
    [width, shown],
  );
  const held = useHeldOrder(rows);
  return (
    <div ref={frame} className={styles.frame} {...held.handlers}>
      <DataGrid
        label="Channels"
        columns={columns}
        rows={held.rows}
        getRowId={getRowId}
        onRowAction={(row) => void navigate(`/channels/${row.channel.id}`)}
      />
    </div>
  );
}
