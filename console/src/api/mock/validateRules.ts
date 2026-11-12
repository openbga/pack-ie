import type {
  CodeTables,
  Comparison,
  ConcatPart,
  Condition,
  DestinationConfig,
  FilterOperator,
  FilterRule,
  Formula,
  MappingRule,
  RequireCondition,
} from "../types";
import { blank, error, type Issues, required, warning } from "./validateIssues";

const NAME = "[A-Za-z][A-Za-z0-9_]*";
const INDEX = "(?:\\[\\d+\\])?";
/** `PID.3[0].1`, `OBX[2].5`, `R.3`, `col.sample_id`: a segment or column, then fields. */
const MESSAGE_PATH = new RegExp(`^${NAME}${INDEX}(?:\\.[A-Za-z0-9_]+${INDEX}){0,3}$`);
/** `$.3[0].1`: inside `for_each`, the current occurrence. */
const LOOP_PATH = new RegExp(`^\\$\\.[A-Za-z0-9_]+${INDEX}(?:\\.[A-Za-z0-9_]+${INDEX}){0,2}$`);
const SEGMENT_NAME = new RegExp(`^${NAME}$`);
const DECIMAL = /^[+-]?\d+(?:\.\d+)?$/;
const NUMERIC_OPERATORS: readonly FilterOperator[] = ["gt", "gte", "lt", "lte"];
const MAX_REGEX_LENGTH = 200;

/** Where a rule sits: only inside `for_each` do `$.` paths and `sequence` make sense. */
interface Scope {
  tables: CodeTables;
  inLoop: boolean;
  /** Inside `append`: `to` paths are `$.` paths of the new record, `from` paths are absolute. */
  inAppend?: boolean;
}

function pathProblem(path: string, inLoop: boolean): string | null {
  if (path.startsWith("$")) {
    if (!inLoop) return "Paths starting with $. are only valid inside a for_each rule.";
    return LOOP_PATH.test(path) ? null : "Not a valid path; use $.field[occurrence].component.";
  }
  return MESSAGE_PATH.test(path)
    ? null
    : "Not a valid path; use SEGMENT[occurrence].field[repetition].component, R.3 or col.name.";
}

function checkPath(issues: Issues, at: string, value: string, inLoop: boolean, what: string): void {
  if (blank(value)) {
    error(issues, at, `${what} is required.`);
    return;
  }
  const problem = pathProblem(value.trim(), inLoop);
  if (problem !== null) error(issues, at, problem);
}

function checkSegmentName(issues: Issues, at: string, value: string, what: string): void {
  if (blank(value)) error(issues, at, `${what} is required.`);
  else if (!SEGMENT_NAME.test(value.trim())) {
    error(issues, at, `${what} must be a record or segment name such as R or OBX.`);
  }
}

function checkValue(
  issues: Issues,
  at: string,
  comparison: Pick<Comparison, "operator" | "value" | "ignoreCase">,
): void {
  const { operator, value } = comparison;
  if (operator === "exists" || operator === "not_exists") return;
  if (blank(value)) {
    error(issues, `${at}.value`, "A value to compare with is required.");
    return;
  }
  if (operator === "in" || operator === "not_in") {
    if (value.split(",").every((item) => item.trim() === "")) {
      error(issues, `${at}.value`, "Give at least one value, separated by commas.");
    }
  } else if (operator === "matches") {
    checkRegex(issues, `${at}.value`, value);
  } else if (NUMERIC_OPERATORS.includes(operator) && !DECIMAL.test(value.trim())) {
    warning(issues, `${at}.value`, "This is not a number, so the comparison is never true.");
  }
}

function checkRegex(issues: Issues, at: string, value: string): void {
  if (value.length > MAX_REGEX_LENGTH) {
    error(issues, at, `A pattern may have at most ${MAX_REGEX_LENGTH} characters.`);
    return;
  }
  try {
    new RegExp(value);
  } catch {
    error(issues, at, "This is not a valid regular expression.");
  }
}

function checkComparison(issues: Issues, at: string, comparison: Comparison, inLoop: boolean) {
  checkPath(issues, `${at}.path`, comparison.path, inLoop, "The path");
  checkValue(issues, at, comparison);
}

/** A comparison or a combination; the items of a combination are at `at.items[n]`. */
function checkCondition(issues: Issues, at: string, condition: Condition, inLoop: boolean): void {
  if ("path" in condition) {
    checkComparison(issues, at, condition, inLoop);
    return;
  }
  const items = "all" in condition ? condition.all : "any" in condition ? condition.any : null;
  if (items === null) {
    const inner = "not" in condition ? condition.not : null;
    if (inner !== null) checkCondition(issues, `${at}.items[0]`, inner, inLoop);
    return;
  }
  if (items.length === 0) error(issues, at, "Add at least one condition to combine.");
  items.forEach((item, index) => {
    checkCondition(issues, `${at}.items[${index}]`, item, inLoop);
  });
}

export function validateFilters(filters: FilterRule[], path: string, issues: Issues): void {
  filters.forEach((filter, index) => {
    checkCondition(issues, `${path}.filters[${index}]`, filter, false);
  });
}

function checkParts(issues: Issues, at: string, parts: ConcatPart[], scope: Scope): void {
  if (parts.length === 0) error(issues, `${at}.parts`, "Add at least one part to join.");
  parts.forEach((part, index) => {
    const partAt = `${at}.parts[${index}]`;
    if ("from" in part) checkPath(issues, `${partAt}.from`, part.from, scope.inLoop, "The path");
    else if (part.text === "") error(issues, `${partAt}.text`, "The text is required.");
  });
}

function checkTimezone(issues: Issues, at: string, zone: string): void {
  if (blank(zone)) return;
  try {
    new Intl.DateTimeFormat("en", { timeZone: zone.trim() });
  } catch {
    error(issues, at, `"${zone}" is not a time zone name such as Europe/Berlin.`);
  }
}

function checkDatetime(
  issues: Issues,
  at: string,
  rule: Extract<MappingRule, { op: "datetime" }>,
): void {
  required(issues, `${at}.input`, rule.input, "The input format");
  required(issues, `${at}.output`, rule.output, "The output format");
  checkTimezone(issues, `${at}.fromTimezone`, rule.fromTimezone);
  checkTimezone(issues, `${at}.toTimezone`, rule.toTimezone);
  if (blank(rule.fromTimezone) !== blank(rule.toTimezone)) {
    warning(
      issues,
      `${at}.toTimezone`,
      "Set both time zones to convert; with only one the time is not converted.",
    );
  }
}

function checkLookup(
  issues: Issues,
  at: string,
  rule: Extract<MappingRule, { op: "lookup" }>,
  scope: Scope,
): void {
  if (blank(rule.table)) error(issues, `${at}.table`, "Choose a code table.");
  else if (!Object.keys(scope.tables).includes(rule.table)) {
    error(issues, `${at}.table`, `Code table "${rule.table}" does not exist.`);
  }
  if (rule.onMissing === "default" && blank(rule.default)) {
    error(issues, `${at}.default`, "A default is required when missing codes use the default.");
  }
}

function formulaProblems(
  issues: Issues,
  at: string,
  formula: Formula,
  scope: Scope,
): { divides: boolean } {
  if ("constant" in formula) {
    if (
      !DECIMAL.test(formula.constant.trim()) &&
      !/^[+-]?(?:\d+\.?|\.\d+)$/.test(formula.constant)
    ) {
      error(issues, at, `"${formula.constant}" is not a decimal number.`);
    }
    return { divides: false };
  }
  if ("find" in formula) {
    checkSegmentName(issues, at, formula.find, "The record to search");
    checkCondition(issues, at, formula.where, true);
    if (!formula.from.trim().startsWith("$.")) {
      error(issues, at, "The value of a found record is a path starting $., such as $.5.");
    }
    return { divides: false };
  }
  if ("from" in formula) {
    checkPath(issues, at, formula.from, scope.inLoop, "The path");
    return { divides: false };
  }
  const found = formula.args.map((arg) => formulaProblems(issues, at, arg, scope));
  return { divides: formula.op === "div" || found.some((each) => each.divides) };
}

function operandCount(formula: Formula): number {
  if ("op" in formula) return formula.args.reduce((sum, arg) => sum + operandCount(arg), 0);
  return "constant" in formula ? 0 : 1;
}

function checkCalculate(
  issues: Issues,
  at: string,
  rule: Extract<MappingRule, { op: "calculate" }>,
  scope: Scope,
): void {
  const { divides } = formulaProblems(issues, `${at}.formula`, rule.formula, scope);
  if (divides && rule.scale === null) {
    error(issues, `${at}.scale`, "A formula with a division needs decimal places.");
  }
  if (rule.scale !== null && (rule.scale < 0 || rule.scale > 12)) {
    error(issues, `${at}.scale`, "Decimal places must be from 0 to 12.");
  }
  if (rule.onInvalid === "keep" && operandCount(rule.formula) !== 1) {
    error(
      issues,
      `${at}.onInvalid`,
      "Writing the value unchanged needs a formula with exactly one operand.",
    );
  }
  if (rule.onInvalid === "default" && blank(rule.default)) {
    error(issues, `${at}.default`, "A default is required when an invalid calculation uses it.");
  }
}

const APPENDABLE = ["copy", "constant", "lookup", "concat", "datetime", "calculate", "sequence"];

function checkAppend(
  issues: Issues,
  at: string,
  rule: Extract<MappingRule, { op: "append" }>,
  scope: Scope,
): void {
  if (scope.inLoop || scope.inAppend) {
    error(issues, `${at}.op`, "append cannot be inside a for_each or another append rule.");
  }
  checkSegmentName(issues, `${at}.into`, rule.into, "The record to add");
  if (rule.rules.length === 0) error(issues, `${at}.rules`, "Add at least one rule.");
  rule.rules.forEach((inner, index) => {
    if (!APPENDABLE.includes(inner.op)) {
      error(issues, `${at}.rules[${index}].op`, `${inner.op} cannot be used inside append.`);
    }
    if (hasTarget(inner) && !inner.to.trim().startsWith("$.")) {
      error(issues, `${at}.rules[${index}].to`, "In an appended record, To is a path starting $.");
    }
  });
  checkRuleList(issues, `${at}.rules`, rule.rules, { ...scope, inLoop: false, inAppend: true });
}

function checkLoopHeader(
  issues: Issues,
  at: string,
  rule: Extract<MappingRule, { op: "for_each" }>,
  scope: Scope,
): void {
  if (scope.inLoop || scope.inAppend) {
    error(issues, `${at}.op`, "for_each cannot be inside a for_each or an append rule.");
  }
  checkSegmentName(issues, `${at}.over`, rule.over, "The record to loop over");
  if (rule.mode !== "repeat") {
    checkSegmentName(issues, `${at}.into`, rule.into, "The record to create");
  } else if (!/^[A-Za-z][A-Za-z0-9_]*(?:\[\d+\])?\.\d+$/.test(rule.into.trim())) {
    error(issues, `${at}.into`, "With repeat, Into is a field path such as O.5.");
  }
}

function checkShape(issues: Issues, at: string, rule: MappingRule, scope: Scope): void {
  switch (rule.op) {
    case "copy":
      checkPath(issues, `${at}.from`, rule.from, scope.inLoop, "The source path");
      return;
    case "constant":
      if (rule.value === "") warning(issues, `${at}.value`, "This writes empty text.");
      return;
    case "lookup":
      checkPath(issues, `${at}.from`, rule.from, scope.inLoop, "The source path");
      checkLookup(issues, at, rule, scope);
      return;
    case "concat":
      checkParts(issues, at, rule.parts, scope);
      return;
    case "calculate":
      checkCalculate(issues, at, rule, scope);
      return;
    case "datetime":
      checkPath(issues, `${at}.from`, rule.from, scope.inLoop, "The source path");
      checkDatetime(issues, at, rule);
      return;
    case "when":
      checkCondition(issues, `${at}.condition`, rule.condition, scope.inLoop);
      checkRuleList(issues, `${at}.then`, rule.then, scope);
      checkRuleList(issues, `${at}.else`, rule.else, scope);
      return;
    case "append":
      checkAppend(issues, at, rule, scope);
      return;
    case "for_each":
      checkLoopHeader(issues, at, rule, scope);
      if (rule.rules.length === 0) warning(issues, `${at}.rules`, "This loop has no rules.");
      checkRuleList(issues, `${at}.rules`, rule.rules, { ...scope, inLoop: true });
      return;
    case "sequence":
      if (!scope.inLoop && !scope.inAppend)
        error(issues, `${at}.op`, "sequence is only valid inside a for_each or append rule.");
      if (!Number.isInteger(rule.start) || rule.start < 0) {
        error(issues, `${at}.start`, "Start must be a whole number, 0 or more.");
      }
  }
}

function hasTarget(rule: MappingRule): rule is Extract<MappingRule, { to: string }> {
  return "to" in rule;
}

function checkRule(issues: Issues, at: string, rule: MappingRule, scope: Scope): void {
  checkShape(issues, at, rule, scope);
  if (hasTarget(rule)) {
    checkPath(
      issues,
      `${at}.to`,
      rule.to,
      scope.inLoop || scope.inAppend === true,
      "The target path",
    );
  }
}

function checkRuleList(issues: Issues, at: string, rules: MappingRule[], scope: Scope): void {
  rules.forEach((rule, index) => {
    checkRule(issues, `${at}[${index}]`, rule, scope);
  });
}

/** A later top-level rule writing the same target replaces the earlier one. */
function checkRepeatedTargets(issues: Issues, path: string, rules: MappingRule[]): void {
  const targets = new Set<string>();
  rules.forEach((rule, index) => {
    if (!hasTarget(rule) || blank(rule.to)) return;
    if (targets.has(rule.to)) {
      warning(
        issues,
        `${path}[${index}].to`,
        `"${rule.to}" is written by an earlier rule too; the later rule wins.`,
      );
    }
    targets.add(rule.to);
  });
}

function checkTables(issues: Issues, path: string, tables: CodeTables): void {
  for (const [name, rows] of Object.entries(tables)) {
    if (blank(name)) error(issues, `${path}.codeTables`, "Every code table needs a name.");
    else if (Object.keys(rows).some((code) => blank(code))) {
      error(issues, `${path}.codeTables.${name}`, `Code table "${name}" has a row without a code.`);
    }
  }
}

/** A destination's mapping and code tables; `path` is the destination's, `channels[0].destinations[1]`. */
function checkRequire(issues: Issues, path: string, list: readonly RequireCondition[]): void {
  list.forEach((entry, index) => {
    const at = `${path}.require[${index}]`;
    if ("same" in entry) {
      if (
        blank(entry.same) ||
        entry.same.includes("$") ||
        /\[\d+\]/.test(entry.same.split(".")[0] ?? "")
      ) {
        error(
          issues,
          `${at}.path`,
          "Use a source path without $. and without an occurrence, such as OBR.3.1.",
        );
      }
      return;
    }
    checkSegmentName(issues, `${at}.record`, entry.count, "The record to count");
    if (entry.min === null && entry.max === null) {
      error(issues, `${at}.min`, "Set a minimum, a maximum or both.");
    } else if (entry.min !== null && entry.max !== null && entry.min > entry.max) {
      error(issues, `${at}.min`, "The minimum is more than the maximum.");
    }
  });
}

export function validateMappings(
  destination: Pick<DestinationConfig, "mappings" | "codeTables" | "require">,
  path: string,
  issues: Issues,
): void {
  checkTables(issues, path, destination.codeTables);
  checkRequire(issues, path, destination.require ?? []);
  const scope: Scope = { tables: destination.codeTables, inLoop: false };
  checkRuleList(issues, `${path}.mappings`, destination.mappings, scope);
  checkRepeatedTargets(issues, `${path}.mappings`, destination.mappings);
}
