// The form's values for a condition (a filter, or the test of a `when` rule): one comparison of a
// field, or a combination of conditions (docs/spec/configuration.md §8.1).
import type { Condition, FilterOperator } from "@/api/types";

/** How a condition is built: one comparison, or a combination of conditions. */
export type ConditionType = "compare" | "all" | "any" | "not";

export interface ConditionFormValues {
  type: ConditionType;
  /** A comparison's own fields; the others are dropped on save. */
  path: string;
  operator: FilterOperator;
  value: string;
  ignoreCase: boolean;
  /** `all` and `any`: the conditions combined. `not`: the one condition it negates. */
  items: ConditionFormValues[];
}

/** Operators that compare text, so letter case can be ignored. */
export const TEXT_OPERATORS: ReadonlySet<FilterOperator> = new Set<FilterOperator>([
  "equals",
  "not_equals",
  "in",
  "not_in",
  "starts_with",
  "contains",
  "matches",
]);

/** Operators that take no value. */
export function takesValue(operator: FilterOperator): boolean {
  return operator !== "exists" && operator !== "not_exists";
}

export function emptyCondition(): ConditionFormValues {
  return { type: "compare", path: "", operator: "equals", value: "", ignoreCase: false, items: [] };
}

export function conditionToValues(condition: Condition): ConditionFormValues {
  const base = emptyCondition();
  if ("all" in condition)
    return { ...base, type: "all", items: condition.all.map(conditionToValues) };
  if ("any" in condition)
    return { ...base, type: "any", items: condition.any.map(conditionToValues) };
  if ("not" in condition)
    return { ...base, type: "not", items: [conditionToValues(condition.not)] };
  return { ...base, ...condition };
}

export function valuesToCondition(values: ConditionFormValues): Condition {
  const items = values.items.map(valuesToCondition);
  switch (values.type) {
    case "all":
      return { all: items };
    case "any":
      return { any: items };
    case "not":
      return { not: items[0] ?? valuesToCondition(emptyCondition()) };
    case "compare":
      return {
        path: values.path.trim(),
        operator: values.operator,
        value: takesValue(values.operator) ? values.value : "",
        ignoreCase: TEXT_OPERATORS.has(values.operator) && values.ignoreCase,
      };
  }
}
