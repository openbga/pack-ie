/** A value of a report cell; null and undefined are empty. */
type Cell = string | number | boolean | null | undefined;

/**
 * One cell as RFC 4180 text. A text a spreadsheet would read as a formula (it starts with `=`,
 * `+`, `-`, `@`, a tab or a carriage return) is prefixed with `'`, as the engine's exports do;
 * numbers are written as they are.
 */
export function csvCell(value: Cell): string {
  if (value === null || value === undefined) return "";
  if (typeof value !== "string") return String(value);
  const guarded = /^[=+\-@\t\r]/.test(value) ? `'${value}` : value;
  return /[",\r\n]/.test(guarded) ? `"${guarded.replaceAll('"', '""')}"` : guarded;
}

/** A header row and the rows under it, each line ended by CRLF (RFC 4180). */
export function toCsv(header: readonly string[], rows: readonly (readonly Cell[])[]): string {
  return [header, ...rows].map((row) => `${row.map(csvCell).join(",")}\r\n`).join("");
}
