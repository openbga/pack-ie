import { describe, expect, it } from "vitest";
import { alertColumnsFor } from "./alertColumns";

const headers = (width: number | null) => alertColumnsFor(width).map((column) => column.header);

describe("alertColumnsFor", () => {
  it("puts Next step right after the summary and keeps every column at 1024 pixels of content", () => {
    expect(headers(1024)).toEqual([
      "Severity",
      "Alert",
      "Next step",
      "State",
      "Concerns",
      "First seen",
      "Last seen",
      "Seen",
    ]);
    expect(headers(null)).toEqual(headers(1024));
  });

  it("drops Last seen, First seen and Concerns first, never Severity, Alert, Next step, State or Seen", () => {
    expect(headers(800)).toEqual(["Severity", "Alert", "Next step", "State", "Concerns", "Seen"]);
    expect(headers(1)).toEqual(["Severity", "Alert", "Next step", "State", "Seen"]);
  });
});
