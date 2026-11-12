import { describe, expect, it } from "vitest";
import type { Formula } from "@/api/types";
import { formulaPaths, parseFormula, printFormula, splitOperands } from "./formulaText";

const parse = (text: string): Formula => {
  const parsed = parseFormula(text);
  if (!("formula" in parsed)) throw new Error(parsed.error);
  return parsed.formula;
};
const from = (path: string): Formula => ({ from: path, steps: [] });
const constant = (value: string): Formula => ({ constant: value });

describe("formula text", () => {
  it("reads operators with their usual strength", () => {
    expect(parse("$.4 * 0.0555")).toEqual({ op: "mul", args: [from("$.4"), constant("0.0555")] });
    expect(parse("a + b * c")).toEqual({
      op: "add",
      args: [from("a"), { op: "mul", args: [from("b"), from("c")] }],
    });
  });

  it("joins a chain into one operator and keeps brackets as nesting", () => {
    expect(parse("a + b + c")).toEqual({ op: "add", args: [from("a"), from("b"), from("c")] });
    expect(parse("(a + b) + c")).toEqual({
      op: "add",
      args: [{ op: "add", args: [from("a"), from("b")] }, from("c")],
    });
    expect(parse("a - b - c")).toEqual({ op: "sub", args: [from("a"), from("b"), from("c")] });
    expect(parse("a + b - c")).toEqual({
      op: "sub",
      args: [{ op: "add", args: [from("a"), from("b")] }, from("c")],
    });
    expect(parse("a / b / c")).toEqual({ op: "div", args: [from("a"), from("b"), from("c")] });
  });

  it("reads functions, signed numbers and every kind of path", () => {
    expect(parse("max(OBX[1].5, 0)")).toEqual({
      op: "max",
      args: [from("OBX[1].5"), constant("0")],
    });
    expect(parse("abs(R[2].3.4)")).toEqual({ op: "abs", args: [from("R[2].3.4")] });
    expect(parse("min(a, b, c)")).toEqual({ op: "min", args: [from("a"), from("b"), from("c")] });
    expect(parse("a * -2")).toEqual({ op: "mul", args: [from("a"), constant("-2")] });
    expect(parse("a -2")).toEqual({ op: "sub", args: [from("a"), constant("2")] });
    expect(parse(" .5 + 7. ")).toEqual({ op: "add", args: [constant(".5"), constant("7.")] });
    expect(parse("col.result")).toEqual(from("col.result"));
  });

  it.each([
    ["", "Enter a formula"],
    ["a +", "Expected a number, a path or a bracket at the end"],
    ["a b", 'Expected an operator or the end of the formula at character 3, found "b"'],
    ["(a + b", "Expected a closing bracket at the end"],
    ["a % 2", 'Unexpected "%" at character 3'],
    ["5mmol", "A number cannot run into letters"],
    ["-a", "A minus sign can only start a number"],
    ["abs(a, b)", "abs takes one value"],
    ["min(a)", "min takes at least two values"],
    ["a * )", 'Expected a number, a path or a bracket at character 5, found ")"'],
  ])("explains %j", (text, message) => {
    const parsed = parseFormula(text);
    expect("error" in parsed && parsed.error).toContain(message);
  });

  it.each([
    "$.4 * 0.0555",
    "(OBX[1].5 + OBX[2].5) / 2",
    "a + b * c",
    "(a + b) * c",
    "a + b - c",
    "a - (b - c)",
    "(a + b) + c",
    "a + (b + c)",
    "a / (b * c)",
    "a * b / c",
    "a * (b / c)",
    "max($.4, 0) - abs($.5)",
    "min(a + b, c * 2, 7)",
    "a - -2",
  ])("prints %j so that it reads back as the same tree", (text) => {
    const formula = parse(text);
    expect(printFormula(formula)).toBe(text);
    expect(parse(printFormula(formula))).toEqual(formula);
  });

  it("shows signs as a sentence would, and lists the paths read", () => {
    const formula = parse("($.4 - $.5) / 2 * $.4");
    expect(printFormula(formula, { pretty: true })).toBe("($.4 − $.5) ÷ 2 × $.4");
    expect(formulaPaths(formula)).toEqual(["$.4", "$.5"]);
  });

  describe("named operands", () => {
    const find = (code: string): Formula => ({
      find: "OBX",
      where: { path: "$.3.1", operator: "equals", value: code, ignoreCase: false },
      from: "$.5",
      steps: [],
    });
    const anionGap: Formula = {
      op: "sub",
      args: [find("NA"), { op: "add", args: [find("CL"), find("HCO3")] }],
    };

    it("names an operand that needs more than a path, after the code it looks for", () => {
      const { text, operands } = splitOperands(anionGap);
      expect(text).toBe("@na - (@cl + @hco3)");
      expect(operands.map((each) => each.name)).toEqual(["na", "cl", "hco3"]);
    });

    it("gives the same operand one name and different ones their own", () => {
      const trimmed: Formula = { from: "A", steps: [{ step: "trim" }] };
      const other: Formula = { from: "A", steps: [{ step: "upper" }] };
      const { text, operands } = splitOperands({
        op: "add",
        args: [trimmed, trimmed, other, find("X Y")],
      });
      expect(text).toBe("@value + @value + @value2 + @match");
      expect(operands).toHaveLength(3);
    });

    it("reads the names back into the operands they stand for", () => {
      const { text, operands } = splitOperands(anionGap);
      const named = Object.fromEntries(operands.map((each) => [each.name, each.operand]));
      expect(parseFormula(text, named)).toEqual({ formula: anionGap });
    });

    it("says which operand is missing", () => {
      const parsed = parseFormula("@na - 1", {});
      expect("error" in parsed && parsed.error).toContain(
        "No operand is named @na, at character 1",
      );
    });

    it("leaves plain paths and constants in the text", () => {
      expect(splitOperands(parse("$.4 * 0.0555")).operands).toEqual([]);
      expect(formulaPaths(anionGap)).toEqual([]);
    });
  });
});
