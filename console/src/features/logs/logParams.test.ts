import { describe, expect, it } from "vitest";
import {
  canFollow,
  DEFAULT_FILTERS,
  isFiltered,
  parseLogFilters,
  toLogQuery,
  toLogSearchParams,
} from "./logParams";

const parse = (search: string) => parseLogFilters(new URLSearchParams(search));

describe("the Logs URL", () => {
  it("defaults to Info and above, live", () => {
    expect(parse("")).toEqual(DEFAULT_FILTERS);
    expect(toLogSearchParams(DEFAULT_FILTERS).toString()).toBe("");
    expect(isFiltered(DEFAULT_FILTERS)).toBe(false);
  });

  it("reads the links the channel and endpoint pages make", () => {
    expect(parse("?channel=ch-1")).toEqual({ ...DEFAULT_FILTERS, channel: "ch-1" });
    expect(parse("?endpoint=ep-1")).toEqual({ ...DEFAULT_FILTERS, endpoint: "ep-1" });
    expect(parse("?event=channel.started")).toEqual({
      ...DEFAULT_FILTERS,
      event: "channel.started",
    });
    expect(isFiltered({ ...DEFAULT_FILTERS, event: "channel.started" })).toBe(true);
  });

  it("reads and writes every filter", () => {
    const filters = {
      level: "warn" as const,
      channel: "c",
      endpoint: "e",
      event: "delivery.state_changed",
      text: "no ack",
      range: "custom" as const,
      from: "2026-11-01T08:00:00Z",
      to: "2026-11-02T08:00:00Z",
    };
    const search = toLogSearchParams(filters).toString();
    expect(search).toBe(
      "level=warn&channel=c&endpoint=e&event=delivery.state_changed&text=no+ack&range=custom&from=2026-11-01T08%3A00%3A00Z&to=2026-11-02T08%3A00%3A00Z",
    );
    expect(parse(search)).toEqual(filters);
    expect(isFiltered(filters)).toBe(true);
  });

  it("ignores what it does not know, and times outside a custom range", () => {
    expect(parse("?level=loud&range=toString&from=2026-11-01T00:00:00Z")).toEqual(DEFAULT_FILTERS);
    expect(parse("?range=1h&from=2026-11-01T00:00:00Z").from).toBeUndefined();
  });
});

describe("the engine's query for the filters", () => {
  const NOW = Date.parse("2026-11-06T12:00:00Z");

  it("asks for everything from the level up when live", () => {
    expect(toLogQuery(DEFAULT_FILTERS, NOW)).toEqual({ minLevel: "info" });
  });

  it("counts a range back from now", () => {
    expect(toLogQuery({ ...DEFAULT_FILTERS, range: "15m" }, NOW).from).toBe(
      "2026-11-06T11:45:00.000Z",
    );
    expect(toLogQuery({ ...DEFAULT_FILTERS, range: "1h" }, NOW).from).toBe(
      "2026-11-06T11:00:00.000Z",
    );
    expect(toLogQuery({ ...DEFAULT_FILTERS, range: "24h" }, NOW).from).toBe(
      "2026-11-05T12:00:00.000Z",
    );
  });

  it("maps the other filters and a custom range onto the engine's names", () => {
    expect(
      toLogQuery(
        {
          level: "error",
          channel: "c",
          endpoint: "e",
          event: "operator.command",
          text: "x",
          range: "custom",
          from: "2026-11-01T00:00:00Z",
          to: "bogus",
        },
        NOW,
      ),
    ).toEqual({
      minLevel: "error",
      from: "2026-11-01T00:00:00Z",
      channelId: "c",
      endpointId: "e",
      event: "operator.command",
      text: "x",
    });
  });

  it("can follow unless the range ends in the past", () => {
    expect(canFollow(DEFAULT_FILTERS)).toBe(true);
    expect(canFollow({ ...DEFAULT_FILTERS, range: "1h" })).toBe(true);
    expect(canFollow({ ...DEFAULT_FILTERS, range: "custom", from: "2026-11-01T00:00:00Z" })).toBe(
      true,
    );
    expect(canFollow({ ...DEFAULT_FILTERS, range: "custom", to: "2026-11-01T00:00:00Z" })).toBe(
      false,
    );
  });
});
