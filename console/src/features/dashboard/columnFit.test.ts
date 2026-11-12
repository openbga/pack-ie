import { describe, expect, it } from "vitest";
import { COLUMN_ORDER, DROP_ORDER, endpointsWidth, FIXED_WIDTH, visibleColumns } from "./columnFit";

describe("visibleColumns", () => {
  it("shows every column when the width is not measured or ample", () => {
    expect(visibleColumns(null)).toEqual(COLUMN_ORDER);
    expect(visibleColumns(5000)).toEqual(COLUMN_ORDER);
  });

  it("drops Retry, Last error, Session, Last activity, Traffic and Queue, in that order", () => {
    const widths = [1500, 1300, 1150, 1000, 840, 700, 100];
    const kept = widths.map((width) => visibleColumns(width));
    // Whatever the width, what remains is the list minus a prefix of the drop order.
    for (const columns of kept) {
      const dropped = COLUMN_ORDER.filter((id) => !columns.includes(id));
      expect(dropped).toEqual(
        DROP_ORDER.slice(0, dropped.length).sort(
          (a, b) => COLUMN_ORDER.indexOf(a) - COLUMN_ORDER.indexOf(b),
        ),
      );
    }
    // Each narrower window keeps no more than the wider one.
    for (let index = 1; index < kept.length; index += 1) {
      expect(kept[index]?.length).toBeLessThanOrEqual(kept[index - 1]?.length ?? 0);
    }
  });

  it("removes Retry first, then Last error, then Session", () => {
    const sizes = Array.from({ length: 1500 }, (_, index) => 1500 - index);
    const seen = new Set<string>();
    const droppedOrder: string[] = [];
    for (const width of sizes) {
      for (const id of COLUMN_ORDER) {
        if (!visibleColumns(width).includes(id) && !seen.has(id)) {
          seen.add(id);
          droppedOrder.push(id);
        }
      }
    }
    expect(droppedOrder).toEqual([...DROP_ORDER]);
  });

  it("never hides Channel, State or Endpoints", () => {
    expect(visibleColumns(1)).toEqual(["channel", "state", "endpoints"]);
  });

  it("keeps every shown column at its minimum width, Endpoints included, at any width", () => {
    for (let width = 670; width <= 2400; width += 7) {
      const shown = visibleColumns(width);
      const endpoints = endpointsWidth(width, shown);
      expect(endpoints).toBeGreaterThanOrEqual(280);
      expect(endpoints).toBeLessThanOrEqual(420);
      // Channel and Last error get at least their minimums from what Endpoints leaves.
      const fixedPx = (Object.keys(FIXED_WIDTH) as (keyof typeof FIXED_WIDTH)[])
        .filter((id) => shown.includes(id))
        .reduce((sum, id) => sum + FIXED_WIDTH[id], 0);
      const rest = width - endpoints - fixedPx;
      expect(rest).toBeGreaterThanOrEqual(200 + (shown.includes("error") ? 220 : 0));
    }
  });

  it("gives Endpoints its minimum when the width is not measured", () => {
    expect(endpointsWidth(null, COLUMN_ORDER)).toBe(280);
  });
});
