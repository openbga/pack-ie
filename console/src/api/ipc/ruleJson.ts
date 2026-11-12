// Small readers shared by the rule parsers (`mappings.ts`, `calculate.ts`): rule JSON is read
// strictly, so that a rule the console cannot show completely is not shown at all.
import { textOf } from "./steps";

export type Json = Record<string, unknown>;

export function onlyKeys(json: Json, allowed: readonly string[]): boolean {
  return Object.keys(json).every((key) => allowed.includes(key));
}

export function str(json: Json, key: string): string | null {
  const value = json[key];
  return typeof value === "string" ? value : null;
}

/** An optional text (a string or a number): `""` when absent, null when present but not text. */
export function optionalStr(json: Json, key: string): string | null {
  const value = json[key];
  return value === undefined ? "" : textOf(value);
}

export function strings(json: Json, keys: string[]): string[] | null {
  const values = keys.map((key) => str(json, key));
  return values.every((v): v is string => v !== null) ? values : null;
}

/** `{key: value}`, or nothing when `skip`: the way optional fields stay out of the JSON. */
export const some = (key: string, value: unknown, skip: boolean): Json =>
  skip ? {} : { [key]: value };

/** `value` when it is one of `allowed` (or `fallback` when absent), else null. */
export function choice<T extends string>(
  value: unknown,
  allowed: ReadonlySet<string>,
  fallback: T,
): T | null {
  const chosen = value ?? fallback;
  return typeof chosen === "string" && allowed.has(chosen) ? (chosen as T) : null;
}
