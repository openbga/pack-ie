import { describe, expect, it } from "vitest";
import {
  ALWAYS,
  COLUMN_WIDTH,
  DEFAULT_ORDER,
  DROP_ORDER,
  fittingColumns,
  GRID_CHROME,
} from "./columnFit";

const needed = (ids: readonly string[]) =>
  ids.reduce((sum, id) => sum + COLUMN_WIDTH[id as keyof typeof COLUMN_WIDTH], GRID_CHROME);

describe("the default columns that fit", () => {
  it("keeps every column when the width is not known, as in a test or before layout", () => {
    expect(fittingColumns(null)).toEqual(DEFAULT_ORDER);
  });

  it("keeps all nine on a wide grid, and drops from the right as it narrows", () => {
    expect(fittingColumns(1920)).toEqual(DEFAULT_ORDER);
    const kept = (width: number) => fittingColumns(width).length;
    let previous = DEFAULT_ORDER.length;
    for (let width = 1400; width >= 400; width -= 10) {
      const now = kept(width);
      expect(now).toBeLessThanOrEqual(previous);
      previous = now;
    }
  });

  it("drops Size, then Patient ID, then Message type, then Direction", () => {
    const all = needed(DEFAULT_ORDER);
    expect(fittingColumns(all - 1)).not.toContain("sizeBytes");
    expect(fittingColumns(all - 1)).toContain("patientId");
    const withoutSize = all - COLUMN_WIDTH.sizeBytes;
    expect(fittingColumns(withoutSize - 1)).not.toContain("patientId");
    expect(fittingColumns(withoutSize - 1)).toContain("messageType");
    const withoutPatient = withoutSize - COLUMN_WIDTH.patientId;
    expect(fittingColumns(withoutPatient - 1)).not.toContain("messageType");
    expect(fittingColumns(withoutPatient - 1)).toContain("direction");
    const withoutType = withoutPatient - COLUMN_WIDTH.messageType;
    expect(fittingColumns(withoutType - 1)).not.toContain("direction");
    expect(DROP_ORDER).toEqual(["sizeBytes", "patientId", "messageType", "direction"]);
  });

  it("never drops Received, Channel, Message state, Delivery summary or Identifier", () => {
    expect(ALWAYS).toEqual(["receivedAt", "channel", "state", "deliveries", "identifier"]);
    for (const width of [1, 300, 600, 800]) {
      expect(fittingColumns(width)).toEqual(ALWAYS);
    }
  });

  it("fits seven columns at the 1280 laptop with the side navigation open", () => {
    // 1280 wide, 256 for the navigation, 16 of padding each side.
    const frame = 1280 - 256 - 32;
    expect(fittingColumns(frame)).toEqual([
      "receivedAt",
      "channel",
      "direction",
      "state",
      "deliveries",
      "messageType",
      "identifier",
    ]);
  });

  it("keeps every column in display order", () => {
    for (const width of [400, 960, 1100, 1300]) {
      const kept = fittingColumns(width);
      expect(kept).toEqual(DEFAULT_ORDER.filter((id) => kept.includes(id)));
    }
  });
});
