/**
 * Text steps and calculate formulas as the engine's JSON has them (docs/spec/configuration.md
 * §8.2). A rule with steps lists them next to its `from`: `"steps": [{"step": "trim"}, ...]`.
 * Anything the console does not know (an unknown step, an extra key, a field of the wrong type)
 * is not shown at all: the reader returns null and the whole mapping stays hidden and untouched.
 */

import type { Formula, FormulaOp, Step } from "../types";
import { conditionFromExpression, conditionToExpression } from "./filters";
import { isRecord } from "./util";
import type { WireExpression } from "./wire";

type Json = Record<string, unknown>;

const hasOnly = (json: Json, allowed: readonly string[]) =>
  Object.keys(json).every((key) => allowed.includes(key));

const integer = (value: unknown): value is number =>
  typeof value === "number" && Number.isInteger(value);

/** Text the engine takes as a JSON string or a number; a number is its text. */
export function textOf(value: unknown): string | null {
  if (typeof value === "string") return value;
  return typeof value === "number" && Number.isFinite(value) ? String(value) : null;
}

const SIMPLE_STEPS: ReadonlySet<string> = new Set(["trim", "upper", "lower"]);

function sliceFromJson(json: Json): Step | null {
  const length = json.length ?? null;
  const ok = integer(json.start) && (length === null || integer(length));
  return hasOnly(json, ["step", "start", "length"]) && ok
    ? { step: "slice", start: json.start as number, length: length as number | null }
    : null;
}

function stepFromJson(json: unknown): Step | null {
  if (!isRecord(json) || typeof json.step !== "string") return null;
  if (SIMPLE_STEPS.has(json.step)) {
    return hasOnly(json, ["step"]) ? ({ step: json.step } as Step) : null;
  }
  const { chars, separator, take, find, with: replacement } = json;
  switch (json.step) {
    case "strip_leading":
      return hasOnly(json, ["step", "chars"]) && typeof chars === "string"
        ? { step: "strip_leading", chars }
        : null;
    case "split":
      return hasOnly(json, ["step", "separator", "take"]) &&
        typeof separator === "string" &&
        integer(take)
        ? { step: "split", separator, take }
        : null;
    case "slice":
      return sliceFromJson(json);
    case "replace":
      return hasOnly(json, ["step", "find", "with"]) &&
        typeof find === "string" &&
        typeof replacement === "string"
        ? { step: "replace", find, with: replacement }
        : null;
    default:
      return null;
  }
}

/** The steps of a rule (`undefined` is none), or null when one is not readable. */
export function stepsFromJson(value: unknown): Step[] | null {
  if (value === undefined) return [];
  if (!Array.isArray(value)) return null;
  const steps = value.map(stepFromJson);
  return steps.every((step): step is Step => step !== null) ? steps : null;
}

function stepToJson(step: Step): Json {
  if (step.step === "slice") {
    return {
      step: "slice",
      start: step.start,
      ...(step.length === null ? {} : { length: step.length }),
    };
  }
  return { ...step };
}

/** `{steps: [...]}`, or nothing when there are none. */
export const stepsEntry = (steps: readonly Step[] | undefined): Json =>
  steps === undefined || steps.length === 0 ? {} : { steps: steps.map(stepToJson) };

const OPERATORS: ReadonlySet<string> = new Set<FormulaOp>([
  "add",
  "sub",
  "mul",
  "div",
  "min",
  "max",
]);

function operandFromJson(json: Json): Formula | null {
  const steps = stepsFromJson(json.steps);
  if (steps === null || typeof json.from !== "string") return null;
  if ("find" in json) {
    if (!hasOnly(json, ["find", "where", "from", "steps"])) return null;
    const where = conditionFromExpression(json.where);
    return typeof json.find === "string" && where !== null
      ? { find: json.find, where, from: json.from, steps }
      : null;
  }
  return hasOnly(json, ["from", "steps"]) ? { from: json.from, steps } : null;
}

export function formulaFromJson(value: unknown): Formula | null {
  const constant = textOf(value);
  if (constant !== null) return { constant };
  if (!isRecord(value)) return null;
  if ("from" in value) return operandFromJson(value);
  const keys = Object.keys(value);
  const key = keys[0];
  if (keys.length !== 1 || key === undefined) return null;
  const operand = value[key];
  if (key === "abs") {
    const arg = formulaFromJson(operand);
    return arg === null ? null : { op: "abs", args: [arg] };
  }
  if (!OPERATORS.has(key) || !Array.isArray(operand)) return null;
  const args = operand.map(formulaFromJson);
  return args.every((arg): arg is Formula => arg !== null) ? { op: key as FormulaOp, args } : null;
}

export function formulaToJson(formula: Formula): unknown {
  if ("constant" in formula) return formula.constant;
  if ("find" in formula) {
    return {
      find: formula.find,
      where: conditionToExpression(formula.where) satisfies WireExpression,
      from: formula.from,
      ...stepsEntry(formula.steps),
    };
  }
  if ("from" in formula) return { from: formula.from, ...stepsEntry(formula.steps) };
  const [only] = formula.args;
  if (formula.op === "abs" && only !== undefined) return { abs: formulaToJson(only) };
  return { [formula.op]: formula.args.map(formulaToJson) };
}
