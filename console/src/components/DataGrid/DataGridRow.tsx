import type { RowData } from "@tanstack/react-table";
import { type CSSProperties, memo } from "react";
import { CheckMark } from "@/components/Checkbox";
import { type ActionItem, RowActions } from "@/components/RowActions";
import styles from "./DataGrid.module.css";
import { type CellHandlers, cellBehavior, GridCell } from "./GridCell";
import type { DataGridColumn } from "./gridTypes";

interface DataGridRowProps<Row extends RowData> {
  row: Row;
  rowId: string;
  /** Position in display order, from 0. */
  index: number;
  /** Offset from the top of the body, in pixels. */
  top: number;
  /** Rows of the full answer before the first one shown, for `aria-rowindex`. */
  rowOffset: number;
  columns: readonly DataGridColumn<Row>[];
  /** Column template and minimum width, shared by every row. */
  layout: CSSProperties;
  /** Selection state; null when the row may not be selected, absent without a check column. */
  isSelected: boolean | null | undefined;
  /** The row's record is open, beside or after the list. */
  isOpen: boolean;
  /** The column holding the grid's tab stop, when this row has it. */
  tabStopCol: number | undefined;
  handlers: CellHandlers;
  /** Measures the row's height, for grids with wrapping columns. */
  measureRef: ((node: HTMLElement | null) => void) | undefined;
  /** The grid ends with a row actions column. */
  actionsColumn: boolean;
  /** The row's identifier for its actions button; null when the row has no actions. */
  actionsLabel: string | null;
  /** The items of the row's actions menu while it is open; null while it is closed. */
  menuItems: readonly ActionItem[] | null;
  onMenuChange: (rowId: string, isOpen: boolean) => void;
}

const NO_ITEMS: readonly ActionItem[] = [];

/**
 * One data row. Text that does not fit a cell ends in "…" and shows in full on hover, unless
 * its column wraps. A count of zero shows as a muted `0`.
 */
function GridRow<Row extends RowData>(props: DataGridRowProps<Row>) {
  const { row, index, columns, isSelected, tabStopCol, handlers, measureRef } = props;
  const offset = isSelected === undefined ? 0 : 1;
  const cell = (col: number) => cellBehavior(index, col, tabStopCol, handlers);
  return (
    // biome-ignore lint/a11y/useSemanticElements lint/a11y/useFocusableInteractive: ARIA grid on divs (see GridCell); cells take focus, not rows.
    <div
      ref={measureRef}
      role="row"
      aria-rowindex={props.rowOffset + index + 2}
      aria-selected={isSelected ?? undefined}
      aria-current={props.isOpen || undefined}
      data-index={index}
      data-wrap={measureRef ? true : undefined}
      className={styles.row}
      style={{ ...props.layout, transform: `translateY(${props.top}px)` }}
    >
      {isSelected !== undefined && (
        <GridCell role="gridcell" className={styles.checkCell} {...cell(0)}>
          {isSelected !== null && <CheckMark state={isSelected ? "checked" : "unchecked"} />}
        </GridCell>
      )}
      {columns.map((column, col) => {
        const value = column.cell(row);
        return (
          <GridCell
            key={column.id}
            role="gridcell"
            className={styles.cell}
            data-align={column.align}
            data-wrap={column.wrap || undefined}
            data-zero={value === 0 || undefined}
            data-identifier={column.identifier || undefined}
            {...cell(col + offset)}
          >
            {value}
          </GridCell>
        );
      })}
      {props.actionsColumn && (
        <GridCell
          role="gridcell"
          className={`${styles.actionsCell} ${styles.actionsEnd}`}
          {...cell(columns.length + offset)}
        >
          {props.actionsLabel !== null && (
            <RowActions
              identifier={props.actionsLabel}
              items={props.menuItems ?? NO_ITEMS}
              isOpen={props.menuItems !== null}
              onOpenChange={(isOpen) => props.onMenuChange(props.rowId, isOpen)}
            />
          )}
        </GridCell>
      )}
    </div>
  );
}

/** One data row; memoized, so scrolling, focus moves and opening re-render only the rows that change. */
export const DataGridRow = memo(GridRow) as typeof GridRow;
