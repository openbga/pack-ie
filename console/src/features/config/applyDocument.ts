import { useEngineMutation } from "@/api/hooks";
import type { ApplyResult, ConfigDocument, EngineError, ValidationResult } from "@/api/types";
import { channelNamer, describeApply } from "./outcomes";

/** Queries a configuration change makes stale, by `EngineApi` method name. */
export const CONFIG_QUERIES = [
  "getConfig",
  "listConfigVersions",
  "getConfigVersion",
  "getChannel",
  "getEndpoint",
  "previewRollback",
] as const;

/** A validation result with errors, in the shape of a rejected command. */
export function validationFailure(result: ValidationResult): EngineError {
  const errors = result.issues.filter((issue) => issue.severity === "error");
  return {
    code: "validation",
    message: `The configuration has ${errors.length === 1 ? "a problem" : `${errors.length} problems`}. The running configuration was not changed.`,
    fieldErrors: errors.map((issue) => ({ field: issue.path, message: issue.message })),
    correlationId: "",
  };
}

interface ApplyInput {
  document: ConfigDocument;
  note: string;
}

/**
 * Validates `document` and applies it as a new version when it is valid; an invalid document
 * rejects with its problems and leaves the running configuration active (architecture
 * invariant 5). Warnings do not block.
 */
export function useApplyDocument() {
  return useEngineMutation<ApplyInput, ApplyResult>(
    async (api, { document, note }) => {
      const result = await api.validateConfig(document);
      if (!result.ok) throw validationFailure(result);
      return api.applyConfig(document, note.trim());
    },
    { invalidates: CONFIG_QUERIES },
  );
}

/**
 * The toast text after an apply: the new version and what each channel underwent (see
 * `describeApply`); `channels` are where to find labels, the new document's first. A partial result is a warning, which stays until it is dismissed.
 */
export function appliedToast(result: ApplyResult, ...channels: ConfigDocument["channels"][]) {
  return describeApply(result, channelNamer(...channels), {
    done: `Configuration version ${result.version} applied`,
    partial: `Configuration version ${result.version} applied, but not everywhere`,
  });
}
