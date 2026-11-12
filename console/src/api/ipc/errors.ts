import type { IpcError, ErrorCode as IpcErrorCode, RejectionCode } from "@/ipc/generated";
import type { EngineError, ErrorCode, FieldError } from "../types";
import type { CommandError } from "./bridge";
import { toUiPath } from "./paths";
import { isRecord } from "./util";
import type { WireConfig } from "./wire";

const CODES: Record<IpcErrorCode, ErrorCode> = {
  bad_request: "internal",
  invalid_config: "validation",
  conflict: "conflict",
  not_found: "not_found",
  rejected: "conflict",
  not_implemented: "not_implemented",
  unavailable: "unavailable",
  internal: "internal",
};

/** A `rejected` command says why with a reason code; some reasons are other kinds of error. */
const REJECTION_CODES: Partial<Record<RejectionCode, ErrorCode>> = {
  not_found: "not_found",
  destination_not_found: "not_found",
  invalid_config: "validation",
  version_conflict: "conflict",
  transcript_in_use: "conflict",
};

const ENGINE_CODES = new Set<string>([
  "validation",
  "not_found",
  "conflict",
  "engine_unavailable",
  "not_implemented",
  "unavailable",
  "internal",
]);

const GENERIC_MESSAGE = "The console could not complete the action.";
const OFFLINE_MESSAGE = "The engine is not running.";

export function engineError(
  code: ErrorCode,
  message: string,
  fieldErrors: FieldError[] = [],
): EngineError {
  return { code, message, fieldErrors, correlationId: "" };
}

function isEngineError(value: unknown): value is EngineError {
  return (
    isRecord(value) &&
    typeof value.code === "string" &&
    ENGINE_CODES.has(value.code) &&
    typeof value.message === "string" &&
    Array.isArray(value.fieldErrors)
  );
}

/** `document` is what the engine checked, when the console sent one (it refines paths). */
export function fromIpcError(error: IpcError, document?: WireConfig): EngineError {
  const reason = error.reason_code === null ? undefined : REJECTION_CODES[error.reason_code];
  const code = error.code === "rejected" && reason !== undefined ? reason : CODES[error.code];
  // Only errors are field errors; the engine lists warnings of the document as well.
  const fieldErrors = error.field_errors
    .filter(({ severity }) => severity === "error")
    .map(({ path, message }) => ({ field: toUiPath(path, document), message }));
  const refused = engineError(code, error.message, fieldErrors);
  if (error.code === "rejected" && error.reason_code !== null) {
    refused.rejection = error.reason_code;
  }
  return refused;
}

function isCommandError(value: unknown): value is CommandError {
  return isRecord(value) && typeof value.kind === "string";
}

function messageOf(error: { message?: unknown }, fallback: string): string {
  return typeof error.message === "string" && error.message.trim() !== ""
    ? error.message
    : fallback;
}

/** Whatever a Tauri command or this layer rejected with, as the `EngineError` the pages expect. */
export function fromCommandError(value: unknown, document?: WireConfig): EngineError {
  if (isEngineError(value)) return value;
  if (!isCommandError(value)) return engineError("internal", GENERIC_MESSAGE);
  switch (value.kind) {
    case "ipc":
      return fromIpcError(value.error, document);
    case "disconnected":
      return engineError("engine_unavailable", messageOf(value, OFFLINE_MESSAGE));
    default:
      return engineError("internal", messageOf(value, GENERIC_MESSAGE));
  }
}

/** A command that cannot reach the engine at all (`start_service`): the text shown verbatim. */
export function fromServiceError(value: unknown): EngineError {
  if (isEngineError(value)) return value;
  if (isRecord(value) && typeof value.message === "string") {
    return engineError("unavailable", value.message);
  }
  if (typeof value === "string") return engineError("unavailable", value);
  return engineError("unavailable", "The service could not be started.");
}
