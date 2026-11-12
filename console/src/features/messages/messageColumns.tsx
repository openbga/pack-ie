import { ArrowDownToLine, ArrowLeft, ArrowRight } from "lucide-react";
import type { MessageDirection, MessageSummary } from "@/api/types";
import type { DataGridColumn } from "@/components/DataGrid";
import { StatusBadge } from "@/components/StatusBadge";
import { Timestamp } from "@/components/Timestamp";
import { formatBytes } from "@/lib/format";
import { ALWAYS, COLUMN_WIDTH, DEFAULT_ORDER, type DefaultColumnId } from "./columnFit";
import { DeliveryChips } from "./DeliveryChips";
import styles from "./messageColumns.module.css";
import { directionShortText, directionText, messageStateStatus } from "./messageWording";

const NOT_PROVIDED = "Not provided";

/** The identifier people look for: the control id, else the sample id. */
export const identifierOf = (message: MessageSummary): string | null =>
  message.controlId ?? message.sampleId;

function State({ message }: { message: MessageSummary }) {
  const status = messageStateStatus(message.state);
  return <StatusBadge tone={status.tone}>{status.label}</StatusBadge>;
}

const DIRECTION_ICONS: Record<MessageDirection, typeof ArrowRight> = {
  inbound: ArrowDownToLine,
  a_to_b: ArrowRight,
  b_to_a: ArrowLeft,
};

/** An arrow and a few words; the full wording is the title. */
function Direction({ direction }: { direction: MessageDirection }) {
  const Icon = DIRECTION_ICONS[direction];
  return (
    <span className={styles.direction} title={directionText(direction)}>
      <Icon aria-hidden="true" className={styles.arrow} />
      <span>{directionShortText(direction)}</span>
    </span>
  );
}

/**
 * The default columns of spec §6. Column ids of the sortable ones are the engine's sort
 * fields, so a click on a header sorts on the engine. Widths come from `columnFit`: the
 * columns with a fixed width, and the four that share the rest (channel, delivery summary,
 * identifier, patient ID) in proportion to the width each needs. Names and identifiers
 * truncate with "…" and show in full on hover; none wraps letter by letter.
 */
const DEFAULT_COLUMNS: Record<DefaultColumnId, DataGridColumn<MessageSummary>> = {
  receivedAt: {
    id: "receivedAt",
    header: "Received",
    identifier: true,
    sortable: true,
    defaultSort: "desc",
    width: COLUMN_WIDTH.receivedAt,
    cell: (message) => <Timestamp value={message.receivedAt} compact seconds />,
  },
  channel: {
    id: "channel",
    header: "Channel",
    sortable: true,
    grow: 1,
    cell: (message) => message.channelLabel,
  },
  direction: {
    id: "direction",
    header: "Direction",
    width: COLUMN_WIDTH.direction,
    cell: (message) => <Direction direction={message.direction} />,
  },
  state: {
    id: "state",
    header: "Message state",
    sortable: true,
    width: COLUMN_WIDTH.state,
    cell: (message) => <State message={message} />,
  },
  deliveries: {
    id: "deliveries",
    header: "Delivery summary",
    wrap: true,
    grow: COLUMN_WIDTH.deliveries / COLUMN_WIDTH.channel,
    cell: (message) => <DeliveryChips message={message} />,
  },
  messageType: {
    id: "messageType",
    header: "Message type",
    sortable: true,
    width: COLUMN_WIDTH.messageType,
    cell: (message) => message.messageType ?? NOT_PROVIDED,
  },
  identifier: {
    id: "identifier",
    header: "Identifier",
    grow: 1,
    cell: (message) => identifierOf(message) ?? NOT_PROVIDED,
  },
  patientId: {
    id: "patientId",
    header: "Patient ID",
    grow: 1,
    cell: (message) => message.patientId ?? NOT_PROVIDED,
  },
  sizeBytes: {
    id: "sizeBytes",
    header: "Size",
    sortable: true,
    align: "end",
    width: COLUMN_WIDTH.sizeBytes,
    cell: (message) => formatBytes(message.sizeBytes),
  },
};

/** Columns a person can add through the column chooser (spec §6). */
const OPTIONAL_COLUMNS: readonly DataGridColumn<MessageSummary>[] = [
  { id: "messageId", header: "Message ID", width: 296, cell: (message) => message.id },
  {
    id: "sourcePeer",
    header: "Source peer",
    cell: (message) => message.sourcePeer ?? NOT_PROVIDED,
  },
  {
    id: "configVersion",
    header: "Configuration version",
    align: "end",
    width: 168,
    cell: (message) => message.configVersion,
  },
  {
    id: "resentFrom",
    header: "Resent from",
    width: 220,
    cell: (message) => message.resentFrom ?? "Not a resend",
  },
];

const ALWAYS_SHOWN: ReadonlySet<string> = new Set(ALWAYS);

const CHOOSABLE: readonly DataGridColumn<MessageSummary>[] = [
  ...DEFAULT_ORDER.map((id) => DEFAULT_COLUMNS[id]),
  ...OPTIONAL_COLUMNS,
].filter((column) => !ALWAYS_SHOWN.has(column.id));

/** What the column chooser offers: the default columns that can go, then the optional ones. */
export const COLUMN_CHOICES = CHOOSABLE.map(({ id, header }) => ({ id, label: header }));

const ALL_COLUMNS: readonly DataGridColumn<MessageSummary>[] = [
  ...DEFAULT_ORDER.map((id) => DEFAULT_COLUMNS[id]),
  ...OPTIONAL_COLUMNS,
];

/**
 * The grid's columns, default ones first then the optional ones, each in its own order: the
 * `ids` asked for, and always the columns that stay.
 */
export function messageColumns(ids: readonly string[]): DataGridColumn<MessageSummary>[] {
  return ALL_COLUMNS.filter((column) => ALWAYS_SHOWN.has(column.id) || ids.includes(column.id));
}

/** The chooser's ids among `ids`: the columns shown that a person can remove. */
export const choosableIn = (ids: readonly string[]): string[] =>
  CHOOSABLE.filter((column) => ids.includes(column.id)).map((column) => column.id);
