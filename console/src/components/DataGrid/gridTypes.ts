import type { RowData } from "@tanstack/react-table";
import type { ReactNode } from "react";
import type { GridBulk } from "./SelectionBar";
import type { GridRowActions } from "./useRowMenu";

export interface DataGridColumn<Row extends RowData> {
  id: string;
  header: string;
  cell: (row: Row) => ReactNode;
  /**
   * The value to sort by; columns without one are not sortable. Rows whose value is
   * `undefined` stay at the bottom in either direction.
   */
  sortValue?: (row: Row) => string | number | undefined;
  /**
   * Sorts the list by this column when it first shows, instead of the engine's order. A
   * click on the header then turns the direction around. Set on one column at most.
   */
  defaultSort?: "asc" | "desc";
  /**
   * Sortable although the grid has no `sortValue` to sort by: the engine sorts, so the grid
   * only reports the header click (`sort` and `onSortChange` on the grid).
   */
  sortable?: boolean;
  /** Fixed width in pixels; other columns share the remaining space. */
  width?: number;
  /**
   * Share of the remaining space, relative to the other flexible columns (default 1), for a
   * column of running text next to short ones.
   */
  grow?: number;
  /**
   * Lets long text wrap onto more lines, for status and summary columns. Otherwise text that
   * does not fit ends in "…" and shows in full on hover.
   */
  wrap?: boolean;
  /** `end` for numbers and counts, so their digits line up. */
  align?: "end";
  /**
   * The identifier people use, such as a channel label or message id, styled as a link so it
   * is visible that rows open. Not a link itself: a click on the row or Enter opens it.
   */
  identifier?: boolean;
}

/** The column the engine sorts a server-driven grid by. */
export interface GridSort {
  /** A column's `id`. */
  id: string;
  descending: boolean;
}

/** Where a server-driven grid's rows sit in the engine's full answer, for assistive technology. */
export interface GridRowWindow {
  /** Zero-based position of the first row shown. */
  offset: number;
  /** Rows in the whole answer; `null` when the engine did not count them. */
  total: number | null;
}

export interface DataGridProps<Row extends RowData> {
  /** Accessible name of the grid. */
  label: string;
  /** Keep stable between renders (module constant or `useMemo`). */
  columns: readonly DataGridColumn<Row>[];
  /**
   * Keep stable between renders; the engine's order is shown until a column is sorted,
   * unless a column has a `defaultSort`.
   */
  rows: readonly Row[];
  getRowId: (row: Row) => string;
  /**
   * Opens a row's record: a click anywhere on the row, except on a control inside it or a
   * drag that selected text, or Enter on any of its cells. `order` holds the row IDs in
   * display order, for Previous and Next. Arrow keys only move focus.
   */
  onRowAction?: (row: Row, order: readonly string[]) => void;
  /**
   * The row whose record is open: tinted, with a bar on its left edge and `aria-current`.
   * When it becomes null while focus has nowhere to be (the side panel holding focus
   * closed), focus returns to that row.
   */
  openRowId?: string | null;
  /**
   * Called with the rows' IDs in display order whenever that order changes (a sort, a find, a
   * new answer), so a side panel's Previous and Next follow the list as shown: pass
   * `useOpenRecord().followOrder`. Keep it stable between renders.
   */
  onOrderChange?: (order: readonly string[]) => void;
  /**
   * A last column with each row's actions menu, also opened by right-click and Shift+F10. It
   * stays in view at the inline end when the grid scrolls sideways.
   */
  rowActions?: GridRowActions<Row>;
  /** Bulk actions: a check column, Space to toggle, select all, and a selection bar above. */
  bulk?: GridBulk;
  /**
   * For the main grid of a list page: takes the remaining height of its flex-column parent,
   * such as `Page`, instead of at most 70% of the window.
   */
  fill?: boolean;
  /**
   * Server-driven sorting: the engine sorts, so `rows` arrive in order and are shown as given.
   * `sort` is the column the rows are sorted by (`null` for the engine's default order); a click
   * on a sortable header, or Enter or Space on it, calls `onSortChange` with the next sort
   * (ascending, descending, then `null`; a column with `defaultSort: "desc"` starts descending).
   * Giving `onSortChange` turns this mode on, and `defaultSort` then only picks the first direction.
   */
  sort?: GridSort | null;
  onSortChange?: (sort: GridSort | null) => void;
  /**
   * The rows shown are the previous answer while a new one loads: the grid is marked busy
   * (`aria-busy`) and dimmed, and keeps its rows.
   */
  isLoading?: boolean;
  /** For a grid that shows one page of a longer answer: the full row count and the page's offset. */
  rowWindow?: GridRowWindow;
  /** The grid scrolls back to its first row whenever this value changes, such as a new page. */
  scrollResetKey?: string | number;
  /** Called when the grid scrolls away from its first row (`false`) and back to it (`true`). */
  onAtTopChange?: (atTop: boolean) => void;
}
