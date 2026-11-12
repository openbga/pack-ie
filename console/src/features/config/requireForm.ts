// The form's values for a mapping's source preconditions (`require`, docs/spec/configuration.md
// §8.2). Numbers are held as text while typed.
import type { RequireCondition } from "@/api/types";

export interface RequireFormValues {
  kind: "count" | "same";
  /** `count`: the record or segment, such as OBR. */
  record: string;
  /** `count`: either limit may be empty, not both. */
  min: string;
  max: string;
  /** `same`: the path whose text every occurrence must share, such as OBR.3.1. */
  path: string;
}

export function emptyRequire(kind: RequireFormValues["kind"] = "count"): RequireFormValues {
  return { kind, record: "", min: "", max: "", path: "" };
}

export function requireToValues(condition: RequireCondition): RequireFormValues {
  if ("same" in condition) return { ...emptyRequire("same"), path: condition.same };
  return {
    ...emptyRequire("count"),
    record: condition.count,
    min: condition.min === null ? "" : String(condition.min),
    max: condition.max === null ? "" : String(condition.max),
  };
}

const limit = (text: string): number | null => (text.trim() === "" ? null : Number(text.trim()));

export function valuesToRequire(values: RequireFormValues): RequireCondition {
  if (values.kind === "same") return { same: values.path.trim() };
  return { count: values.record.trim(), min: limit(values.min), max: limit(values.max) };
}
