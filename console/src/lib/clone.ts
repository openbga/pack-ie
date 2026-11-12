/** Deep copy of plain JSON data (documents, DTOs). */
export function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}
