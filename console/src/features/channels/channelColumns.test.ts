import { describe, expect, it } from "vitest";
import { channelColumnsFor } from "./channelColumns";

const headers = (width: number | null) => channelColumnsFor(width).map((column) => column.header);

describe("channelColumnsFor", () => {
  it("shows every column when the width is not measured, or ample", () => {
    expect(headers(null)).toEqual([
      "Channel",
      "Kind",
      "State",
      "Endpoints",
      "Messages in / out",
      "Queue / buffer",
      "Last activity",
      "Last error",
    ]);
    expect(headers(1904)).toEqual(headers(null));
  });

  it("fits 1024 pixels of content: everything but Last activity and Last error", () => {
    expect(headers(1024)).toEqual([
      "Channel",
      "Kind",
      "State",
      "Endpoints",
      "Messages in / out",
      "Queue / buffer",
    ]);
  });

  it("drops Last error, Last activity, Kind, Endpoints, Messages and Queue in that order, keeping Channel and State", () => {
    expect(headers(768)).toEqual(["Channel", "State", "Messages in / out", "Queue / buffer"]);
    expect(headers(1)).toEqual(["Channel", "State"]);
  });
});
