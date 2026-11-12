import { describe, expect, it } from "vitest";
import type { MappingRule } from "../types";
import {
  isEmptyMapping,
  mappingFromWire,
  mappingToWire,
  ruleToJson,
  sameMapping,
} from "./mappings";
import { MAPPING } from "./test/configFixtures";
import type { WireMapping } from "./wire";

describe("reading a mapping", () => {
  it("reads every rule kind with deterministic editor ids", () => {
    const parts = mappingFromWire(MAPPING);
    expect(parts?.mappings.map((m) => [m.id, m.op])).toEqual([
      ["m0", "copy"],
      ["m1", "copy"],
      ["m2", "constant"],
      ["m3", "lookup"],
      ["m4", "concat"],
      ["m5", "datetime"],
      ["m6", "when"],
      ["m7", "for_each"],
      ["m8", "sequence"],
    ]);
    const when = parts?.mappings[6];
    expect(when?.op === "when" && [when.then.map((r) => r.id), when.else.map((r) => r.id)]).toEqual(
      [["m6.0"], ["m6.1"]],
    );
    const each = parts?.mappings[7];
    expect(each?.op === "for_each" && each.rules.map((r) => r.id)).toEqual([
      "m7.0",
      "m7.1",
      "m7.2",
    ]);
    expect(parts?.mappings[0]).toEqual({
      id: "m0",
      op: "copy",
      from: "PID.3",
      to: "R.3",
      default: "n/a",
      required: true,
    });
    expect(parts?.mappings[1]).toMatchObject({ default: "", required: false });
    expect(parts?.mappings[5]).toMatchObject({ fromTimezone: "UTC", toTimezone: "Europe/Berlin" });
    expect(parts?.mappings[6]).toMatchObject({
      condition: { path: "PV1.2", operator: "in", value: "I, E", ignoreCase: true },
    });
    expect(parts?.codeTables).toEqual({ sex: { M: "male", F: "female" }, empty: {} });
  });

  it("shows nothing for a pass-through destination", () => {
    for (const mapping of [null, undefined]) {
      expect(mappingFromWire(mapping)).toEqual({
        mappings: [],
        codeTables: {},
        requireSingleGroup: false,
        require: [],
      });
    }
  });

  it("applies the specified defaults", () => {
    const parts = mappingFromWire({
      rules: [
        { op: "lookup", from: "A", table: "t", to: "B" },
        { op: "sequence", to: "C" },
      ],
    });
    expect(parts?.mappings).toEqual([
      {
        id: "m0",
        op: "lookup",
        from: "A",
        table: "t",
        to: "B",
        onMissing: "error",
        default: "",
        ignoreCase: false,
      },
      { id: "m1", op: "sequence", to: "C", start: 1 },
    ]);
    expect(parts?.codeTables).toEqual({});
  });

  it("reads lookup ignore_case and numbers used as text, and writes them back", () => {
    const original: WireMapping = {
      tables: { sex: { M: "1" } },
      rules: [
        { op: "lookup", from: "A", table: "sex", to: "B", ignore_case: true },
        { op: "constant", value: 5, to: "C" },
        { op: "copy", from: "A", to: "D", default: 0 },
        { op: "concat", parts: [{ from: "A" }, { text: 7 }], to: "E" },
      ],
    };
    const parts = mappingFromWire(original);
    expect(parts?.mappings.map((rule) => rule.op)).toEqual([
      "lookup",
      "constant",
      "copy",
      "concat",
    ]);
    expect(parts?.mappings[0]).toMatchObject({ ignoreCase: true });
    expect(parts?.mappings[1]).toMatchObject({ value: "5" });
    expect(parts?.mappings[2]).toMatchObject({ default: "0" });
    // Untouched, the rules come back as they were; edited, ignore_case is written when set.
    expect(mappingToWire(parts as NonNullable<typeof parts>, original)).toEqual(original);
    const lookup = parts?.mappings[0];
    if (lookup?.op !== "lookup") throw new Error("fixture");
    expect(ruleToJson({ ...lookup, ignoreCase: false })).not.toHaveProperty("ignore_case");
    expect(ruleToJson(lookup)).toHaveProperty("ignore_case", true);
  });

  it("reads the contract's calculate examples and writes them back sparsely", () => {
    const original: WireMapping = {
      rules: [
        {
          op: "calculate",
          formula: { mul: [{ from: "$.4" }, "0.0555"] },
          to: "$.5",
          scale: 2,
          rounding: "half_up",
          comparator: "carry",
          on_invalid: "skip",
          required: false,
        },
        {
          op: "calculate",
          formula: { div: [{ from: "OBX[1].5" }, 88.4] },
          to: "OBX[1].5",
          scale: 2,
        },
        {
          op: "calculate",
          formula: { abs: { sub: [{ from: "A" }, "1"] } },
          to: "B",
          on_invalid: "default",
          default: 0,
        },
        { op: "calculate", formula: { from: "A" }, to: "A", on_invalid: "keep" },
      ],
    };
    const parts = mappingFromWire(original);
    expect(parts?.mappings[0]).toMatchObject({
      op: "calculate",
      formula: { op: "mul", args: [{ from: "$.4", steps: [] }, { constant: "0.0555" }] },
      scale: 2,
      rounding: "half_up",
      comparator: "carry",
      onInvalid: "skip",
    });
    // Absent fields are the engine's defaults: a comparator is carried, an invalid value fails.
    expect(parts?.mappings[1]).toMatchObject({
      formula: { op: "div", args: [{ from: "OBX[1].5", steps: [] }, { constant: "88.4" }] },
      scale: 2,
      comparator: "carry",
      onInvalid: "error",
      required: false,
    });
    expect(parts?.mappings[2]).toMatchObject({
      formula: {
        op: "abs",
        args: [{ op: "sub", args: [{ from: "A", steps: [] }, { constant: "1" }] }],
      },
      scale: null,
      default: "0",
    });
    expect(parts?.mappings[3]).toMatchObject({ onInvalid: "keep" });
    // Untouched rules come back as they were (the number 88.4 stays a number)...
    expect(mappingToWire(parts as NonNullable<typeof parts>, original)).toEqual(original);
    // ...and a rule the operator built is written without its defaults, with constants as text.
    const rules = (parts?.mappings ?? []).map((rule) => ruleToJson(rule));
    expect(rules).toEqual([
      {
        op: "calculate",
        formula: { mul: [{ from: "$.4" }, "0.0555"] },
        to: "$.5",
        scale: 2,
        on_invalid: "skip",
      },
      {
        op: "calculate",
        formula: { div: [{ from: "OBX[1].5" }, "88.4"] },
        to: "OBX[1].5",
        scale: 2,
      },
      {
        op: "calculate",
        formula: { abs: { sub: [{ from: "A" }, "1"] } },
        to: "B",
        on_invalid: "default",
        default: "0",
      },
      { op: "calculate", formula: { from: "A" }, to: "A", on_invalid: "keep" },
    ]);
  });

  it("keeps the operator's choice of reject: the engine's default is carry", () => {
    const rule = mappingFromWire({
      rules: [{ op: "calculate", formula: "1", to: "A", comparator: "reject" }],
    })?.mappings[0];
    if (rule?.op !== "calculate") throw new Error("fixture");
    expect(rule.comparator).toBe("reject");
    expect(ruleToJson(rule)).toEqual({
      op: "calculate",
      formula: "1",
      to: "A",
      comparator: "reject",
    });
    const carried = ruleToJson({ ...rule, comparator: "carry" });
    expect(carried).not.toHaveProperty("comparator");
    // What is written, read again, is the same choice.
    for (const comparator of ["reject", "carry"] as const) {
      const json = ruleToJson({ ...rule, comparator });
      const again = mappingFromWire({ rules: [json] })?.mappings[0];
      expect(again).toMatchObject({ comparator });
    }
  });

  it("reads and writes every operand and step of the contract", () => {
    const original: WireMapping = {
      rules: [
        {
          op: "append",
          into: "OBX",
          rules: [
            {
              op: "calculate",
              to: "$.5",
              on_invalid: "skip",
              formula: {
                sub: [
                  {
                    find: "OBX",
                    where: { path: "$.3.1", op: "equals", value: "NA" },
                    from: "$.5",
                    steps: [{ step: "replace", find: ",", with: "." }],
                  },
                  { from: "OBX[2].5", steps: [{ step: "trim" }] },
                ],
              },
            },
            { op: "sequence", to: "$.1" },
            { op: "constant", value: "AG^Anion gap", to: "$.3" },
          ],
        },
        {
          op: "copy",
          from: "PID.3",
          to: "PID.4",
          steps: [
            { step: "strip_leading", chars: "0" },
            { step: "split", separator: "/", take: -1 },
            { step: "slice", start: 0, length: 20 },
            { step: "slice", start: 2 },
            { step: "upper" },
          ],
        },
        {
          op: "concat",
          parts: [{ from: "A", steps: [{ step: "lower" }] }, { text: "-" }, { from: "B" }],
          to: "C",
        },
        { op: "lookup", from: "A", table: "t", to: "B", steps: [{ step: "trim" }] },
        {
          op: "datetime",
          from: "A",
          to: "B",
          input: "yyyy",
          output: "yy",
          steps: [{ step: "trim" }],
        },
      ],
    };
    const parts = mappingFromWire(original);
    expect(parts?.mappings.map((rule) => rule.op)).toEqual([
      "append",
      "copy",
      "concat",
      "lookup",
      "datetime",
    ]);
    const appended = parts?.mappings[0];
    expect(appended?.op === "append" && appended.rules.map((rule) => rule.op)).toEqual([
      "calculate",
      "sequence",
      "constant",
    ]);
    expect(mappingToWire(parts as NonNullable<typeof parts>, original)).toEqual(original);
    // Written from the console's model, the same JSON comes out.
    const rules = (parts?.mappings ?? []).map((rule) => ruleToJson(rule));
    expect(rules).toEqual(original.rules);
  });

  it("does not write empty steps and writes slice without length when there is none", () => {
    const rule = mappingFromWire({
      rules: [{ op: "copy", from: "A", to: "B", steps: [{ step: "slice", start: 1 }] }],
    })?.mappings[0];
    if (rule?.op !== "copy") throw new Error("fixture");
    expect(ruleToJson(rule)).toEqual({
      op: "copy",
      from: "A",
      to: "B",
      steps: [{ step: "slice", start: 1 }],
    });
    expect(ruleToJson({ ...rule, steps: [] })).toEqual({ op: "copy", from: "A", to: "B" });
  });

  it("reads and writes for_each mode update and calculate notation sn", () => {
    const original: WireMapping = {
      rules: [
        {
          op: "for_each",
          over: "OBX",
          into: "OBX",
          mode: "update",
          rules: [
            { op: "calculate", notation: "sn", formula: { from: "$.5" }, to: "$.5", scale: 1 },
            { op: "sequence", to: "$.1" },
          ],
        },
        { op: "for_each", over: "R", into: "OBX", mode: "create", rules: [] },
      ],
    };
    const parts = mappingFromWire(original);
    const [loop, plain] = parts?.mappings ?? [];
    expect(loop).toMatchObject({ op: "for_each", mode: "update" });
    expect(plain && "mode" in plain).toBe(false);
    expect(mappingToWire(parts as NonNullable<typeof parts>, original)).toEqual(original);
    // Written from the model: `create` and `text` are the defaults and stay out.
    expect((parts?.mappings ?? []).map((rule) => ruleToJson(rule))).toEqual([
      original.rules?.[0],
      { op: "for_each", over: "R", into: "OBX", rules: [] },
    ]);
    expect(
      mappingFromWire({
        rules: [{ op: "for_each", over: "R", into: "R", mode: "sideways", rules: [] }],
      }),
    ).toBeNull();
    expect(
      mappingFromWire({ rules: [{ op: "calculate", formula: "1", to: "A", notation: "xml" }] }),
    ).toBeNull();
  });

  it("round-trips the mapping's require_single_group, as written", () => {
    const original: WireMapping = {
      require_single_group: true,
      rules: [{ op: "constant", value: "X", to: "A" }],
    };
    const parts = mappingFromWire(original);
    expect(parts?.requireSingleGroup).toBe(true);
    expect(mappingToWire(parts as NonNullable<typeof parts>, original)).toEqual(original);
    // Cleared, it is no longer written; an explicit false that was not touched stays.
    expect(
      mappingToWire(
        { ...(parts as NonNullable<typeof parts>), requireSingleGroup: false, require: [] },
        original,
      ),
    ).toEqual({
      rules: [{ op: "constant", value: "X", to: "A" }],
    });
    const explicit: WireMapping = { require_single_group: false, rules: [] };
    const shown = mappingFromWire({
      ...explicit,
      rules: [{ op: "constant", value: "X", to: "A" }],
    });
    expect(shown?.requireSingleGroup).toBe(false);
    expect(
      mappingFromWire({ rules: [], require_single_group: "yes" } as unknown as WireMapping),
    ).toBeNull();
    // Set on a mapping that had none, it is written.
    expect(
      mappingToWire(
        { mappings: shown?.mappings ?? [], codeTables: {}, requireSingleGroup: true, require: [] },
        null,
      ),
    ).toEqual({ require_single_group: true, rules: [{ op: "constant", value: "X", to: "A" }] });
  });

  it("round-trips source requirements and the repeat mode of a loop", () => {
    const original: WireMapping = {
      require: [{ count: "PID", max: 1 }, { count: "OBR", min: 1, max: 18 }, { same: "OBR.3.1" }],
      rules: [
        {
          op: "for_each",
          over: "OBR",
          into: "O.5",
          mode: "repeat",
          rules: [{ op: "copy", from: "$.4.1", to: "$.5.4", required: true }],
        },
      ],
    };
    const parts = mappingFromWire(original);
    expect(parts?.require).toEqual([
      { count: "PID", min: null, max: 1 },
      { count: "OBR", min: 1, max: 18 },
      { same: "OBR.3.1" },
    ]);
    expect(parts?.mappings[0]).toMatchObject({ op: "for_each", mode: "repeat", into: "O.5" });
    expect(mappingToWire(parts as NonNullable<typeof parts>, original)).toEqual(original);
    // Edited: written sparsely, in order; cleared: the key is gone.
    const edited = mappingToWire(
      { ...(parts as NonNullable<typeof parts>), require: [{ count: "OBR", min: 2, max: null }] },
      original,
    );
    expect(edited?.require).toEqual([{ count: "OBR", min: 2 }]);
    expect(
      mappingToWire({ ...(parts as NonNullable<typeof parts>), require: [] }, original),
    ).not.toHaveProperty("require");
    for (const bad of [
      [{ count: "OBR", min: "1" }],
      [{ same: "A", count: "B" }],
      [{ count: "OBR", extra: 1 }],
      "OBR",
    ]) {
      expect(mappingFromWire({ rules: [], require: bad } as unknown as WireMapping)).toBeNull();
    }
  });

  it("does not write rounding without decimal places", () => {
    const rule = mappingFromWire({
      rules: [{ op: "calculate", formula: "1", to: "A", scale: 1, rounding: "truncate" }],
    })?.mappings[0];
    if (rule?.op !== "calculate") throw new Error("fixture");
    expect(ruleToJson({ ...rule, scale: null })).toEqual({
      op: "calculate",
      formula: "1",
      to: "A",
    });
  });

  it.each([
    ["a formula with two keys", { add: ["1", "2"], mul: ["1", "2"] }],
    ["an unknown formula operator", { pow: ["1", "2"] }],
    ["an operator over a non-list", { add: "1" }],
    ["an unreadable operand", { add: [["1"], "2"] }],
    ["an operand with an extra key", { from: "A", note: "x" }],
    ["a find without its path", { find: "OBX", where: { path: "A", op: "exists" } }],
    [
      "a find with an unreadable condition",
      { find: "OBX", where: { path: "A", op: "near" }, from: "$.5" },
    ],
    ["a step the console does not know", { from: "A", steps: [{ step: "reverse" }] }],
    ["a step with an extra key", { from: "A", steps: [{ step: "trim", all: true }] }],
    ["a step list that is not a list", { from: "A", steps: "trim" }],
    ["a formula that is not a value", true],
  ])("refuses to show a calculate with %s", (_, formula) => {
    expect(mappingFromWire({ rules: [{ op: "calculate", formula, to: "A" }] })).toBeNull();
  });

  it.each([
    ["an unknown step", { op: "copy", from: "A", to: "B", steps: [{ step: "reverse" }] }],
    [
      "a split without take",
      { op: "copy", from: "A", to: "B", steps: [{ step: "split", separator: "/" }] },
    ],
    ["an append with an unknown key", { op: "append", into: "OBX", rules: [], extra: 1 }],
    ["an append without into", { op: "append", rules: [] }],
    [
      "on_invalid of an unknown kind",
      { op: "calculate", formula: "1", to: "A", on_invalid: "ignore" },
    ],
  ])("does not show a mapping with %s", (_, rule) => {
    expect(mappingFromWire({ rules: [rule] })).toBeNull();
  });

  it.each([
    ["an unknown operation", { rules: [{ op: "explode", to: "A" }] }],
    ["an unknown field", { rules: [{ op: "constant", value: "1", to: "A", note: "x" }] }],
    [
      "a condition with an unknown operator",
      // biome-ignore lint/suspicious/noThenProperty: `then` is the rule's own field
      { rules: [{ op: "when", condition: { path: "A", op: "near" }, then: [] }] },
    ],
    ["a table that is not text", { tables: { t: { a: 1 } }, rules: [] }],
    ["no rules", { tables: {} }],
    ["an unknown key", { rules: [], extra: true }],
  ])("does not show %s", (_name, mapping) => {
    expect(mappingFromWire(mapping as unknown as WireMapping)).toBeNull();
  });
});

describe("writing a mapping", () => {
  it("writes back exactly what it read", () => {
    const parts = mappingFromWire(MAPPING);
    if (!parts) throw new Error("fixture");
    expect(mappingToWire(parts, MAPPING)).toEqual(MAPPING);
  });

  it("writes the sparse form of rules that have no original", () => {
    const rules: MappingRule[] = [
      { id: "a", op: "copy", from: "A", to: "B", default: "", required: false },
      {
        id: "b",
        op: "lookup",
        from: "A",
        table: "t",
        to: "B",
        onMissing: "error",
        default: "",
        ignoreCase: false,
      },
      {
        id: "b2",
        op: "lookup",
        from: "A",
        table: "t",
        to: "B",
        onMissing: "keep",
        default: "",
        ignoreCase: false,
      },
      {
        id: "c",
        op: "datetime",
        from: "A",
        to: "B",
        input: "yyyy",
        output: "yy",
        fromTimezone: "",
        toTimezone: "",
      },
      {
        id: "d",
        op: "when",
        condition: { path: "A", operator: "exists", value: "", ignoreCase: false },
        // biome-ignore lint/suspicious/noThenProperty: `then` is the rule's own field
        then: [{ id: "e", op: "constant", value: "1", to: "B" }],
        else: [],
      },
      { id: "f", op: "sequence", to: "C", start: 1 },
      { id: "g", op: "sequence", to: "C", start: 3 },
    ];
    expect(rules.map((rule) => ruleToJson(rule))).toEqual([
      { op: "copy", from: "A", to: "B" },
      { op: "lookup", from: "A", table: "t", to: "B" },
      { op: "lookup", from: "A", table: "t", to: "B", on_missing: "keep" },
      { op: "datetime", from: "A", to: "B", input: "yyyy", output: "yy" },
      {
        op: "when",
        condition: { path: "A", op: "exists" },
        // biome-ignore lint/suspicious/noThenProperty: `then` is the rule's own field
        then: [{ op: "constant", value: "1", to: "B" }],
      },
      { op: "sequence", to: "C" },
      { op: "sequence", to: "C", start: 3 },
    ]);
  });

  it("is null when nothing is left, and keeps tables when only they remain", () => {
    expect(
      mappingToWire(
        { mappings: [], codeTables: {}, requireSingleGroup: false, require: [] },
        MAPPING,
      ),
    ).toBeNull();
    expect(
      isEmptyMapping({ mappings: [], codeTables: {}, requireSingleGroup: false, require: [] }),
    ).toBe(true);
    expect(
      mappingToWire(
        { mappings: [], codeTables: { t: { a: "b" } }, requireSingleGroup: false, require: [] },
        null,
      ),
    ).toEqual({
      tables: { t: { a: "b" } },
      rules: [],
    });
  });

  it("leaves out empty tables of a new mapping", () => {
    const mapping = mappingToWire(
      {
        mappings: [{ id: "a", op: "constant", value: "1", to: "B" }],
        codeTables: {},
        requireSingleGroup: false,
        require: [],
      },
      undefined,
    );
    expect(mapping).toEqual({ rules: [{ op: "constant", value: "1", to: "B" }] });
  });

  it("writes the rules the operator did not touch exactly as they were", () => {
    // The first rule spells out defaults that the sparse form would drop.
    const original: WireMapping = {
      tables: { t: { a: "b" } },
      rules: [
        { op: "copy", from: "A", to: "B", required: false },
        { op: "lookup", from: "A", table: "t", to: "C", on_missing: "error" },
        { op: "sequence", to: "D", start: 1 },
      ],
    };
    const parts = mappingFromWire(original);
    if (!parts) throw new Error("fixture");
    const [, second, third] = parts.mappings;
    if (!second || !third || second.op !== "lookup") throw new Error("fixture");
    const edited = mappingToWire(
      { ...parts, mappings: [third, { ...second, onMissing: "skip" }] },
      original,
    );
    expect(edited).toEqual({
      tables: { t: { a: "b" } },
      rules: [
        { op: "sequence", to: "D", start: 1 },
        { op: "lookup", from: "A", table: "t", to: "C", on_missing: "skip" },
      ],
    });
  });

  it("keeps the unchanged rules inside a container that changed", () => {
    const original: WireMapping = {
      rules: [
        {
          op: "for_each",
          over: "OBX",
          into: "R",
          rules: [
            { op: "copy", from: "$.5", to: "$.4", required: false },
            { op: "copy", from: "$.6", to: "$.5" },
          ],
        },
      ],
    };
    const parts = mappingFromWire(original);
    const loop = parts?.mappings[0];
    if (!parts || loop?.op !== "for_each" || loop.rules[1]?.op !== "copy") {
      throw new Error("fixture");
    }
    const changed = {
      ...loop,
      rules: [loop.rules[0] as MappingRule, { ...loop.rules[1], to: "$.9" }],
    };
    expect(mappingToWire({ ...parts, mappings: [changed] }, original)).toEqual({
      rules: [
        {
          op: "for_each",
          over: "OBX",
          into: "R",
          rules: [
            { op: "copy", from: "$.5", to: "$.4", required: false },
            { op: "copy", from: "$.6", to: "$.9" },
          ],
        },
      ],
    });
  });

  it("does not hand out the original objects", () => {
    const parts = mappingFromWire(MAPPING);
    if (!parts) throw new Error("fixture");
    const written = mappingToWire(parts, MAPPING);
    expect(written?.rules[0]).not.toBe(MAPPING.rules[0]);
    expect(written?.tables).not.toBe(MAPPING.tables);
  });
});

describe("sameMapping", () => {
  it("ignores editor ids", () => {
    const parts = mappingFromWire(MAPPING);
    if (!parts) throw new Error("fixture");
    const renamed = { ...parts, mappings: parts.mappings.map((m) => ({ ...m, id: `z-${m.id}` })) };
    expect(sameMapping(parts, renamed)).toBe(true);
    expect(sameMapping(parts, { ...parts, codeTables: {} })).toBe(false);
  });
});
