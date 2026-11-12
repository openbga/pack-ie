import type { ConfigIssue } from "../types";

export type Issues = ConfigIssue[];

/** `code` is the engine's problem code (docs/spec/configuration.md section 9) where one fits. */
export function error(issues: Issues, path: string, message: string, code?: string): void {
  issues.push(
    code === undefined
      ? { severity: "error", path, message }
      : { severity: "error", path, message, code },
  );
}

export function warning(issues: Issues, path: string, message: string, code?: string): void {
  issues.push(
    code === undefined
      ? { severity: "warning", path, message }
      : { severity: "warning", path, message, code },
  );
}

export function blank(value: string | undefined | null): boolean {
  return value === undefined || value === null || value.trim() === "";
}

export function required(
  issues: Issues,
  path: string,
  value: string | undefined,
  what: string,
): void {
  if (blank(value)) error(issues, path, `${what} is required.`);
}

export function checkPort(issues: Issues, path: string, port: number): void {
  if (!Number.isInteger(port) || port < 1 || port > 65535) {
    error(issues, path, "Port must be a whole number from 1 to 65535.");
  }
}

export function checkPositive(
  issues: Issues,
  path: string,
  value: number,
  what: string,
  code?: string,
): void {
  if (!Number.isFinite(value) || value <= 0) {
    error(issues, path, `${what} must be greater than 0.`, code);
  }
}

export function checkRange(
  issues: Issues,
  path: string,
  value: number,
  range: { min: number; max: number },
  what: string,
): void {
  if (!Number.isInteger(value) || value < range.min || value > range.max) {
    error(issues, path, `${what} must be a whole number from ${range.min} to ${range.max}.`);
  }
}
