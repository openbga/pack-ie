import type { ReactNode } from "react";
import styles from "./MiniTable.module.css";

export interface MiniColumn<Row> {
  id: string;
  header: string;
  cell: (row: Row) => ReactNode;
  /** `end` for numbers, so their digits line up. */
  align?: "end";
}

/**
 * A small child list of a record page (a handful of rows, such as a channel's endpoints):
 * a plain table that the page scrolls with. Large collections use `DataGrid` instead.
 */
export function MiniTable<Row>({
  label,
  columns,
  rows,
  getRowId,
}: {
  /** Accessible name of the table. */
  label: string;
  columns: readonly MiniColumn<Row>[];
  rows: readonly Row[];
  getRowId: (row: Row) => string;
}) {
  return (
    <div className={styles.scroll}>
      <table aria-label={label} className={styles.table}>
        <thead>
          <tr>
            {columns.map((column) => (
              <th key={column.id} scope="col" data-align={column.align}>
                {column.header}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr key={getRowId(row)}>
              {columns.map((column) => (
                <td key={column.id} data-align={column.align}>
                  {column.cell(row)}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
