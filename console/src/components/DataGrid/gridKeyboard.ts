/** Row index of the header row; data rows count from 0. */
export const HEADER_ROW = -1;

/** A cell of the grid, by display order. */
export interface CellPosition {
  row: number;
  col: number;
}

export interface GridSize {
  rowCount: number;
  colCount: number;
}

interface KeyContext {
  /** Ctrl or Cmd is held. */
  ctrl: boolean;
  /** Rows moved by Page Up and Page Down: about one screen. */
  pageRows: number;
}

type Move = (from: CellPosition, size: GridSize, keys: KeyContext) => CellPosition;

// Keyboard model of the WAI-ARIA APG data grid pattern.
const MOVES: Record<string, Move> = {
  ArrowUp: (from) => ({ ...from, row: from.row - 1 }),
  ArrowDown: (from) => ({ ...from, row: from.row + 1 }),
  ArrowLeft: (from) => ({ ...from, col: from.col - 1 }),
  ArrowRight: (from) => ({ ...from, col: from.col + 1 }),
  PageUp: (from, _size, keys) => ({ ...from, row: from.row - keys.pageRows }),
  PageDown: (from, _size, keys) => ({ ...from, row: from.row + keys.pageRows }),
  Home: (from, _size, keys) => ({ row: keys.ctrl ? HEADER_ROW : from.row, col: 0 }),
  End: (from, size, keys) => ({
    row: keys.ctrl ? size.rowCount - 1 : from.row,
    col: size.colCount - 1,
  }),
};

/** Keeps a position inside the grid, for example after rows were removed. */
export function clampPosition(position: CellPosition, size: GridSize): CellPosition {
  return {
    row: Math.min(Math.max(position.row, HEADER_ROW), size.rowCount - 1),
    col: Math.min(Math.max(position.col, 0), size.colCount - 1),
  };
}

/** The cell a navigation key moves to, or `null` when the key does not navigate. */
export function nextPosition(
  key: string,
  from: CellPosition,
  size: GridSize,
  keys: KeyContext,
): CellPosition | null {
  const move = MOVES[key];
  return move ? clampPosition(move(from, size, keys), size) : null;
}
