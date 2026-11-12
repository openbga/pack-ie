import { describe, expect, it } from "vitest";
import { HEADER_ROW, nextPosition } from "./gridKeyboard";

const size = { rowCount: 50, colCount: 4 };
const keys = { ctrl: false, pageRows: 10 };

describe("nextPosition", () => {
  it("moves with arrows and stops at the edges", () => {
    expect(nextPosition("ArrowDown", { row: HEADER_ROW, col: 0 }, size, keys)).toEqual({
      row: 0,
      col: 0,
    });
    expect(nextPosition("ArrowUp", { row: HEADER_ROW, col: 0 }, size, keys)).toEqual({
      row: HEADER_ROW,
      col: 0,
    });
    expect(nextPosition("ArrowLeft", { row: 3, col: 0 }, size, keys)).toEqual({ row: 3, col: 0 });
    expect(nextPosition("ArrowRight", { row: 3, col: 3 }, size, keys)).toEqual({ row: 3, col: 3 });
  });

  it("pages by screen and clamps to the last row", () => {
    expect(nextPosition("PageDown", { row: 45, col: 1 }, size, keys)).toEqual({ row: 49, col: 1 });
    expect(nextPosition("PageUp", { row: 5, col: 1 }, size, keys)).toEqual({
      row: HEADER_ROW,
      col: 1,
    });
  });

  it("jumps within the row, or to the grid corners with Ctrl", () => {
    expect(nextPosition("End", { row: 7, col: 1 }, size, keys)).toEqual({ row: 7, col: 3 });
    expect(nextPosition("Home", { row: 7, col: 2 }, size, keys)).toEqual({ row: 7, col: 0 });
    const ctrl = { ...keys, ctrl: true };
    expect(nextPosition("End", { row: 7, col: 1 }, size, ctrl)).toEqual({ row: 49, col: 3 });
    expect(nextPosition("Home", { row: 7, col: 2 }, size, ctrl)).toEqual({
      row: HEADER_ROW,
      col: 0,
    });
  });

  it("ignores keys that do not navigate", () => {
    expect(nextPosition("a", { row: 0, col: 0 }, size, keys)).toBeNull();
  });

  it("stays on the header row of an empty grid", () => {
    const empty = { rowCount: 0, colCount: 2 };
    expect(nextPosition("ArrowDown", { row: HEADER_ROW, col: 0 }, empty, keys)).toEqual({
      row: HEADER_ROW,
      col: 0,
    });
  });
});
