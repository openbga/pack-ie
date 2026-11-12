/**
 * Filters: the console's list of conditions and the engine's filter expressions.
 *
 * Wire format (docs/spec/configuration.md section 8.1): a filter is an expression, either one
 * comparison `{path, op, value?, ignore_case?}` or a combination `{all: [...]}`, `{any: [...]}`
 * or `{not: ...}` of expressions. `value` is absent for `exists` and `not_exists`, an array for
 * `in` and `not_in` (the console shows it comma-joined), text or a number otherwise;
 * `ignore_case` is written only when true. The same expression is a mapping `when` condition.
 *
 * An expression the console cannot show (extra keys, values that are not text or numbers,
 * nesting beyond the engine's limit) is a hidden expression: it is kept as it was and written back at
 * the same place relative to the plain ones, that is after as many plain expressions as
 * preceded it originally (or at the end when there are fewer now). Editor ids (`f0`, `f1`, ...
 * among the plain ones) are made up on read, tell which original a rule came from, and are
 * never written.
 */

import { clone } from "@/lib/clone";
import type { Comparison, Condition, FilterOperator, FilterRule } from "../types";
import { isEqual, isRecord, splitList } from "./util";
import type { WireExpression } from "./wire";

const OPERATORS: ReadonlySet<string> = new Set<FilterOperator>([
  "equals",
  "not_equals",
  "in",
  "not_in",
  "starts_with",
  "contains",
  "matches",
  "exists",
  "not_exists",
  "gt",
  "gte",
  "lt",
  "lte",
]);
const LIST_OPERATORS: ReadonlySet<string> = new Set(["in", "not_in"]);
const NO_VALUE_OPERATORS: ReadonlySet<string> = new Set(["exists", "not_exists"]);
const KEYS: ReadonlySet<string> = new Set(["path", "op", "value", "ignore_case"]);
/** The engine accepts expressions nested this deep (docs/spec/configuration.md §8.1). */
const MAX_DEPTH = 32;

/** Text the engine takes as a JSON string or a number; a number is compared as its text. */
function textOf(value: unknown): string | null {
  if (typeof value === "string") return value;
  return typeof value === "number" && Number.isFinite(value) ? String(value) : null;
}

function valueText(operator: string, value: unknown): string | null {
  if (NO_VALUE_OPERATORS.has(operator)) return value === undefined ? "" : null;
  if (LIST_OPERATORS.has(operator)) {
    if (!Array.isArray(value)) return null;
    const items = value.map(textOf);
    return items.every((item): item is string => item !== null) ? items.join(", ") : null;
  }
  return textOf(value);
}

function comparisonFromExpression(expression: Record<string, unknown>): Comparison | null {
  if (!Object.keys(expression).every((key) => KEYS.has(key))) return null;
  const { path, op, value, ignore_case: ignoreCase } = expression;
  if (typeof path !== "string" || typeof op !== "string" || !OPERATORS.has(op)) return null;
  if (ignoreCase !== undefined && typeof ignoreCase !== "boolean") return null;
  const text = valueText(op, value);
  if (text === null) return null;
  return { path, operator: op as FilterOperator, value: text, ignoreCase: ignoreCase === true };
}

function combinationFromExpression(
  expression: Record<string, unknown>,
  depth: number,
): Condition | null {
  const keys = Object.keys(expression);
  const key = keys[0];
  if (keys.length !== 1 || (key !== "all" && key !== "any" && key !== "not")) return null;
  const inner = expression[key];
  if (key === "not") {
    const negated = readCondition(inner, depth + 1);
    return negated === null ? null : { not: negated };
  }
  if (!Array.isArray(inner)) return null;
  const items = inner.map((item) => readCondition(item, depth + 1));
  if (!items.every((item): item is Condition => item !== null)) return null;
  return key === "all" ? { all: items } : { any: items };
}

function readCondition(expression: unknown, depth: number): Condition | null {
  if (!isRecord(expression) || depth > MAX_DEPTH) return null;
  return "path" in expression
    ? comparisonFromExpression(expression)
    : combinationFromExpression(expression, depth);
}

/** The condition an expression holds, or null when the console cannot show it. */
export function conditionFromExpression(expression: unknown): Condition | null {
  return readCondition(expression, 1);
}

export function conditionToExpression(condition: Condition): WireExpression {
  if ("all" in condition) return { all: condition.all.map(conditionToExpression) };
  if ("any" in condition) return { any: condition.any.map(conditionToExpression) };
  if ("not" in condition) return { not: conditionToExpression(condition.not) };
  const { path, operator, value, ignoreCase } = condition;
  const expression: WireExpression = { path, op: operator };
  if (LIST_OPERATORS.has(operator)) expression.value = splitList(value);
  else if (!NO_VALUE_OPERATORS.has(operator)) expression.value = value;
  if (ignoreCase) expression.ignore_case = true;
  return expression;
}

const withoutId = (rule: FilterRule): Condition => {
  const { id: _id, ...condition } = rule;
  return condition;
};

interface Plain {
  id: string;
  expression: WireExpression;
  condition: Condition;
}

/** The expressions the console shows, and where the hidden ones sit among them. */
function split(expressions: readonly WireExpression[] | undefined) {
  const plain: Plain[] = [];
  const hidden: { expression: WireExpression; anchor: number }[] = [];
  for (const expression of expressions ?? []) {
    const condition = conditionFromExpression(expression);
    if (condition === null) hidden.push({ expression, anchor: plain.length });
    else plain.push({ id: `f${plain.length}`, expression, condition });
  }
  return { plain, hidden };
}

export function filtersFromWire(expressions: readonly WireExpression[] | undefined): FilterRule[] {
  return split(expressions).plain.map(({ id, condition }): FilterRule => ({ id, ...condition }));
}

/**
 * The console index of the filter at `rawIndex` of `expressions`, or null when the console
 * does not show it.
 */
export function visibleFilterIndex(
  expressions: readonly WireExpression[] | undefined,
  rawIndex: number,
): number | null {
  let visible = 0;
  for (const [i, expression] of (expressions ?? []).entries()) {
    const shown = conditionFromExpression(expression) !== null;
    if (i === rawIndex) return shown ? visible : null;
    if (shown) visible += 1;
  }
  return null;
}

export function filtersToWire(
  rules: readonly FilterRule[],
  original: readonly WireExpression[] | undefined,
): WireExpression[] {
  const { plain, hidden } = split(original);
  const byId = new Map(plain.map((p) => [p.id, p]));
  const out = rules.map((rule): WireExpression => {
    const before = byId.get(rule.id);
    if (before !== undefined && isEqual(withoutId(rule), before.condition))
      return clone(before.expression);
    return conditionToExpression(rule);
  });
  hidden.forEach(({ expression, anchor }, inserted) => {
    out.splice(Math.min(anchor, rules.length) + inserted, 0, clone(expression));
  });
  return out;
}

/** Whether two filter lists are the same to the console (editor ids do not count). */
export function sameFilters(a: readonly FilterRule[], b: readonly FilterRule[]): boolean {
  return isEqual(a.map(withoutId), b.map(withoutId));
}
