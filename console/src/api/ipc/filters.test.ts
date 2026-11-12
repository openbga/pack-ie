import { describe, expect, it } from "vitest";
import type { Comparison, Condition, FilterRule } from "../types";
import {
  conditionFromExpression,
  conditionToExpression,
  filtersFromWire,
  filtersToWire,
  sameFilters,
  visibleFilterIndex,
} from "./filters";

const rule = (
  extra: Partial<Comparison> & Pick<Comparison, "path" | "operator"> & { id?: string },
): FilterRule => ({
  id: "x",
  value: "",
  ignoreCase: false,
  ...extra,
});

describe("filter expressions", () => {
  it.each([
    [
      { path: "A.1", op: "equals", value: "x" },
      { path: "A.1", operator: "equals", value: "x", ignoreCase: false },
    ],
    [
      { path: "A.1", op: "in", value: ["a", "b"] },
      { path: "A.1", operator: "in", value: "a, b", ignoreCase: false },
    ],
    [
      { path: "A.1", op: "not_in", value: [] },
      { path: "A.1", operator: "not_in", value: "", ignoreCase: false },
    ],
    [
      { path: "A.1", op: "exists" },
      { path: "A.1", operator: "exists", value: "", ignoreCase: false },
    ],
    [
      { path: "A.1", op: "contains", value: "x", ignore_case: true },
      { path: "A.1", operator: "contains", value: "x", ignoreCase: true },
    ],
    [
      { path: "A.1", op: "gte", value: "1.50" },
      { path: "A.1", operator: "gte", value: "1.50", ignoreCase: false },
    ],
  ])("reads %j", (expression, expected) => {
    expect(conditionFromExpression(expression)).toEqual(expected);
  });

  it.each([
    [
      "all of",
      {
        all: [
          { path: "A", op: "exists" },
          { path: "B", op: "equals", value: 5 },
        ],
      },
      {
        all: [
          { path: "A", operator: "exists", value: "", ignoreCase: false },
          { path: "B", operator: "equals", value: "5", ignoreCase: false },
        ],
      },
      {
        all: [
          { path: "A", op: "exists" },
          { path: "B", op: "equals", value: "5" },
        ],
      },
    ],
    [
      "not any of",
      { not: { any: [{ path: "A", op: "in", value: ["x", 7] }] } },
      { not: { any: [{ path: "A", operator: "in", value: "x, 7", ignoreCase: false }] } },
      { not: { any: [{ path: "A", op: "in", value: ["x", "7"] }] } },
    ],
    ["an empty combination", { any: [] }, { any: [] }, { any: [] }],
  ])("reads %s", (_name, expression, expected, written) => {
    expect(conditionFromExpression(expression)).toEqual(expected);
    expect(conditionToExpression(expected as Condition)).toEqual(written);
  });

  it.each([
    ["a combination with a sibling key", { all: [], any: [] }],
    ["a combination that is not a list", { all: { path: "A", op: "exists" } }],
    ["a negation of two", { not: [{ path: "A", op: "exists" }] }],
    ["an unreadable item", { all: [{ path: "A", op: "near" }] }],
    ["an unknown operator", { path: "A", op: "near", value: "x" }],
    ["a value that is not text", { path: "A", op: "equals", value: true }],
    ["a list that is not text", { path: "A", op: "in", value: ["a", null] }],
    ["a value on exists", { path: "A", op: "exists", value: "x" }],
    ["a missing value", { path: "A", op: "equals" }],
    ["an unknown key", { path: "A", op: "equals", value: "x", extra: 1 }],
    ["a text ignore_case", { path: "A", op: "equals", value: "x", ignore_case: "yes" }],
    ["not an object", "x"],
  ])("does not show %s", (_name, expression) => {
    expect(conditionFromExpression(expression)).toBeNull();
  });
});

describe("filtersToWire", () => {
  it("writes the specified form of every operator kind", () => {
    const expressions = filtersToWire(
      [
        rule({ path: "A", operator: "equals", value: "v", ignoreCase: true }),
        rule({ path: "B", operator: "in", value: " a , b,, c " }),
        rule({ path: "C", operator: "not_exists", value: "ignored" }),
        rule({ path: "D", operator: "lt", value: "2" }),
      ],
      undefined,
    );
    expect(expressions).toEqual([
      { path: "A", op: "equals", value: "v", ignore_case: true },
      { path: "B", op: "in", value: ["a", "b", "c"] },
      { path: "C", op: "not_exists" },
      { path: "D", op: "lt", value: "2" },
    ]);
  });

  it("keeps unchanged expressions exactly, spelled-out defaults included", () => {
    const original = [{ path: "A", op: "equals", value: "x", ignore_case: false }];
    expect(filtersToWire(filtersFromWire(original), original)).toEqual(original);
  });

  it("matches originals by editor id, so a reordered list keeps what it did not change", () => {
    const original = [
      { path: "A", op: "equals", value: "1", ignore_case: false },
      { path: "B", op: "equals", value: "2", ignore_case: false },
    ];
    const [first, second] = filtersFromWire(original);
    if (!first || !second) throw new Error("fixture");
    expect(filtersToWire([second, first], original)).toEqual([original[1], original[0]]);
    expect(filtersToWire([second, { ...first, value: "9" }], original)).toEqual([
      original[1],
      { path: "A", op: "equals", value: "9" },
    ]);
  });

  it("keeps expressions the console cannot show, at the same place among the others", () => {
    const complex1 = { path: "X", op: "near", value: "x" };
    const complex2 = { path: "Y", op: "near" };
    const original = [
      complex1,
      { path: "A", op: "equals", value: "1" },
      { path: "B", op: "equals", value: "2" },
      complex2,
      { path: "C", op: "equals", value: "3" },
    ];
    const shown = filtersFromWire(original);
    expect(shown.map((f) => ("path" in f ? f.path : null))).toEqual(["A", "B", "C"]);

    expect(filtersToWire(shown, original)).toEqual(original);

    const [first, , third] = shown;
    if (!first || !third) throw new Error("fixture");
    const edited = filtersToWire([first, rule({ path: "N", operator: "exists" }), third], original);
    expect(edited).toEqual([
      complex1,
      { path: "A", op: "equals", value: "1" },
      { path: "N", op: "exists" },
      complex2,
      { path: "C", op: "equals", value: "3" },
    ]);

    expect(filtersToWire([], original)).toEqual([complex1, complex2]);
    expect(filtersToWire([first], original)).toEqual([complex1, original[1], complex2]);
  });

  it("does not hand out the original objects", () => {
    const original = [{ path: "A", op: "equals", value: "x" }];
    const [written] = filtersToWire(filtersFromWire(original), original);
    expect(written).toEqual(original[0]);
    expect(written).not.toBe(original[0]);
  });
});

describe("visibleFilterIndex", () => {
  const expressions = [
    { path: "Z", op: "near" },
    { path: "A", op: "exists" },
    { path: "B", op: "near" },
    { path: "C", op: "exists" },
  ];

  it("counts only the filters the console shows", () => {
    expect(visibleFilterIndex(expressions, 1)).toBe(0);
    expect(visibleFilterIndex(expressions, 3)).toBe(1);
  });

  it("has none for a hidden or missing filter", () => {
    expect(visibleFilterIndex(expressions, 0)).toBeNull();
    expect(visibleFilterIndex(expressions, 2)).toBeNull();
    expect(visibleFilterIndex(expressions, 9)).toBeNull();
    expect(visibleFilterIndex(undefined, 0)).toBeNull();
  });
});

describe("sameFilters", () => {
  it("ignores editor ids", () => {
    const a = [rule({ id: "1", path: "A", operator: "exists" })];
    const b = [rule({ id: "2", path: "A", operator: "exists" })];
    expect(sameFilters(a, b)).toBe(true);
    expect(sameFilters(a, [rule({ id: "1", path: "B", operator: "exists" })])).toBe(false);
  });
});
