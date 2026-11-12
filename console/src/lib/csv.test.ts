import { describe, expect, it } from "vitest";
import { csvCell, toCsv } from "./csv";

describe("csvCell", () => {
  it("leaves plain text and numbers as they are and empties null", () => {
    expect(csvCell("plain")).toBe("plain");
    expect(csvCell(12)).toBe("12");
    expect(csvCell(-3)).toBe("-3");
    expect(csvCell(false)).toBe("false");
    expect(csvCell(null)).toBe("");
    expect(csvCell(undefined)).toBe("");
  });

  it("quotes commas, quotes and line breaks", () => {
    expect(csvCell("a,b")).toBe('"a,b"');
    expect(csvCell('say "hi"')).toBe('"say ""hi"""');
    expect(csvCell("one\ntwo")).toBe('"one\ntwo"');
  });

  it("prefixes text a spreadsheet would run as a formula", () => {
    expect(csvCell("=SUM(A1)")).toBe("'=SUM(A1)");
    expect(csvCell("+1")).toBe("'+1");
    expect(csvCell("-1")).toBe("'-1");
    expect(csvCell("@cmd")).toBe("'@cmd");
    expect(csvCell("=a,b")).toBe('"\'=a,b"');
  });
});

describe("toCsv", () => {
  it("writes a header and rows, each ended by CRLF", () => {
    expect(
      toCsv(
        ["a", "b"],
        [
          [1, "x,y"],
          [null, "z"],
        ],
      ),
    ).toBe('a,b\r\n1,"x,y"\r\n,z\r\n');
  });
});
