import type { EngineError } from "@/api/types";

/** A synthetic engine error for tests. */
export function engineError(
  code: EngineError["code"],
  message: string,
  extra: Partial<EngineError> = {},
): EngineError {
  return {
    code,
    message,
    fieldErrors: [],
    correlationId: "corr-test-0001",
    ...extra,
  };
}
