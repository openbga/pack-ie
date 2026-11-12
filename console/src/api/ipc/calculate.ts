// The `calculate` rule as engine JSON (docs/spec/configuration.md §8.2). Optional fields are
// written only when they differ from the engine's default: `comparator` is `carry` unless
// written, `on_invalid` is `error`, `rounding` is `half_up`, and `rounding` is never written
// without `scale`.
import type { CalculateOnInvalid, CalculateRounding, MappingRule } from "../types";
import { choice, type Json, onlyKeys, optionalStr, some, str } from "./ruleJson";
import { formulaFromJson, formulaToJson } from "./steps";

type Calculate = Extract<MappingRule, { op: "calculate" }>;

const KEYS = [
  "op",
  "formula",
  "to",
  "scale",
  "rounding",
  "comparator",
  "notation",
  "on_invalid",
  "default",
  "required",
];
const ROUNDINGS: ReadonlySet<string> = new Set<CalculateRounding>([
  "half_up",
  "half_even",
  "truncate",
]);
const COMPARATORS: ReadonlySet<string> = new Set(["reject", "carry"]);
const NOTATIONS: ReadonlySet<string> = new Set(["text", "sn"]);
const ON_INVALID: ReadonlySet<string> = new Set<CalculateOnInvalid>([
  "error",
  "skip",
  "keep",
  "default",
]);

export function parseCalculate(json: Json, id: string): MappingRule | null {
  if (!onlyKeys(json, KEYS)) return null;
  const formula = formulaFromJson(json.formula);
  const to = str(json, "to");
  const scale = json.scale ?? null;
  const rounding = choice<CalculateRounding>(json.rounding, ROUNDINGS, "half_up");
  const comparator = choice<"reject" | "carry">(json.comparator, COMPARATORS, "carry");
  const notation = choice<"text" | "sn">(json.notation, NOTATIONS, "text");
  const onInvalid = choice<CalculateOnInvalid>(json.on_invalid, ON_INVALID, "error");
  const fallback = optionalStr(json, "default");
  const required = json.required ?? false;
  const scaleOk = scale === null || (typeof scale === "number" && Number.isInteger(scale));
  if (formula === null || to === null || fallback === null || !scaleOk) return null;
  if (rounding === null || comparator === null || onInvalid === null) return null;
  if (notation === null) return null;
  if (typeof required !== "boolean") return null;
  return {
    id,
    op: "calculate",
    formula,
    to,
    scale,
    rounding,
    comparator,
    notation,
    onInvalid,
    default: fallback,
    required,
  };
}

export function calculateToJson(rule: Calculate): Json {
  return {
    op: "calculate",
    formula: formulaToJson(rule.formula),
    to: rule.to,
    ...some("scale", rule.scale, rule.scale === null),
    ...some("rounding", rule.rounding, rule.scale === null || rule.rounding === "half_up"),
    ...some("comparator", rule.comparator, rule.comparator === "carry"),
    ...some("notation", rule.notation, rule.notation === "text"),
    ...some("on_invalid", rule.onInvalid, rule.onInvalid === "error"),
    ...some("default", rule.default, rule.onInvalid !== "default"),
    ...some("required", true, !rule.required),
  };
}
