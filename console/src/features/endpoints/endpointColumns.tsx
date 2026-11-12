import type { EndpointSnapshot, EndpointState } from "@/api/types";
import type { DataGridColumn } from "@/components/DataGrid";
import { formatAge, formatBytes, formatCount } from "@/lib/format";
import { ENDPOINT_KINDS, endpointKindLabel, endpointUrgency } from "@/lib/status";
import { Clip } from "../shared-operate/Clip";
import { activityText } from "../shared-operate/channelRules";
import { type FitSpec, fitColumns } from "../shared-operate/fitColumns";
import { ShapeChip } from "../shared-operate/ShapeChip";
import { endpointShape } from "../shared-operate/stateShapes";

/** An endpoint with the labels of the channels that use it, so the list needs no second lookup. */
export interface EndpointRow extends EndpointSnapshot {
  channelLabels: readonly string[];
}

export const ENDPOINT_STATES: readonly EndpointState[] = [
  "error",
  "retrying",
  "disconnected",
  "connecting",
  "connected",
  "listening",
  "disabled",
];

export const ENDPOINT_STATE_OPTIONS = [
  { id: "all", label: "All states" },
  ...ENDPOINT_STATES.map((state) => ({ id: state, label: endpointShape(state).label })),
];

export const ENDPOINT_KIND_OPTIONS = [
  { id: "all", label: "All kinds" },
  ...ENDPOINT_KINDS.map((kind) => ({ id: kind, label: endpointKindLabel(kind) })),
];

/** The state chip and for how long, on one line. */
function StateCell({ endpoint }: { endpoint: EndpointRow }) {
  return (
    <>
      <ShapeChip shape={endpointShape(endpoint.state)} />{" "}
      <span>since {formatAge(endpoint.stateSince)}</span>
    </>
  );
}

function usedBy(endpoint: EndpointRow): string {
  const [first, ...rest] = endpoint.channelLabels;
  if (first === undefined) return "Not used";
  return rest.length === 0 ? first : `${first} and ${rest.length} more`;
}

type ColumnId =
  | "label"
  | "kind"
  | "summary"
  | "state"
  | "bytesIn"
  | "bytesOut"
  | "activity"
  | "streak"
  | "channels"
  | "error";

/**
 * Minimum widths in pixels and the order columns leave in as the grid narrows (first to leave
 * first): Last error, Used by, Failure streak, Kind, Bytes out, Bytes in, Last activity, then
 * Summary. Endpoint and State stay. At 1280 pixels, 1024 of content, everything to Last
 * activity shows on one line per row.
 */
const SPECS: readonly FitSpec<ColumnId>[] = [
  { id: "label", min: 192 },
  { id: "kind", min: 96, drop: 4 },
  { id: "summary", min: 150, drop: 8 },
  { id: "state", min: 232 },
  { id: "bytesIn", min: 90, drop: 6 },
  { id: "bytesOut", min: 100, drop: 5 },
  { id: "activity", min: 120, drop: 7 },
  { id: "streak", min: 110, drop: 3 },
  { id: "channels", min: 160, drop: 2 },
  { id: "error", min: 180, drop: 1 },
];

/** The actions column's width, which the columns share the rest with. */
const ACTIONS_PX = 40;

/** Name first, then what it points at, its state and traffic, and the last error. */
const ALL_COLUMNS: readonly (DataGridColumn<EndpointRow> & { id: ColumnId })[] = [
  {
    id: "label",
    header: "Endpoint",
    identifier: true,
    cell: (e) => <Clip text={e.label} />,
    sortValue: (e) => e.label.toLowerCase(),
  },
  {
    id: "kind",
    header: "Kind",
    width: 96,
    cell: (e) => <Clip text={endpointKindLabel(e.kind)} />,
    sortValue: (e) => e.kind,
  },
  { id: "summary", header: "Summary", cell: (e) => <Clip text={e.summary} /> },
  {
    id: "state",
    header: "State",
    width: 232,
    cell: (e) => <StateCell endpoint={e} />,
    sortValue: (e) => endpointUrgency(e.state),
  },
  {
    id: "bytesIn",
    header: "Bytes in",
    width: 90,
    align: "end",
    cell: (e) => formatBytes(e.counters.bytesIn),
    sortValue: (e) => e.counters.bytesIn,
  },
  {
    id: "bytesOut",
    header: "Bytes out",
    width: 100,
    align: "end",
    cell: (e) => formatBytes(e.counters.bytesOut),
    sortValue: (e) => e.counters.bytesOut,
  },
  {
    id: "activity",
    header: "Last activity",
    width: 120,
    cell: (e) => activityText(e.lastActivityAt),
    sortValue: (e) => (e.lastActivityAt === null ? undefined : Date.parse(e.lastActivityAt)),
  },
  {
    id: "streak",
    header: "Failure streak",
    width: 110,
    align: "end",
    cell: (e) => formatCount(e.consecutiveFailureCount),
    sortValue: (e) => e.consecutiveFailureCount,
  },
  { id: "channels", header: "Used by", cell: (e) => <Clip text={usedBy(e)} /> },
  {
    id: "error",
    header: "Last error",
    cell: (e) => <Clip text={e.lastError?.summary ?? "None"} />,
  },
];

/**
 * The columns for a grid of `width` pixels (null: not measured, all of them). Rows are one
 * line: text that does not fit ends in an ellipsis with the whole in its title.
 */
export function endpointColumnsFor(width: number | null): readonly DataGridColumn<EndpointRow>[] {
  const shown = fitColumns(SPECS, width === null ? null : width - ACTIONS_PX);
  return ALL_COLUMNS.filter((column) => shown.includes(column.id));
}
