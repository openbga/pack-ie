import { describe, expect, it } from "vitest";
import { type FitSpec, fitColumns } from "./fitColumns";

const specs: readonly FitSpec<"a" | "b" | "c" | "d">[] = [
  { id: "a", min: 100 },
  { id: "b", min: 100, drop: 3 },
  { id: "c", min: 100, drop: 1 },
  { id: "d", min: 100, drop: 2 },
];

describe("fitColumns", () => {
  it("keeps everything when the width is not measured, or ample", () => {
    expect(fitColumns(specs, null)).toEqual(["a", "b", "c", "d"]);
    expect(fitColumns(specs, 400)).toEqual(["a", "b", "c", "d"]);
  });

  it("drops in the given order, never a column kept by one that leaves later", () => {
    expect(fitColumns(specs, 399)).toEqual(["a", "b", "d"]);
    expect(fitColumns(specs, 299)).toEqual(["a", "b"]);
    expect(fitColumns(specs, 199)).toEqual(["a"]);
    expect(fitColumns(specs, 0)).toEqual(["a"]);
  });

  it("stops at the first column that does not fit even when a later one would", () => {
    const uneven: FitSpec<"a" | "b" | "c">[] = [
      { id: "a", min: 100 },
      { id: "b", min: 500, drop: 2 },
      { id: "c", min: 50, drop: 1 },
    ];
    expect(fitColumns(uneven, 300)).toEqual(["a"]);
  });
});
