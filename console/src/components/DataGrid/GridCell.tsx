import type {
  FocusEventHandler,
  KeyboardEventHandler,
  MouseEventHandler,
  PointerEventHandler,
  ReactNode,
} from "react";

/**
 * The grid's event handlers. One stable set serves every cell, so rows can be memoized; each
 * handler reads the cell's position from its `data-cell` attribute.
 */
export interface CellHandlers {
  onKeyDown: KeyboardEventHandler<HTMLElement>;
  onFocus: FocusEventHandler<HTMLElement>;
  onClick: MouseEventHandler<HTMLElement>;
  onContextMenu: MouseEventHandler<HTMLElement>;
  onPointerEnter: PointerEventHandler<HTMLElement>;
}

/** The attributes `DataGrid` gives every cell for focus and keyboard handling. */
interface CellBehavior extends CellHandlers {
  "data-cell": string;
  "aria-colindex": number;
  tabIndex: number;
}

/** The behavior of the cell at `row`, `col`; `tabStopCol` is the row's tab stop, if it has one. */
export function cellBehavior(
  row: number,
  col: number,
  tabStopCol: number | undefined,
  handlers: CellHandlers,
): CellBehavior {
  return {
    "data-cell": `${row}:${col}`,
    "aria-colindex": col + 1,
    tabIndex: col === tabStopCol ? 0 : -1,
    ...handlers,
  };
}

interface GridCellProps extends CellBehavior {
  role: "gridcell" | "columnheader";
  className: string | undefined;
  children: ReactNode;
  "aria-sort"?: "ascending" | "descending" | "none" | undefined;
  "data-sortable"?: true | undefined;
  "data-align"?: "end" | undefined;
  "data-wrap"?: true | undefined;
  "data-zero"?: true | undefined;
  "data-identifier"?: true | undefined;
}

/**
 * One cell of the grid. The grid uses ARIA roles on `div`s rather than table elements:
 * its virtualized rows are absolutely positioned, and WebKit drops table semantics when a
 * table element's `display` changes.
 */
export function GridCell({ role, children, ...rest }: GridCellProps) {
  return (
    <div role={role} {...rest}>
      {children}
    </div>
  );
}
