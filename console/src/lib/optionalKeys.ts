/** The value without the keys given, so unset fields are absent rather than `undefined`. */
export function without<T extends object>(value: T, ...keys: (keyof T)[]): T {
  const next = { ...value };
  for (const key of keys) delete next[key];
  return next;
}

/** The value with `key` set to `next`, or without it when `next` is undefined or empty. */
export function withValue<T extends object, Key extends keyof T>(
  value: T,
  key: Key,
  next: T[Key] | undefined,
): T {
  return next === undefined || next === "" ? without(value, key) : { ...value, [key]: next };
}
