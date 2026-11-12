import { describe, expect, it } from "vitest";
import { compareRows, rowsToText, textToRows } from "./tableText";

describe("code tables as text", () => {
  it("reads comma and tab separated lines, skipping blanks", () => {
    const { rows, problems } = textToRows("M,Male\r\n\nF\tFemale\n U , Unknown ");
    expect(problems).toEqual([]);
    expect(rows).toEqual([
      { code: "M", text: "Male" },
      { code: "F", text: "Female" },
      { code: "U", text: "Unknown" },
    ]);
  });

  it("keeps commas in the text and reads quoted values", () => {
    const { rows } = textToRows('GLU,Glucose, fasting\n"A,B","say ""hi"""\nX,"  padded  "\nE,');
    expect(rows).toEqual([
      { code: "GLU", text: "Glucose, fasting" },
      { code: "A,B", text: 'say "hi"' },
      { code: "X", text: "  padded  " },
      { code: "E", text: "" },
    ]);
  });

  it("names every line it cannot use", () => {
    const { rows, problems } = textToRows('M,Male\nnonsense\n,x\nM,Again\n"open,x');
    expect(rows).toEqual([{ code: "M", text: "Male" }]);
    expect(problems.map((p) => p.line)).toEqual([2, 3, 4, 5]);
    expect(problems[2]?.message).toBe("The code M is already on line 1.");
  });

  it("writes text that reads back the same", () => {
    const rows = [
      { code: "A,B", text: 'say "hi"' },
      { code: "G", text: "Glucose, fasting" },
      { code: "P", text: "  padded" },
      { code: "E", text: "" },
    ];
    expect(textToRows(rowsToText(rows)).rows).toEqual(rows);
  });

  it("counts what replacing rows does, by code", () => {
    const before = [
      { code: "M", text: "Male" },
      { code: " F ", text: "Female" },
      { code: "U", text: "Unknown" },
    ];
    const after = [
      { code: "M", text: "Male" },
      { code: "F", text: "Woman" },
      { code: "X", text: "Other" },
    ];
    expect(compareRows(before, after)).toEqual({
      added: 1,
      changed: 1,
      removed: 1,
      unchanged: 1,
    });
  });
});
