import { describe, expect, it } from "vitest";
import { fittingColumns } from "./columnFit";

describe("fittingColumns", () => {
  it("keeps every default column where the width is not measured or is wide", () => {
    const all = ["time", "actor", "action", "object", "outcome"];
    expect(fittingColumns(null)).toEqual(all);
    expect(fittingColumns(960)).toEqual(all);
  });

  it("never offers Origin among the defaults", () => {
    expect(fittingColumns(null)).not.toContain("origin");
  });

  it("leaves out Object first, then Actor, and never Time, Action or Outcome", () => {
    expect(fittingColumns(560)).toEqual(["time", "actor", "action", "outcome"]);
    expect(fittingColumns(400)).toEqual(["time", "action", "outcome"]);
    expect(fittingColumns(100)).toEqual(["time", "action", "outcome"]);
  });
});
