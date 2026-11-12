import { describe, expect, it } from "vitest";
import { filterWords } from "./filterWords";
import { DEFAULT_FILTERS } from "./messageParams";

const label = (id: string) => `Channel ${id}`;

describe("filterWords", () => {
  it("always names the time range", () => {
    expect(filterWords(DEFAULT_FILTERS, label)).toEqual(["received in the last 24 hours"]);
    expect(filterWords({ channels: [] }, label)).toEqual(["received at any time"]);
    expect(filterWords({ channels: [], from: "2026-06-01T00:00:00Z" }, label)).toEqual([
      "received in a custom time range",
    ]);
  });

  it("lists every criterion that is set, as the filter bar shows them", () => {
    expect(
      filterWords(
        {
          channels: ["a", "b"],
          from: "-7d",
          direction: "inbound",
          state: "routed",
          delivery: "failed",
          id: "PT-0001",
          text: "ORU",
          messageId: "0123abcd",
        },
        label,
      ),
    ).toEqual([
      "received in the last 7 days",
      "identifier starts with PT-0001",
      "text contains ORU",
      "channel Channel a or Channel b",
      "direction Inbound",
      "message state Routed",
      "delivery state Failed, not delivered",
      "message ID starts with 0123abcd",
    ]);
  });

  it("says exactly for an exact identifier, and names a quick filter instead of the states", () => {
    expect(
      filterWords(
        { channels: [], from: "-1h", id: "X1", idExact: true, quick: "attention" },
        label,
      ),
    ).toEqual(["received in the last hour", "identifier is X1", "quick filter Needs attention"]);
  });

  it("leaves out a text or message ID too short to search, as the engine is not asked for them", () => {
    expect(
      filterWords({ channels: [], from: "-24h", text: "ab", messageId: "abc" }, label),
    ).toEqual(["received in the last 24 hours"]);
  });
});
