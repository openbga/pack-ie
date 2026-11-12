import type { RowData } from "@tanstack/react-table";
import { ArrowDown, ArrowUp, ArrowUpDown } from "lucide-react";
import type { CSSProperties } from "react";
import { VisuallyHidden } from "react-aria-components";
import { CheckMark } from "@/components/Checkbox";
import styles from "./DataGrid.module.css";
import { type CellHandlers, cellBehavior, GridCell } from "./GridCell";
import { HEADER_ROW } from "./gridKeyboard";
import type { DataGridColumn } from "./gridTypes";

type SortDirection = "asc" | "desc" | false;

interface DataGridHeaderProps<Row extends RowData> {
  columns: readonly DataGridColumn<Row>[];
  /** Column template and minimum width, shared by every row. */
  layout: CSSProperties;
  /** State of the select-all cell; absent when the grid has no selection. */
  selectAll: "checked" | "mixed" | "unchecked" | undefined;
  /** The grid ends with a row actions column. */
  actionsColumn: boolean;
  sortDirection: (columnId: string) => SortDirection;
  /** The column holding the grid's tab stop, when the header row has it. */
  tabStopCol: number | undefined;
  handlers: CellHandlers;
}

/** The sticky header row: select all, and column headers that sort on Enter, Space or click. */
export function DataGridHeader<Row extends RowData>({
  columns,
  layout,
  selectAll,
  actionsColumn,
  sortDirection,
  tabStopCol,
  handlers,
}: DataGridHeaderProps<Row>) {
  const offset = selectAll === undefined ? 0 : 1;
  const cell = (col: number) => cellBehavior(HEADER_ROW, col, tabStopCol, handlers);
  return (
    // biome-ignore lint/a11y/useSemanticElements lint/a11y/useFocusableInteractive: ARIA grid on divs (see GridCell); cells take focus, not rows.
    <div role="row" aria-rowindex={1} className={styles.headerRow} style={layout}>
      {selectAll !== undefined && (
        <GridCell role="columnheader" className={styles.headerCell} {...cell(0)}>
          <CheckMark state={selectAll} />
          <VisuallyHidden>Select all rows</VisuallyHidden>
        </GridCell>
      )}
      {columns.map((column, index) => {
        const sortable = column.sortValue !== undefined || column.sortable === true;
        const direction = sortDirection(column.id);
        return (
          <GridCell
            key={column.id}
            role="columnheader"
            aria-sort={sortable ? ariaSort(direction) : undefined}
            className={styles.headerCell}
            data-sortable={sortable || undefined}
            data-align={column.align}
            {...cell(index + offset)}
          >
            <span className={styles.headerText} title={column.header}>
              {column.header}
            </span>
            {sortable && <SortIcon direction={direction} />}
          </GridCell>
        );
      })}
      {actionsColumn && (
        <GridCell
          role="columnheader"
          className={`${styles.headerCell} ${styles.actionsEnd}`}
          {...cell(columns.length + offset)}
        >
          <VisuallyHidden>Actions</VisuallyHidden>
        </GridCell>
      )}
    </div>
  );
}

function ariaSort(direction: SortDirection) {
  if (direction === "asc") return "ascending";
  return direction === "desc" ? "descending" : "none";
}

const SORT_ICONS = { asc: ArrowUp, desc: ArrowDown, none: ArrowUpDown };

function SortIcon({ direction }: { direction: SortDirection }) {
  const Icon = SORT_ICONS[direction || "none"];
  return <Icon aria-hidden="true" className={styles.sortIcon} data-active={direction !== false} />;
}
