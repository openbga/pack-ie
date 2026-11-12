import type { ChannelKind, ChannelSnapshot } from "@/api/types";
import type { DataGridColumn } from "@/components/DataGrid";
import { StatusBadge } from "@/components/StatusBadge";
import { formatCount } from "@/lib/format";
import { channelKindLabel, channelUrgency, protocolLabel } from "@/lib/status";
import { ChannelStateChip } from "../shared-operate/ChannelStateChip";
import { Clip } from "../shared-operate/Clip";
import { activityText, endpointsText, pressureSummary } from "../shared-operate/channelRules";
import { type FitSpec, fitColumns } from "../shared-operate/fitColumns";
import { channelShape } from "../shared-operate/stateShapes";

/** The state filter's values: the state, or for a stopped channel why it stopped. */
export type ChannelStateKey =
  | "running"
  | "starting"
  | "retrying"
  | "degraded"
  | "paused"
  | "disabled"
  | "configuration"
  | "fatal"
  | "replaced"
  | "shutdown"
  | "store_unavailable"
  | "not_implemented";

export function channelStateKey(
  channel: Pick<ChannelSnapshot, "state" | "stopReason">,
): ChannelStateKey {
  return channel.state === "stopped" ? (channel.stopReason ?? "fatal") : channel.state;
}

const STATE_SAMPLES: Record<ChannelStateKey, Pick<ChannelSnapshot, "state" | "stopReason">> = {
  running: { state: "running", stopReason: null },
  starting: { state: "starting", stopReason: null },
  retrying: { state: "retrying", stopReason: null },
  degraded: { state: "degraded", stopReason: null },
  paused: { state: "stopped", stopReason: "paused" },
  disabled: { state: "stopped", stopReason: "disabled" },
  configuration: { state: "stopped", stopReason: "configuration" },
  fatal: { state: "stopped", stopReason: "fatal" },
  replaced: { state: "stopped", stopReason: "replaced" },
  shutdown: { state: "stopped", stopReason: "shutdown" },
  store_unavailable: { state: "stopped", stopReason: "store_unavailable" },
  not_implemented: { state: "stopped", stopReason: "not_implemented" },
};

/** Labels come from `stateShapes`, so the filter says what the chip says. */
export const CHANNEL_STATES: readonly { id: ChannelStateKey; label: string }[] = (
  Object.keys(STATE_SAMPLES) as ChannelStateKey[]
).map((id) => ({ id, label: channelShape(STATE_SAMPLES[id]).label }));

export const CHANNEL_STATE_OPTIONS = [{ id: "all", label: "All states" }, ...CHANNEL_STATES];

export const CHANNEL_KIND_OPTIONS: readonly { id: "all" | ChannelKind; label: string }[] = [
  { id: "all", label: "All kinds" },
  { id: "bridge", label: channelKindLabel("bridge") },
  { id: "message", label: channelKindLabel("message") },
];

function QueueCell({ channel }: { channel: ChannelSnapshot }) {
  const pressure = pressureSummary(channel);
  const attention = channel.queue?.needsAttention ?? 0;
  const text =
    channel.kind === "bridge"
      ? `${Math.round(channel.pressure * 100)}% of buffer`
      : `${formatCount(channel.queue?.depth ?? 0)} queued`;
  return (
    <span title={pressure.text}>
      {text}
      {attention > 0 && (
        <>
          {" "}
          <StatusBadge tone="danger">{`${formatCount(attention)} need attention`}</StatusBadge>
        </>
      )}
      {pressure.tone === "warning" && (
        <>
          {" "}
          <StatusBadge tone="warning">Filling up</StatusBadge>
        </>
      )}
    </span>
  );
}

const kindText = (c: ChannelSnapshot) =>
  c.protocol === null
    ? channelKindLabel(c.kind)
    : `${channelKindLabel(c.kind)} · ${protocolLabel(c.protocol)}`;

type ColumnId =
  | "label"
  | "kind"
  | "state"
  | "endpoints"
  | "messages"
  | "queue"
  | "activity"
  | "error";

/**
 * Widths in pixels, with the order columns leave in as the grid narrows (first to leave
 * first): Last error, Last activity, Kind, Endpoints, Messages, Queue. Channel and State stay.
 * Fixed widths are exact; Channel, Endpoints and Last error share what is left, with the
 * minimums given. At 1280 pixels, 1024 of content, every column but Last activity and Last
 * error shows.
 */
const SPECS: readonly FitSpec<ColumnId>[] = [
  { id: "label", min: 192 },
  { id: "kind", min: 120, drop: 3 },
  { id: "state", min: 180 },
  { id: "endpoints", min: 170, drop: 4 },
  { id: "messages", min: 150, drop: 5 },
  { id: "queue", min: 160, drop: 6 },
  { id: "activity", min: 120, drop: 2 },
  { id: "error", min: 160, drop: 1 },
];

/** The actions column's width, which the columns share the rest with. */
const ACTIONS_PX = 40;

const ALL_COLUMNS: readonly (DataGridColumn<ChannelSnapshot> & { id: ColumnId })[] = [
  {
    id: "label",
    header: "Channel",
    identifier: true,
    grow: 1,
    cell: (c) => <Clip text={c.label} />,
    sortValue: (c) => c.label.toLowerCase(),
  },
  {
    id: "kind",
    header: "Kind",
    width: 150,
    cell: (c) => <Clip text={kindText(c)} />,
    sortValue: (c) => c.kind,
  },
  {
    id: "state",
    header: "State",
    width: 168,
    wrap: true,
    cell: (c) => <ChannelStateChip channel={c} />,
    sortValue: (c) => channelUrgency(c),
  },
  {
    id: "endpoints",
    header: "Endpoints",
    grow: 1.5,
    cell: (c) => <Clip text={endpointsText(c)} />,
  },
  {
    id: "messages",
    header: "Messages in / out",
    width: 140,
    align: "end",
    cell: (c) => `${formatCount(c.counters.messagesIn)} / ${formatCount(c.counters.messagesOut)}`,
    sortValue: (c) => c.counters.messagesIn,
  },
  {
    id: "queue",
    header: "Queue / buffer",
    width: 150,
    wrap: true,
    cell: (c) => <QueueCell channel={c} />,
    sortValue: (c) => c.pressure,
  },
  {
    id: "activity",
    header: "Last activity",
    width: 120,
    cell: (c) => activityText(c.lastActivityAt),
    sortValue: (c) => (c.lastActivityAt === null ? undefined : Date.parse(c.lastActivityAt)),
  },
  {
    id: "error",
    header: "Last error",
    grow: 1,
    cell: (c) => <Clip text={c.lastError?.summary ?? "None"} />,
  },
];

/**
 * The columns for a grid of `width` pixels (null: not measured, all of them). Rows are one
 * line, text that does not fit ends in an ellipsis with the whole in its title; only the state
 * chip and the queue's badges may wrap.
 */
export function channelColumnsFor(
  width: number | null,
): readonly DataGridColumn<ChannelSnapshot>[] {
  const shown = fitColumns(SPECS, width === null ? null : width - ACTIONS_PX);
  return ALL_COLUMNS.filter((column) => shown.includes(column.id));
}
