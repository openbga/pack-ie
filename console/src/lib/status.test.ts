import { describe, expect, it } from "vitest";
import { channelUrgency } from "./status";

describe("channel urgency", () => {
  it("does not alarm for a stopped channel, whatever the reason; the engine's alerts do that", () => {
    for (const stopReason of [
      "paused",
      "disabled",
      "configuration",
      "fatal",
      "not_implemented",
    ] as const) {
      expect(channelUrgency({ state: "stopped", stopReason })).toBe(1);
    }
    expect(channelUrgency({ state: "degraded", stopReason: null })).toBe(3);
  });
});
