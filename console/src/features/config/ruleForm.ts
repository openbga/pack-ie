// The form's values for filters, mapping rules and code tables, and how they become the
// document's types (docs/spec/configuration.md §8). Numbers are held as text while typed.
// Field names follow the engine's paths (`mappings.1.rules.0.to`, `mappings.2.then.0.from`) so
// engine errors land on them; rule ids are kept as `ruleId`, since the form library keys its
// own list rows by `id`.
import type {
  CalculateOnInvalid,
  CalculateRounding,
  CodeTables,
  ConcatPart,
  FilterRule,
  LookupOnMissing,
  MappingOp,
  MappingRule,
  Step,
} from "@/api/types";
import {
  type ConditionFormValues,
  conditionToValues,
  emptyCondition,
  valuesToCondition,
} from "./conditionForm";
import { parseFormula, splitOperands } from "./formulaText";
import {
  type OperandFormValues,
  operandToValues,
  type StepFormValues,
  stepToValues,
  valuesToOperand,
  valuesToStep,
} from "./stepForm";

/** Where a rule list sits: only inside `for_each` and `append` does a `$.` path make sense. */
export type RuleContext = "top" | "loop" | "append";

export interface FilterFormValues extends ConditionFormValues {
  ruleId: string;
}

/** One piece of a `concat`: the text at a path (`from`) or literal text (`text`). */
export interface PartFormValues {
  kind: "from" | "text";
  from: string;
  text: string;
  /** A `from` part's text steps. */
  steps: StepFormValues[];
}

/** Every field of every op; `op` decides which are used, the others are dropped on save. */
export interface RuleFormValues {
  ruleId: string;
  op: MappingOp;
  from: string;
  to: string;
  default: string;
  required: boolean;
  value: string;
  table: string;
  onMissing: LookupOnMissing;
  ignoreCase: boolean;
  parts: PartFormValues[];
  /** `copy`, `lookup` and `datetime`: text steps applied to the text read from `from`. */
  steps: StepFormValues[];
  /** `calculate`: the formula as text (`$.4 * 0.0555`), the operands it names (`@na`), and its rounding as typed. */
  formula: string;
  operands: OperandFormValues[];
  scale: string;
  rounding: CalculateRounding;
  comparator: "reject" | "carry";
  notation: "text" | "sn";
  onInvalid: CalculateOnInvalid;
  input: string;
  output: string;
  fromTimezone: string;
  toTimezone: string;
  condition: ConditionFormValues;
  then: RuleFormValues[];
  else: RuleFormValues[];
  over: string;
  into: string;
  /** `for_each`: create a record for each one found, or rewrite each existing one in place. */
  mode: "create" | "update" | "repeat";
  rules: RuleFormValues[];
  start: string;
}

export interface CodeTableFormValues {
  name: string;
  rows: { code: string; text: string }[];
}

/** The two branch lists of a `when` rule; their field names are `then` and `else`. */
export function branches<Rule>(
  thenRules: Rule[],
  elseRules: Rule[],
): { then: Rule[]; else: Rule[] } {
  // biome-ignore lint/suspicious/noThenProperty: `then` is the rule's field name in runtime.json
  return { then: thenRules, else: elseRules };
}

export function emptyRule(ruleId: string, op: MappingOp = "copy"): RuleFormValues {
  return {
    ruleId,
    op,
    from: "",
    to: "",
    default: "",
    required: false,
    value: "",
    table: "",
    onMissing: "error",
    ignoreCase: false,
    parts: [],
    steps: [],
    formula: "",
    operands: [],
    scale: "",
    rounding: "half_up",
    comparator: "carry",
    notation: "text",
    onInvalid: "error",
    input: "",
    output: "",
    fromTimezone: "",
    toTimezone: "",
    condition: emptyCondition(),
    ...branches([], []),
    over: "",
    into: "",
    mode: "create",
    rules: [],
    start: "1",
  };
}

/** `{steps}` when there are some, so that a rule without steps is the one it was. */
const stepsOf = (steps: StepFormValues[]): { steps?: Step[] } =>
  steps.length === 0 ? {} : { steps: steps.map(valuesToStep) };

function partToValues(part: ConcatPart): PartFormValues {
  return "from" in part
    ? { kind: "from", from: part.from, text: "", steps: (part.steps ?? []).map(stepToValues) }
    : { kind: "text", from: "", text: part.text, steps: [] };
}

function valuesToPart(values: PartFormValues): ConcatPart {
  if (values.kind === "text") return { text: values.text };
  return { from: values.from.trim(), ...stepsOf(values.steps) };
}

export function ruleToValues(rule: MappingRule): RuleFormValues {
  const base = emptyRule(rule.id, rule.op);
  switch (rule.op) {
    case "copy":
      return {
        ...base,
        from: rule.from,
        to: rule.to,
        default: rule.default,
        required: rule.required,
        steps: (rule.steps ?? []).map(stepToValues),
      };
    case "constant":
      return { ...base, value: rule.value, to: rule.to };
    case "lookup":
      return {
        ...base,
        from: rule.from,
        table: rule.table,
        to: rule.to,
        onMissing: rule.onMissing,
        default: rule.default,
        ignoreCase: rule.ignoreCase,
        steps: (rule.steps ?? []).map(stepToValues),
      };
    case "concat":
      return { ...base, parts: rule.parts.map(partToValues), to: rule.to };
    case "datetime":
      return {
        ...base,
        from: rule.from,
        to: rule.to,
        input: rule.input,
        output: rule.output,
        fromTimezone: rule.fromTimezone,
        toTimezone: rule.toTimezone,
        steps: (rule.steps ?? []).map(stepToValues),
      };
    case "calculate": {
      const { text, operands } = splitOperands(rule.formula);
      return {
        ...base,
        formula: text,
        operands: operands.map(({ name, operand }) => operandToValues(name, operand)),
        to: rule.to,
        scale: rule.scale === null ? "" : String(rule.scale),
        rounding: rule.rounding,
        comparator: rule.comparator,
        notation: rule.notation,
        onInvalid: rule.onInvalid,
        default: rule.default,
        required: rule.required,
      };
    }
    case "when":
      return {
        ...base,
        condition: conditionToValues(rule.condition),
        ...branches(rule.then.map(ruleToValues), rule.else.map(ruleToValues)),
      };
    case "for_each":
      return {
        ...base,
        over: rule.over,
        into: rule.into,
        mode: rule.mode ?? "create",
        rules: rule.rules.map(ruleToValues),
      };
    case "append":
      return { ...base, into: rule.into, rules: rule.rules.map(ruleToValues) };
    case "sequence":
      return { ...base, to: rule.to, start: String(rule.start) };
  }
}

export function valuesToRule(values: RuleFormValues): MappingRule {
  const id = values.ruleId;
  const path = (text: string) => text.trim();
  switch (values.op) {
    case "copy":
      return {
        id,
        op: "copy",
        from: path(values.from),
        to: path(values.to),
        default: values.default,
        required: values.required,
        ...stepsOf(values.steps),
      };
    case "constant":
      return { id, op: "constant", value: values.value, to: path(values.to) };
    case "lookup":
      return {
        id,
        op: "lookup",
        from: path(values.from),
        table: values.table,
        to: path(values.to),
        onMissing: values.onMissing,
        default: values.onMissing === "default" ? values.default : "",
        ignoreCase: values.ignoreCase,
        ...stepsOf(values.steps),
      };
    case "concat":
      return { id, op: "concat", parts: values.parts.map(valuesToPart), to: path(values.to) };
    case "datetime":
      return {
        id,
        op: "datetime",
        from: path(values.from),
        to: path(values.to),
        input: path(values.input),
        output: path(values.output),
        fromTimezone: path(values.fromTimezone),
        toTimezone: path(values.toTimezone),
        ...stepsOf(values.steps),
      };
    case "calculate": {
      const named = Object.fromEntries(
        values.operands.map((operand) => [operand.name.trim(), valuesToOperand(operand)]),
      );
      const parsed = parseFormula(values.formula, named);
      const scale = values.scale.trim();
      return {
        id,
        op: "calculate",
        // A formula that does not read is stopped by the field's own check before this runs.
        formula: "formula" in parsed ? parsed.formula : { constant: "" },
        to: path(values.to),
        scale: scale === "" ? null : Number(scale),
        rounding: scale === "" ? "half_up" : values.rounding,
        comparator: values.comparator,
        notation: values.notation,
        onInvalid: values.onInvalid,
        default: values.onInvalid === "default" ? values.default : "",
        required: values.required,
      };
    }
    case "when":
      return {
        id,
        op: "when",
        condition: valuesToCondition(values.condition),
        ...branches(values.then.map(valuesToRule), values.else.map(valuesToRule)),
      };
    case "for_each":
      return {
        id,
        op: "for_each",
        over: path(values.over),
        // Rewriting in place is of the records looped over: there is no other record to write.
        into: path(values.mode === "update" ? values.over : values.into),
        ...(values.mode === "create" ? {} : { mode: values.mode }),
        rules: values.rules.map(valuesToRule),
      };
    case "append":
      return { id, op: "append", into: path(values.into), rules: values.rules.map(valuesToRule) };
    case "sequence":
      return { id, op: "sequence", to: path(values.to), start: Number(values.start.trim()) };
  }
}

/** Every rule id in a form's rule tree, so a new rule gets an id nothing else has. */
export function ruleIds(rules: readonly RuleFormValues[]): string[] {
  return rules.flatMap((rule) => [
    rule.ruleId,
    ...ruleIds(rule.then),
    ...ruleIds(rule.else),
    ...ruleIds(rule.rules),
  ]);
}

export function filterToValues(rule: FilterRule): FilterFormValues {
  const { id, ...condition } = rule;
  return { ruleId: id, ...conditionToValues(condition) };
}

export function valuesToFilter(values: FilterFormValues): FilterRule {
  return { id: values.ruleId, ...valuesToCondition(values) };
}

export function tablesToValues(tables: CodeTables): CodeTableFormValues[] {
  return Object.entries(tables).map(([name, rows]) => ({
    name,
    rows: Object.entries(rows).map(([code, text]) => ({ code, text })),
  }));
}

export function valuesToTables(tables: readonly CodeTableFormValues[]): CodeTables {
  return Object.fromEntries(
    tables.map((table) => [
      table.name.trim(),
      Object.fromEntries(table.rows.map((row) => [row.code.trim(), row.text])),
    ]),
  );
}

export {
  type ConditionFormValues,
  type ConditionType,
  emptyCondition,
  TEXT_OPERATORS,
  takesValue,
} from "./conditionForm";
