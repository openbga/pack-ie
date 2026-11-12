import { type FitSpec, fitColumns } from "../shared-operate/fitColumns";

/** The default columns of the Messages grid (operator-console.md section 6), in display order. */
export type DefaultColumnId =
  | "receivedAt"
  | "channel"
  | "direction"
  | "state"
  | "deliveries"
  | "messageType"
  | "identifier"
  | "patientId"
  | "sizeBytes";

/**
 * The width each default column needs to read well, in pixels. A column with a fixed width in
 * the grid has exactly this; the others (channel, deliveries, identifier, patient ID) share
 * what is left, in proportion to these, so each gets at least this much whenever the columns
 * chosen by `fittingColumns` are the ones shown.
 */
export const COLUMN_WIDTH: Record<DefaultColumnId, number> = {
  receivedAt: 124,
  channel: 128,
  direction: 84,
  state: 136,
  deliveries: 192,
  messageType: 136,
  identifier: 128,
  patientId: 128,
  sizeBytes: 76,
};

/** Never dropped, however narrow the grid: what a person scans a list for. */
export const ALWAYS: readonly DefaultColumnId[] = [
  "receivedAt",
  "channel",
  "state",
  "deliveries",
  "identifier",
];

/**
 * The order columns are dropped in as the grid narrows, first dropped first: from the right
 * (Size, Patient ID, Message type, Direction). The column chooser brings any of them back, and
 * the grid then scrolls sideways, which is a choice rather than the default.
 */
export const DROP_ORDER: readonly DefaultColumnId[] = [
  "sizeBytes",
  "patientId",
  "messageType",
  "direction",
];

/** Every default column, in display order. */
export const DEFAULT_ORDER: readonly DefaultColumnId[] = [
  "receivedAt",
  "channel",
  "direction",
  "state",
  "deliveries",
  "messageType",
  "identifier",
  "patientId",
  "sizeBytes",
];

/**
 * What the grid spends beside its columns, in pixels: the row actions column, the borders and a
 * scrollbar. Taken off the width of the grid's frame before columns are counted.
 */
export const GRID_CHROME = 58;

const SPECS: readonly FitSpec<DefaultColumnId>[] = DEFAULT_ORDER.map((id) => {
  const place = DROP_ORDER.indexOf(id);
  return place === -1
    ? { id, min: COLUMN_WIDTH[id] }
    : { id, min: COLUMN_WIDTH[id], drop: place + 1 };
});

/**
 * The default columns that fit a frame of `width` pixels. `null` (width not measured) keeps
 * them all.
 */
export function fittingColumns(width: number | null): readonly DefaultColumnId[] {
  return fitColumns(SPECS, width === null ? null : width - GRID_CHROME);
}
