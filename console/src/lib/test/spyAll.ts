import { vi } from "vitest";

/**
 * Makes every function in a stub engine's calls a spy, so a test that overrides one can
 * still read its calls. Values that already are spies, and non-functions, stay as they are.
 */
export function spyAll(calls: Record<string, unknown>): Record<string, unknown> {
  return Object.fromEntries(
    Object.entries(calls).map(([name, value]) => [
      name,
      typeof value === "function" && !vi.isMockFunction(value)
        ? vi.fn(value as (...args: unknown[]) => unknown)
        : value,
    ]),
  );
}
