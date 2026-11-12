/**
 * Destination mappings and code tables: the console's rule list and the engine's `mapping`.
 *
 * Wire format (docs/spec/configuration.md section 8.2): a destination's `mapping` is `null`
 * (the message passes through) or `{tables, rules}`. Rule JSON is snake_case; empty optional
 * strings, a false `required`, the default `on_missing`, the default `start` and an empty `else`
 * are omitted. A mapping the console cannot show completely (unknown op or step, extra keys, a
 * value of the wrong type) is not shown at all, and the console refuses to
 * overwrite it. The editor's rule ids are made up on read (`m0`, `m0.0`, `m0.1`, ...) and never
 * written; they tell which original rule an edited one came from, so that rules the operator
 * did not touch are written back exactly as they were.
 */

import { clone } from "@/lib/clone";
import type {
  CodeTables,
  ConcatPart,
  LookupOnMissing,
  MappingRule,
  RequireCondition,
  Step,
} from "../types";
import { calculateToJson, parseCalculate } from "./calculate";
import { conditionFromExpression, conditionToExpression } from "./filters";
import { type Json, onlyKeys, optionalStr, some, str, strings } from "./ruleJson";
import { stepsEntry, stepsFromJson, textOf } from "./steps";
import { isEqual, isRecord } from "./util";
import type { WireMapping } from "./wire";

export interface MappingParts {
  mappings: MappingRule[];
  codeTables: CodeTables;
  /** Refuse a message that holds more than one patient, order or specimen group. */
  requireSingleGroup: boolean;
  require: RequireCondition[];
}

/** Original rules by editor id: what was parsed, and the JSON it came from. */
type Originals = Map<string, { rule: MappingRule; json: Json }>;

const ON_MISSING: ReadonlySet<string> = new Set<LookupOnMissing>([
  "error",
  "keep",
  "default",
  "skip",
]);

/** The `steps` entry of a rule when it has some, so that rules without steps stay as they were. */
const withSteps = (steps: Step[]): { steps?: Step[] } => (steps.length === 0 ? {} : { steps });

function parseCopy(json: Json, id: string): MappingRule | null {
  if (!onlyKeys(json, ["op", "from", "to", "default", "required", "steps"])) return null;
  const text = strings(json, ["from", "to"]);
  const fallback = optionalStr(json, "default");
  const required = json.required ?? false;
  const steps = stepsFromJson(json.steps);
  if (text === null || fallback === null || typeof required !== "boolean" || steps === null) {
    return null;
  }
  const [from = "", to = ""] = text;
  return { id, op: "copy", from, to, default: fallback, required, ...withSteps(steps) };
}

function parseConstant(json: Json, id: string): MappingRule | null {
  if (!onlyKeys(json, ["op", "value", "to"])) return null;
  const value = textOf(json.value);
  const to = str(json, "to");
  if (value === null || to === null) return null;
  return { id, op: "constant", value, to };
}

function parseLookup(json: Json, id: string): MappingRule | null {
  const keys = ["op", "from", "table", "to", "on_missing", "default", "ignore_case", "steps"];
  if (!onlyKeys(json, keys)) return null;
  const steps = stepsFromJson(json.steps);
  const text = strings(json, ["from", "table", "to"]);
  const fallback = optionalStr(json, "default");
  const onMissing = json.on_missing ?? "error";
  const ignoreCase = json.ignore_case ?? false;
  if (text === null || fallback === null || typeof ignoreCase !== "boolean") return null;
  if (steps === null) return null;
  if (typeof onMissing !== "string" || !ON_MISSING.has(onMissing)) return null;
  const [from = "", table = "", to = ""] = text;
  return {
    id,
    op: "lookup",
    from,
    table,
    to,
    onMissing: onMissing as LookupOnMissing,
    default: fallback,
    ignoreCase,
    ...withSteps(steps),
  };
}

function parsePart(part: unknown): ConcatPart | null {
  if (!isRecord(part)) return null;
  const keys = Object.keys(part);
  if (keys[0] === "from" && onlyKeys(part, ["from", "steps"]) && typeof part.from === "string") {
    const steps = stepsFromJson(part.steps);
    return steps === null ? null : { from: part.from, ...withSteps(steps) };
  }
  if (keys.length !== 1) return null;
  if (keys[0] === "text") {
    const text = textOf(part.text);
    return text === null ? null : { text };
  }
  return null;
}

function parseConcat(json: Json, id: string): MappingRule | null {
  if (!onlyKeys(json, ["op", "parts", "to"])) return null;
  const to = str(json, "to");
  if (to === null || !Array.isArray(json.parts)) return null;
  const parts = json.parts.map(parsePart);
  if (!parts.every((part): part is ConcatPart => part !== null)) return null;
  return { id, op: "concat", parts, to };
}

function parseDatetime(json: Json, id: string): MappingRule | null {
  const keys = ["op", "from", "to", "input", "output", "from_timezone", "to_timezone", "steps"];
  if (!onlyKeys(json, keys)) return null;
  const steps = stepsFromJson(json.steps);
  const text = strings(json, ["from", "to", "input", "output"]);
  const fromTimezone = optionalStr(json, "from_timezone");
  const toTimezone = optionalStr(json, "to_timezone");
  if (text === null || fromTimezone === null || toTimezone === null || steps === null) return null;
  const [from = "", to = "", input = "", output = ""] = text;
  return {
    id,
    op: "datetime",
    from,
    to,
    input,
    output,
    fromTimezone,
    toTimezone,
    ...withSteps(steps),
  };
}

function parseRules(
  value: unknown,
  parent: string,
  originals: Originals,
  offset = 0,
): MappingRule[] | null {
  if (!Array.isArray(value)) return null;
  const rules: MappingRule[] = [];
  for (const [i, json] of value.entries()) {
    const rule = parseRule(json, parent === "" ? `m${i}` : `${parent}.${offset + i}`, originals);
    if (rule === null) return null;
    rules.push(rule);
  }
  return rules;
}

function parseWhen(json: Json, id: string, originals: Originals): MappingRule | null {
  if (!onlyKeys(json, ["op", "condition", "then", "else"])) return null;
  const condition = conditionFromExpression(json.condition);
  const thenRules = parseRules(json.then, id, originals);
  const elseRules =
    json.else === undefined ? [] : parseRules(json.else, id, originals, thenRules?.length);
  if (condition === null || thenRules === null || elseRules === null) return null;
  // biome-ignore lint/suspicious/noThenProperty: `then` is the rule's own field, in the engine's JSON too
  return { id, op: "when", condition, then: thenRules, else: elseRules };
}

function parseForEach(json: Json, id: string, originals: Originals): MappingRule | null {
  if (!onlyKeys(json, ["op", "over", "into", "rules", "mode"])) return null;
  const text = strings(json, ["over", "into"]);
  const rules = parseRules(json.rules, id, originals);
  const mode = json.mode ?? "create";
  if (text === null || rules === null) return null;
  if (mode !== "create" && mode !== "update" && mode !== "repeat") return null;
  const [over = "", into = ""] = text;
  return { id, op: "for_each", over, into, rules, ...(mode === "create" ? {} : { mode }) };
}

function parseAppend(json: Json, id: string, originals: Originals): MappingRule | null {
  if (!onlyKeys(json, ["op", "into", "rules"])) return null;
  const into = str(json, "into");
  const rules = parseRules(json.rules, id, originals);
  return into === null || rules === null ? null : { id, op: "append", into, rules };
}

function parseSequence(json: Json, id: string): MappingRule | null {
  if (!onlyKeys(json, ["op", "to", "start"])) return null;
  const to = str(json, "to");
  const start = json.start ?? 1;
  if (to === null || typeof start !== "number" || !Number.isInteger(start)) return null;
  return { id, op: "sequence", to, start };
}

function parseOp(json: Json, id: string, originals: Originals): MappingRule | null {
  switch (json.op) {
    case "copy":
      return parseCopy(json, id);
    case "constant":
      return parseConstant(json, id);
    case "lookup":
      return parseLookup(json, id);
    case "concat":
      return parseConcat(json, id);
    case "datetime":
      return parseDatetime(json, id);
    case "calculate":
      return parseCalculate(json, id);
    case "when":
      return parseWhen(json, id, originals);
    case "for_each":
      return parseForEach(json, id, originals);
    case "append":
      return parseAppend(json, id, originals);
    case "sequence":
      return parseSequence(json, id);
    default:
      return null;
  }
}

function parseRule(json: unknown, id: string, originals: Originals): MappingRule | null {
  if (!isRecord(json) || typeof json.op !== "string") return null;
  const rule = parseOp(json, id, originals);
  if (rule !== null) originals.set(id, { rule, json });
  return rule;
}

/**
 * The engine JSON of one rule; the editor id is dropped. `child` writes the rules inside
 * `when` and `for_each`.
 */
export function ruleToJson(
  rule: MappingRule,
  child: (rule: MappingRule) => Json = ruleToJson,
): Json {
  switch (rule.op) {
    case "copy":
      return {
        op: "copy",
        from: rule.from,
        to: rule.to,
        ...some("default", rule.default, rule.default === ""),
        ...some("required", true, !rule.required),
        ...stepsEntry(rule.steps),
      };
    case "constant":
      return { op: "constant", value: rule.value, to: rule.to };
    case "lookup":
      return {
        op: "lookup",
        from: rule.from,
        table: rule.table,
        to: rule.to,
        ...some("on_missing", rule.onMissing, rule.onMissing === "error"),
        ...some("default", rule.default, rule.default === ""),
        ...some("ignore_case", true, !rule.ignoreCase),
        ...stepsEntry(rule.steps),
      };
    case "concat":
      return {
        op: "concat",
        parts: rule.parts.map((part) =>
          "from" in part ? { from: part.from, ...stepsEntry(part.steps) } : { text: part.text },
        ),
        to: rule.to,
      };
    case "datetime":
      return {
        op: "datetime",
        from: rule.from,
        to: rule.to,
        input: rule.input,
        output: rule.output,
        ...some("from_timezone", rule.fromTimezone, rule.fromTimezone === ""),
        ...some("to_timezone", rule.toTimezone, rule.toTimezone === ""),
        ...stepsEntry(rule.steps),
      };
    case "calculate":
      return calculateToJson(rule);
    case "when":
      return {
        op: "when",
        condition: conditionToExpression(rule.condition),
        // biome-ignore lint/suspicious/noThenProperty: `then` is the rule's own field, in the engine's JSON too
        then: rule.then.map((r) => child(r)),
        ...some(
          "else",
          rule.else.map((r) => child(r)),
          rule.else.length === 0,
        ),
      };
    case "for_each":
      return {
        op: "for_each",
        over: rule.over,
        into: rule.into,
        ...some("mode", rule.mode, rule.mode === undefined),
        rules: rule.rules.map((r) => child(r)),
      };
    case "append":
      return { op: "append", into: rule.into, rules: rule.rules.map((r) => child(r)) };
    case "sequence":
      return { op: "sequence", to: rule.to, ...some("start", rule.start, rule.start === 1) };
  }
}

const isLimit = (n: unknown) => n === null || (typeof n === "number" && Number.isInteger(n));

function requirementFromJson(entry: unknown): RequireCondition | null {
  if (!isRecord(entry)) return null;
  if ("same" in entry) {
    return onlyKeys(entry, ["same"]) && typeof entry.same === "string"
      ? { same: entry.same }
      : null;
  }
  const { count, min = null, max = null } = entry;
  if (!onlyKeys(entry, ["count", "min", "max"]) || typeof count !== "string") return null;
  if (!isLimit(min) || !isLimit(max)) return null;
  return { count, min: min as number | null, max: max as number | null };
}

/** The `require` list of a mapping (`undefined` is none), or null when an entry is not readable. */
function requireFromJson(value: unknown): RequireCondition[] | null {
  if (value === undefined) return [];
  if (!Array.isArray(value)) return null;
  const out = value.map(requirementFromJson);
  return out.every((entry): entry is RequireCondition => entry !== null) ? out : null;
}

function requireToJson(list: readonly RequireCondition[]): Json[] {
  return list.map((entry) =>
    "same" in entry
      ? { same: entry.same }
      : {
          count: entry.count,
          ...some("min", entry.min, entry.min === null),
          ...some("max", entry.max, entry.max === null),
        },
  );
}

function parseTables(value: unknown): CodeTables | null {
  if (value === undefined) return {};
  if (!isRecord(value)) return null;
  const tables: CodeTables = {};
  for (const [name, codes] of Object.entries(value)) {
    if (!isRecord(codes) || !Object.values(codes).every((v) => typeof v === "string")) return null;
    tables[name] = { ...(codes as Record<string, string>) };
  }
  return tables;
}

function parseMapping(mapping: WireMapping, originals: Originals): MappingParts | null {
  const keys = ["tables", "rules", "require_single_group", "require"];
  if (!isRecord(mapping) || !onlyKeys(mapping, keys)) return null;
  const codeTables = parseTables(mapping.tables);
  const rules = parseRules(mapping.rules, "", originals);
  const single = mapping.require_single_group ?? false;
  const require = requireFromJson(mapping.require);
  if (codeTables === null || rules === null || typeof single !== "boolean" || require === null) {
    return null;
  }
  return { mappings: rules, codeTables, requireSingleGroup: single, require };
}

/**
 * What the console shows of a destination's mapping: nothing for `null`, null when the
 * mapping holds something the console cannot show.
 */
export function mappingFromWire(mapping: WireMapping | null | undefined): MappingParts | null {
  if (mapping === null || mapping === undefined) {
    return { mappings: [], codeTables: {}, requireSingleGroup: false, require: [] };
  }
  const originals: Originals = new Map();
  return parseMapping(mapping, originals);
}

export const isEmptyMapping = ({ mappings, codeTables }: MappingParts): boolean =>
  mappings.length === 0 && Object.keys(codeTables).length === 0;

/** Whether two mappings are the same to the console (editor ids do not count). */
export function sameMapping(a: MappingParts, b: MappingParts): boolean {
  return (
    isEqual(
      a.mappings.map((rule) => ruleToJson(rule)),
      b.mappings.map((rule) => ruleToJson(rule)),
    ) &&
    isEqual(a.codeTables, b.codeTables) &&
    a.requireSingleGroup === b.requireSingleGroup &&
    isEqual(a.require, b.require)
  );
}

/** The flag stays as it was written (an explicit `false` included) while it has not changed. */
function singleGroupEntry(
  parts: MappingParts,
  was: MappingParts | null,
  original: WireMapping | null | undefined,
): Json {
  const written = original?.require_single_group;
  if (was?.requireSingleGroup === parts.requireSingleGroup && written !== undefined) {
    return { require_single_group: written };
  }
  return parts.requireSingleGroup ? { require_single_group: true } : {};
}

/**
 * The engine `mapping` for `parts`: `null` when there is nothing in it. `original` is the
 * mapping the console read (so that rules and tables the operator did not touch are written
 * exactly as they were).
 */
export function mappingToWire(
  parts: MappingParts,
  original: WireMapping | null | undefined,
): WireMapping | null {
  if (isEmptyMapping(parts)) return null;
  const originals: Originals = new Map();
  const was =
    original === null || original === undefined ? null : parseMapping(original, originals);

  const rule = (r: MappingRule): Json => {
    const before = originals.get(r.id);
    if (before !== undefined && isEqual(ruleToJson(r), ruleToJson(before.rule))) {
      return clone(before.json);
    }
    return ruleToJson(r, rule);
  };

  const sameTables = was !== null && isEqual(was.codeTables, parts.codeTables);
  const tables = sameTables ? original?.tables : parts.codeTables;
  const flag = singleGroupEntry(parts, was, original);
  const sameRequire = was !== null && isEqual(was.require, parts.require);
  const require = sameRequire ? original?.require : requireToJson(parts.require);
  return {
    ...flag,
    ...(require === undefined || require.length === 0 ? {} : { require: clone(require) }),
    ...(tables === undefined || (!sameTables && Object.keys(tables).length === 0)
      ? {}
      : { tables: clone(tables) }),
    rules: parts.mappings.map(rule),
  };
}
