import type { EngineError, ErrorCode, FieldError } from "../types";

export const SECOND = 1000;
export const MINUTE = 60 * SECOND;
export const HOUR = 60 * MINUTE;
export const DAY = 24 * HOUR;
export const MIB = 1024 * 1024;

export function makeError(
  code: ErrorCode,
  message: string,
  correlationId: string,
  fieldErrors: FieldError[] = [],
): EngineError {
  return { code, message, fieldErrors, correlationId };
}

export function iso(ms: number): string {
  return new Date(ms).toISOString();
}

export function pad(value: number, width: number): string {
  return String(value).padStart(width, "0");
}

/** `yyyyMMddHHmmss`, the timestamp layout of HL7 and ASTM. */
export function compactTime(ms: number): string {
  return new Date(ms).toISOString().replace(/[-:T]/g, "").slice(0, 14);
}

export function clamp(value: number, low: number, high: number): number {
  return Math.min(high, Math.max(low, value));
}

/** Base64 of text whose characters are all single bytes (ASCII and control bytes). */
export function toBase64(text: string): string {
  return btoa(text);
}

/** Formats an integer scaled by `10^decimals` without floating point: 540 at 2 is `5.40`. */
export function formatScaled(scaled: number, decimals: number): string {
  const sign = scaled < 0 ? "-" : "";
  const absolute = Math.abs(scaled);
  if (decimals === 0) return `${sign}${absolute}`;
  const unit = 10 ** decimals;
  const whole = Math.floor(absolute / unit);
  const fraction = String(absolute % unit).padStart(decimals, "0");
  return `${sign}${whole}.${fraction}`;
}
