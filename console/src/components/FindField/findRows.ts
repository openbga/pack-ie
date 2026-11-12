/**
 * The rows where any of `fields` contains the find text, ignoring case and surrounding
 * spaces; every row when the text is blank. Only for directories whose data contract lets
 * the UI filter: the engine returns the whole collection.
 */
export function findRows<Row>(
  rows: readonly Row[],
  query: string,
  fields: (row: Row) => readonly (string | null | undefined)[],
): readonly Row[] {
  const text = query.trim().toLocaleLowerCase();
  if (text === "") return rows;
  return rows.filter((row) =>
    fields(row).some((field) => field?.toLocaleLowerCase().includes(text) ?? false),
  );
}
