import { type ReactNode, useLayoutEffect, useState } from "react";
import { Button } from "@/components/Button";
import styles from "./DataGrid.module.css";
import type { GridSelection } from "./gridSelection";

/** Bulk actions on several rows at once: a check column, Space to toggle, and a selection bar. */
export interface GridBulk extends GridSelection {
  /** The bulk action buttons, shown in the selection bar while rows are selected. */
  actions: ReactNode;
}

const rows = (count: number) => (count === 1 ? "1 row" : `${count} rows`);

/**
 * Keeps the selection to the listed rows. When the find, a filter or a refresh removes
 * selected rows from `rowIds`, they are unselected, so a bulk action never reaches a row
 * that is not shown; returns the notice saying so, until the selection next changes.
 */
export function useListedSelection(bulk: GridBulk | undefined, rowIds: readonly string[]) {
  const [notice, setNotice] = useState<string | null>(null);
  const selectedIds = bulk?.selectedIds;
  const onChange = bulk?.onChange;
  // Reconciles the caller's selection, which lives outside the grid, with the rows shown.
  useLayoutEffect(() => {
    if (!selectedIds?.size || !onChange) return;
    const listed = new Set(rowIds);
    const kept = [...selectedIds].filter((id) => listed.has(id));
    const dropped = selectedIds.size - kept.length;
    if (dropped === 0) return;
    onChange(new Set(kept));
    setNotice(`Unselected ${rows(dropped)} no longer listed.`);
  }, [selectedIds, onChange, rowIds]);

  const selection: GridSelection | undefined = bulk && {
    selectedIds: bulk.selectedIds,
    onChange: (ids) => {
      setNotice(null);
      bulk.onChange(ids);
    },
    isSelectable: bulk.isSelectable,
  };
  return { selection, notice };
}

interface SelectionBarProps {
  count: number;
  actions: ReactNode;
  notice: string | null;
  onClear: () => void;
}

/**
 * The bar above a bulk grid: how many rows are selected, in a polite live region with any
 * notice, then the bulk actions and Clear while any row is selected.
 */
export function SelectionBar({ count, actions, notice, onClear }: SelectionBarProps) {
  return (
    <div className={styles.selectionBar}>
      <p role="status" className={styles.selectionCount}>
        {count === 0 ? "None selected." : `${count} selected.`}
        {notice && ` ${notice}`}
      </p>
      {count > 0 && (
        <>
          {actions}
          <Button variant="quiet" onPress={onClear}>
            Clear
          </Button>
        </>
      )}
    </div>
  );
}
