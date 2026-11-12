import { describe, expect, it } from "vitest";
import { endpointColumnsFor } from "./endpointColumns";

const headers = (width: number | null) => endpointColumnsFor(width).map((column) => column.header);

describe("endpointColumnsFor", () => {
  it("shows every column when the width is not measured, or ample", () => {
    expect(headers(null)).toHaveLength(10);
    expect(headers(1904)).toEqual(headers(null));
  });

  it("keeps State, Bytes in, Bytes out and Last activity at 1024 pixels of content", () => {
    expect(headers(1024)).toEqual([
      "Endpoint",
      "Kind",
      "Summary",
      "State",
      "Bytes in",
      "Bytes out",
      "Last activity",
    ]);
  });

  it("keeps Endpoint and State at any width", () => {
    expect(headers(1)).toEqual(["Endpoint", "State"]);
  });
});
