/** Small helpers for plain JSON data: the documents and DTOs that cross the IPC boundary. */

export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** Deep equality of plain JSON values; key order and `undefined` members do not matter. */
export function isEqual(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (Array.isArray(a)) {
    return Array.isArray(b) && a.length === b.length && a.every((item, i) => isEqual(item, b[i]));
  }
  if (isRecord(a) && isRecord(b)) {
    const keys = new Set([...Object.keys(a), ...Object.keys(b)]);
    for (const key of keys) {
      if (!isEqual(a[key], b[key])) return false;
    }
    return true;
  }
  return false;
}

/** `a, b` to `["a", "b"]`: trimmed, empty entries dropped. */
export function splitList(text: string): string[] {
  return text
    .split(",")
    .map((part) => part.trim())
    .filter((part) => part !== "");
}

/** Base64 of raw bytes. */
export function bytesToBase64(bytes: readonly number[] | Uint8Array): string {
  const CHUNK = 0x8000;
  let binary = "";
  for (let i = 0; i < bytes.length; i += CHUNK) {
    binary += String.fromCharCode(...bytes.slice(i, i + CHUNK));
  }
  return btoa(binary);
}

/** Sets `target[key]` for every modeled field that differs; see `applyPatch`. */
type Setter<U, R> = (raw: R, ui: U, base: U | null) => void;

/** One setter per modeled UI field, writing it into the engine object. */
export type Patch<U, R> = { [K in keyof U]?: Setter<U, R> };

/**
 * Writes the UI fields that differ from `base` (what `fromIpc(raw)` produced) into `raw`.
 * A `null` base means a new item: every modeled field is written.
 */
export function applyPatch<U, R>(raw: R, ui: U, base: U | null, table: Patch<U, R>): void {
  for (const key of Object.keys(table) as (keyof U)[]) {
    const setter = table[key];
    if (setter !== undefined && (base === null || !isEqual(ui[key], base?.[key]))) {
      setter(raw, ui, base);
    }
  }
}
