import { useLayoutEffect, useRef, useState } from "react";
import type { ActionItem } from "@/components/RowActions";

/** A row's actions menu, for the grid's `rowActions` option. */
export interface GridRowActions<Row> {
  /** The row's identifier, as people know it; the button is named "Actions for <label>". */
  label: (row: Row) => string;
  /** Whether the row has any action; rows without one show no button. Called while rendering. */
  hasItems: (row: Row) => boolean;
  /** The row's items, from the record's `allowedActions`; called only when its menu opens. */
  items: (row: Row) => readonly ActionItem[];
}

interface OpenMenu {
  rowId: string;
  items: readonly ActionItem[];
}

interface RowRef<Row> {
  id: string;
  original: Row;
}

/**
 * Which row's actions menu is open, with its items, built once when it opens.
 * `onMenuChange` stays the same function, so rows stay memoized.
 */
export function useRowMenu<Row>(
  rowActions: GridRowActions<Row> | undefined,
  rows: readonly RowRef<Row>[],
) {
  const [menu, setMenu] = useState<OpenMenu | null>(null);
  const latest = useRef({ rowActions, rows });
  useLayoutEffect(() => {
    latest.current = { rowActions, rows };
  });

  const [onMenuChange] = useState(() => (rowId: string, isOpen: boolean) => {
    const { rowActions: actions, rows: current } = latest.current;
    const row = isOpen ? current.find((each) => each.id === rowId) : undefined;
    const items = row && actions ? actions.items(row.original) : [];
    setMenu(items.length > 0 ? { rowId, items } : null);
  });

  /** Opens the menu of the row at `index`, if it has one; for right-click and Shift+F10. */
  function openAt(index: number): boolean {
    const row = rows[index];
    if (!row || !rowActions?.hasItems(row.original)) return false;
    onMenuChange(row.id, true);
    return true;
  }

  return { menu, onMenuChange, openAt };
}
