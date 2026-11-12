// Engine errors, as the console words them.
import type { EngineError, ErrorCode } from "@/api/types";

const FALLBACK_MESSAGES: Record<ErrorCode, string> = {
  validation: "Check the highlighted fields.",
  not_found: "The record was not found. It may have been removed by a configuration change.",
  conflict: "The configuration changed since it was shown. Reload it and try again.",
  engine_unavailable: "The engine is not running.",
  not_implemented: "This engine build does not provide this yet.",
  unavailable: "This function is not available in this version of the console.",
  internal: "The console could not complete the action.",
};

const CODES = new Set<string>(Object.keys(FALLBACK_MESSAGES));

function isEngineError(value: unknown): value is EngineError {
  if (typeof value !== "object" || value === null) return false;
  const candidate: Record<string, unknown> = { ...value };
  return (
    typeof candidate.code === "string" &&
    CODES.has(candidate.code) &&
    typeof candidate.message === "string" &&
    Array.isArray(candidate.fieldErrors) &&
    typeof candidate.correlationId === "string"
  );
}

/**
 * Normalizes anything an `EngineApi` call rejects with. Engine errors pass through; anything
 * else (a transport failure, a bug) becomes `internal` without echoing the raw text, which
 * can carry internal detail.
 */
export function toEngineError(value: unknown): EngineError {
  if (isEngineError(value)) return value;
  return {
    code: "internal",
    message: FALLBACK_MESSAGES.internal,
    fieldErrors: [],
    correlationId: "",
  };
}

/** The text to show for an error: the engine's safe message, or a plain default. */
export function toUserMessage(error: EngineError): string {
  return error.message.trim() || FALLBACK_MESSAGES[error.code];
}

/**
 * Where a form's values differ from the command input it builds: an engine path prefix and
 * the form path it maps to, such as `evidence` → `""` when the form holds the evidence
 * fields at its root. Prefixes match whole path segments; the longest one wins.
 */
export type FieldPaths = Readonly<Record<string, string>>;

/**
 * The form field an engine field error belongs to. The engine names list entries
 * `source.filters[0].path`; form fields are named `filters.0.path`.
 */
export function toFormField(enginePath: string, fieldPaths: FieldPaths = {}): string {
  const path = enginePath.replace(/\[(\d+)\]/g, ".$1");
  const prefix = Object.keys(fieldPaths)
    .filter((key) => path === key || path.startsWith(`${key}.`))
    .sort((a, b) => b.length - a.length)[0];
  if (prefix === undefined) return path;
  const target = fieldPaths[prefix] ?? "";
  const rest = path.slice(prefix.length + 1);
  return [target, rest].filter(Boolean).join(".");
}

/** Engine field errors keyed by form field name, for `Form`'s `validationErrors`. */
export function toValidationErrors(
  error: EngineError | null,
  fieldPaths?: FieldPaths,
): Record<string, string> {
  const errors: Record<string, string> = {};
  for (const fieldError of error?.fieldErrors ?? []) {
    errors[toFormField(fieldError.field, fieldPaths)] = fieldError.message;
  }
  return errors;
}
