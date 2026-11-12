import { describe, expect, it } from "vitest";
import {
  parseTransmissionView,
  periodProblem,
  toQuery,
  toTransmissionParams,
} from "./transmissionParams";

const parse = (query: string) => parseTransmissionView(new URLSearchParams(query));

describe("parseTransmissionView", () => {
  it("defaults to the last 24 hours with nothing chosen", () => {
    expect(parse("")).toEqual({ range: "24h" });
  });

  it("reads the channel, a preset and the destination", () => {
    expect(parse("channel=ch-1&range=7d&destination=dest-1")).toEqual({
      channel: "ch-1",
      range: "7d",
      destination: "dest-1",
    });
  });

  it("treats since or until without a range as a custom period", () => {
    expect(parse("channel=ch-1&since=2026-11-01T00:00:00Z&until=2026-11-02T00:00:00Z")).toEqual({
      channel: "ch-1",
      range: "custom",
      since: "2026-11-01T00:00:00Z",
      until: "2026-11-02T00:00:00Z",
    });
  });

  it("ignores times of a preset, and anything it does not know", () => {
    expect(parse("range=30d&since=2026-11-01T00:00:00Z")).toEqual({ range: "30d" });
    expect(parse("range=forever&since=yesterday&channel=%20")).toEqual({ range: "24h" });
  });
});

describe("toTransmissionParams", () => {
  it("keeps only what differs from the defaults, and round-trips", () => {
    expect(toTransmissionParams({ range: "24h" })).toEqual({});
    const view = {
      channel: "ch-1",
      range: "custom" as const,
      since: "2026-11-01T00:00:00Z",
      until: "2026-11-02T00:00:00Z",
      destination: "dest-1",
    };
    expect(toTransmissionParams(view)).toEqual({
      channel: "ch-1",
      range: "custom",
      since: "2026-11-01T00:00:00Z",
      until: "2026-11-02T00:00:00Z",
      destination: "dest-1",
    });
    expect(parse(new URLSearchParams(toTransmissionParams(view)).toString())).toEqual(view);
  });
});

describe("periodProblem and toQuery", () => {
  const NOW = Date.parse("2026-11-07T12:00:00Z");

  it("needs a channel, and for a custom period a start before its end", () => {
    expect(toQuery({ range: "24h" }, NOW)).toBeNull();
    expect(periodProblem({ range: "custom" })).toBe("Choose when the period starts.");
    expect(
      periodProblem({
        range: "custom",
        since: "2026-11-02T00:00:00Z",
        until: "2026-11-01T00:00:00Z",
      }),
    ).toBe("The period must end after it starts.");
    expect(periodProblem({ range: "custom", since: "2026-11-02T00:00:00Z" })).toBeNull();
    expect(toQuery({ channel: "ch-1", range: "custom" }, NOW)).toBeNull();
  });

  it("counts a preset back from now and leaves the end to the engine", () => {
    expect(toQuery({ channel: "ch-1", range: "24h" }, NOW)).toEqual({
      channelId: "ch-1",
      since: "2026-11-06T12:00:00.000Z",
    });
    expect(toQuery({ channel: "ch-1", range: "7d" }, NOW)?.since).toBe("2026-10-31T12:00:00.000Z");
    expect(toQuery({ channel: "ch-1", range: "30d" }, NOW)?.since).toBe("2026-10-08T12:00:00.000Z");
  });

  it("passes a custom period as chosen", () => {
    expect(
      toQuery(
        {
          channel: "ch-1",
          range: "custom",
          since: "2026-11-01T00:00:00Z",
          until: "2026-11-02T00:00:00Z",
        },
        NOW,
      ),
    ).toEqual({
      channelId: "ch-1",
      since: "2026-11-01T00:00:00Z",
      until: "2026-11-02T00:00:00Z",
    });
  });
});
