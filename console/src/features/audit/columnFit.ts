import { type FitSpec, fitColumns } from "../shared-operate/fitColumns";

/** The columns of the Audit grid that a person can show or hide, besides the three that stay. */
export type AuditColumnId = "time" | "actor" | "origin" | "action" | "object" | "outcome";

/** The default columns, in display order: Origin is optional and always in the detail. */
export const DEFAULT_ORDER: readonly AuditColumnId[] = [
  "time",
  "actor",
  "action",
  "object",
  "outcome",
];

/** Every column the grid can show, in display order. */
export const ALL_ORDER: readonly AuditColumnId[] = [
  "time",
  "actor",
  "origin",
  "action",
  "object",
  "outcome",
];

/**
 * The width each column needs to read well, in pixels. Time, Actor, Origin and Outcome have this
 * as their width; Action and Object share what is left and wrap onto a second line, so a
 * column that fits keeps its whole label.
 *
 * A fixed column cannot size itself to its text, and the text depends on the font the system
 * has: where there is no Helvetica or SF the browser draws DejaVu Sans, about 15% wider. Time
 * and Outcome hold text that must stay whole (`05-12 12:00:00`, a Requested or Succeeded
 * badge), so their widths are what that text takes in Verdana, which is as wide as DejaVu Sans,
 * plus a few pixels; with an event open the list is only about 400 pixels wide at a 1024 pixel
 * window, and Time, Action (at least 128, the grid's own minimum) and Outcome must all fit in
 * it. Actor and Origin are cut with their full text on hover.
 */
export const COLUMN_WIDTH: Record<AuditColumnId, number> = {
  time: 134,
  actor: 112,
  origin: 112,
  action: 150,
  object: 176,
  outcome: 138,
};

/**
 * Never dropped, however narrow the grid: when, what and how it ended. Time, Action and
 * Outcome are the core of the review; who did it is the next to go, after the object.
 */
const DROP: Partial<Record<AuditColumnId, number>> = { object: 1, actor: 2 };

/**
 * What the grid spends beside its columns, in pixels: its borders and a vertical scrollbar.
 * Taken off the width of the list before columns are counted.
 */
const GRID_CHROME = 20;

const SPECS: readonly FitSpec<AuditColumnId>[] = DEFAULT_ORDER.map((id) => {
  const drop = DROP[id];
  return drop === undefined ? { id, min: COLUMN_WIDTH[id] } : { id, min: COLUMN_WIDTH[id], drop };
});

/** The default columns that fit a list `width` pixels wide; `null` (not measured) keeps them all. */
export function fittingColumns(width: number | null): readonly AuditColumnId[] {
  return fitColumns(SPECS, width === null ? null : width - GRID_CHROME);
}
