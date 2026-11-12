import {
  type KeyboardEvent,
  type MouseEvent,
  type RefObject,
  type SyntheticEvent,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import type { CellHandlers } from "./GridCell";
import {
  type CellPosition,
  clampPosition,
  type GridSize,
  HEADER_ROW,
  nextPosition,
} from "./gridKeyboard";

interface GridCellsOptions {
  size: GridSize;
  gridRef: RefObject<HTMLElement | null>;
  rowHeight: number;
  /** Indexes of the rows currently in the DOM. */
  renderedRows: readonly number[];
  /** The grid has a check column (column 0). */
  selectable: boolean;
  scrollToRow: (index: number) => void;
  /** Space or Enter on a header cell, or a click on it. */
  onHeaderAction: (col: number) => void;
  /** Space on a data row, or a click on its check cell. */
  onToggleRow: (row: number) => void;
  /** Enter on a data cell, or a click on one outside the check column and its controls. */
  onRowAction: ((row: number) => void) | undefined;
  /**
   * Shift+F10, the context-menu key or a right-click on a data row; returns whether the row
   * has a menu, so a row without one keeps the native context menu.
   */
  onRowMenu: ((row: number) => boolean) | undefined;
}

interface GridCells {
  tabStop: CellPosition;
  handlers: CellHandlers;
  /** Moves the tab stop and focus to a data row, scrolling it into view; keeps the column. */
  focusRow: (row: number) => void;
}

const cellSelector = (position: CellPosition) => `[data-cell="${position.row}:${position.col}"]`;

/** Elements inside a cell that act on their own, so a click on them does not open the row. */
const CONTROLS =
  'a[href], button, input, select, textarea, label, [role="button"], [role="link"], [role="checkbox"], [role="menuitem"]';

function positionOf(event: SyntheticEvent<HTMLElement>): CellPosition {
  const [row = HEADER_ROW, col = 0] = (event.currentTarget.dataset.cell ?? "")
    .split(":")
    .map(Number);
  return { row, col };
}

/**
 * Whether the event started inside the cell's own DOM. React also bubbles events from
 * portals, such as a row's open menu, through the cell; the grid ignores those.
 */
function isInCell(event: SyntheticEvent<HTMLElement>): boolean {
  return event.target instanceof Node && event.currentTarget.contains(event.target);
}

function isOnControl(event: MouseEvent<HTMLElement>): boolean {
  const control = event.target instanceof Element ? event.target.closest(CONTROLS) : null;
  return control !== null && control !== event.currentTarget;
}

/** A pointer gesture that selected text in the grid is for copying, not for opening. */
function selectedText(grid: HTMLElement | null): boolean {
  const selection = window.getSelection();
  if (!selection || selection.isCollapsed) return false;
  return grid?.contains(selection.anchorNode) ?? false;
}

/** Shift+F10 and the context-menu key open the focused row's actions menu. */
const isMenuKey = (event: KeyboardEvent<HTMLElement>) =>
  event.key === "ContextMenu" || (event.key === "F10" && event.shiftKey);

/**
 * Focus and keyboard behavior of the grid's cells (WAI-ARIA APG data grid): one tab stop
 * on the active cell, arrow-key movement that scrolls rows into view, and actions on
 * Space, Enter, Shift+F10, click and right-click. Focus that lands on a control inside a
 * cell moves to the cell, so the grid stays one tab stop. The handlers stay the same
 * object across renders, so unchanged rows skip rendering.
 */
export function useGridCells(options: GridCellsOptions): GridCells {
  const { size, gridRef, renderedRows } = options;
  const [stored, setActive] = useState<CellPosition>({ row: HEADER_ROW, col: 0 });
  const focusPending = useRef(false);
  const active = clampPosition(stored, size);
  const latest = useRef({ options, active });

  // The stable handlers read the latest options and active cell from here.
  useLayoutEffect(() => {
    latest.current = { options, active };
  });

  // Synchronizes DOM focus, which lives outside React, once the target cell is rendered.
  useLayoutEffect(() => {
    if (!focusPending.current) return;
    const cell = gridRef.current?.querySelector<HTMLElement>(cellSelector(active));
    if (cell) {
      focusPending.current = false;
      cell.focus();
    }
  });

  const [stable] = useState(() => {
    /** Makes `position` the active cell and focuses it once it is rendered. */
    function moveTo(position: CellPosition) {
      const current = latest.current.options;
      setActive(position);
      focusPending.current = true;
      if (position.row !== HEADER_ROW) current.scrollToRow(position.row);
      // Focus at once when the cell is already rendered: the state may not change.
      const cell = current.gridRef.current?.querySelector<HTMLElement>(cellSelector(position));
      if (cell) {
        focusPending.current = false;
        cell.focus();
      }
    }
    const focusRow = (row: number) =>
      moveTo(clampPosition({ row, col: latest.current.active.col }, latest.current.options.size));
    return { handlers: cellHandlers(latest, setActive, moveTo), focusRow };
  });

  // If the active row scrolled out of the DOM, its header cell keeps the tab stop.
  const tabStopRow = renderedRows.includes(active.row) ? active.row : HEADER_ROW;
  return { tabStop: { row: tabStopRow, col: active.col }, ...stable };
}

interface Latest {
  current: { options: GridCellsOptions; active: CellPosition };
}

/** The one set of cell event handlers, reading the grid's latest state from `latest`. */
function cellHandlers(
  latest: Latest,
  setActive: (update: (current: CellPosition) => CellPosition) => void,
  moveTo: (position: CellPosition) => void,
): CellHandlers {
  /** Space or Enter on a header cell, a click on it, Space on a data row, a click on a check cell. */
  function activate(position: CellPosition) {
    const current = latest.current.options;
    if (position.row === HEADER_ROW) current.onHeaderAction(position.col);
    else current.onToggleRow(position.row);
  }
  return {
    onKeyDown: keyHandler(latest, activate, moveTo),
    ...pointerHandlers(latest, activate),
    onFocus: (event) => {
      if (!isInCell(event)) return;
      if (event.target !== event.currentTarget) {
        event.currentTarget.focus();
        return;
      }
      const position = positionOf(event);
      setActive((current) =>
        current.row === position.row && current.col === position.col ? current : position,
      );
    },
  };
}

function keyHandler(
  latest: Latest,
  activate: (position: CellPosition) => void,
  moveTo: (position: CellPosition) => void,
): CellHandlers["onKeyDown"] {
  /** What a key does on the active cell; `undefined` for keys that only move focus. */
  function keyAction(event: KeyboardEvent<HTMLElement>, from: CellPosition) {
    const { onRowAction, onRowMenu } = latest.current.options;
    if (from.row === HEADER_ROW) {
      return event.key === " " || event.key === "Enter" ? () => activate(from) : undefined;
    }
    if (event.key === " ") return () => activate(from);
    if (event.key === "Enter" && onRowAction) return () => onRowAction(from.row);
    if (isMenuKey(event) && onRowMenu) return () => onRowMenu(from.row);
    return undefined;
  }

  function moveFocus(event: KeyboardEvent<HTMLElement>, from: CellPosition) {
    const current = latest.current.options;
    const viewRows = Math.floor((current.gridRef.current?.clientHeight ?? 0) / current.rowHeight);
    const next = nextPosition(event.key, from, current.size, {
      ctrl: event.ctrlKey || event.metaKey,
      pageRows: Math.max(1, viewRows - 1),
    });
    if (!next) return;
    event.preventDefault();
    moveTo(next);
  }

  return (event) => {
    if (!isInCell(event)) return;
    const from = latest.current.active;
    const action = keyAction(event, from);
    if (!action) {
      moveFocus(event, from);
      return;
    }
    event.preventDefault();
    action();
  };
}

function pointerHandlers(
  latest: Latest,
  activate: (position: CellPosition) => void,
): Pick<CellHandlers, "onClick" | "onContextMenu" | "onPointerEnter"> {
  return {
    // Header and check cells act on click. A click elsewhere on a data row opens it, except
    // on a control inside it, after a drag that selected text, or as the second click of a
    // double-click, which the first click already handled.
    onClick: (event) => {
      if (!isInCell(event)) return;
      const position = positionOf(event);
      const { selectable, onRowAction, gridRef } = latest.current.options;
      if (position.row === HEADER_ROW || (selectable && position.col === 0)) {
        activate(position);
        return;
      }
      if (event.detail > 1 || isOnControl(event) || selectedText(gridRef.current)) return;
      onRowAction?.(position.row);
    },
    onContextMenu: (event) => {
      const position = positionOf(event);
      const { onRowMenu } = latest.current.options;
      if (!isInCell(event) || position.row === HEADER_ROW || !onRowMenu) return;
      event.currentTarget.focus();
      if (onRowMenu(position.row)) event.preventDefault();
    },
    // A cell cut short shows its full text on hover. Set here rather than on every cell, so
    // screen readers do not hear the text twice and untruncated cells get no tooltip.
    onPointerEnter: (event) => {
      const cell = event.currentTarget;
      if (cell.scrollWidth > cell.clientWidth) cell.title = cell.textContent ?? "";
      else cell.removeAttribute("title");
    },
  };
}

/**
 * Returns focus to the open row when its record closes: when `openRowId` becomes null while
 * focus has nowhere to be, because the side panel that held it was removed.
 */
export function useReturnFocusToRow(
  openRowId: string | null,
  rowIds: readonly string[],
  focusRow: (row: number) => void,
) {
  const shown = useRef(openRowId);
  // Synchronizes DOM focus, which lives outside React, with the closing of the record.
  useLayoutEffect(() => {
    const closed = shown.current;
    shown.current = openRowId;
    if (openRowId !== null || closed === null) return;
    const focusLost = document.activeElement === null || document.activeElement === document.body;
    const index = rowIds.indexOf(closed);
    if (focusLost && index >= 0) focusRow(index);
  }, [openRowId, rowIds, focusRow]);
}
