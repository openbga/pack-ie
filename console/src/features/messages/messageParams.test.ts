import { describe, expect, it } from "vitest";
import {
  DEFAULT_FILTERS,
  DEFAULT_SORT,
  hasDefaultRange,
  isFiltered,
  isMessageIdPrefix,
  isNarrowed,
  isSearchableText,
  type MessageFilters,
  parseMessageView,
  quickCountQuery,
  resolveTime,
  toMessageFilter,
  toMessageQuery,
  toSearchParams,
} from "./messageParams";

const parse = (search: string) => parseMessageView(new URLSearchParams(search));
const NOW = Date.parse("2026-11-06T12:00:00Z");
const filters = (extra: Partial<MessageFilters> = {}): MessageFilters => ({
  ...DEFAULT_FILTERS,
  ...extra,
});

describe("the Messages list's URL", () => {
  it("starts with the last 24 hours and nothing else", () => {
    expect(parse("").filters).toEqual({ channels: [], from: "-24h" });
    expect(parse("").filters).toEqual(DEFAULT_FILTERS);
    expect(
      toSearchParams({ filters: DEFAULT_FILTERS, sort: DEFAULT_SORT, page: 1 }).toString(),
    ).toBe("");
  });

  it("reads the links the channel page makes", () => {
    expect(parse("?channel=ch-1&quick=attention").filters).toEqual({
      channels: ["ch-1"],
      from: "-24h",
      quick: "attention",
    });
  });

  it("reads every parameter of the view", () => {
    const view = parse(
      "?channel=a&channel=b&direction=a_to_b&state=invalid&delivery=failed&from=-7d&to=2026-11-02T00:00:00Z&sort=sizeBytes&desc=1&page=3",
    );
    expect(view).toEqual({
      filters: {
        channels: ["a", "b"],
        direction: "a_to_b",
        state: "invalid",
        delivery: "failed",
        from: "-7d",
        to: "2026-11-02T00:00:00Z",
      },
      sort: { field: "sizeBytes", descending: true },
      page: 3,
    });
  });

  it("reads any time as an explicit choice, and times that are named as they are", () => {
    expect(parse("?range=any").filters).toEqual({ channels: [] });
    expect(parse("?to=2026-11-02T00:00:00Z").filters).toEqual({
      channels: [],
      to: "2026-11-02T00:00:00Z",
    });
  });

  it.each(["?quick=attention&range=any", "?channel=ch-1&quick=attention&range=any"])(
    "reads the link %s of the channel page or the dashboard as the unresolved deliveries",
    (search) => {
      const { filters } = parse(search);
      expect(filters.quick).toBe("attention");
      expect(filters.from).toBeUndefined();
      expect(filters.channels).toEqual(search.includes("ch-1") ? ["ch-1"] : []);
    },
  );

  it("keeps the 24 hour range unless the link names another", () => {
    expect(parse("?quick=attention").filters.from).toBeDefined();
    expect(parse("?quick=attention&from=-7d").filters.from).toBe("-7d");
  });

  it("lets a quick filter stand in for the state choices", () => {
    expect(parse("?quick=duplicates&state=invalid&delivery=failed").filters).toEqual({
      channels: [],
      from: "-24h",
      quick: "duplicates",
    });
  });

  it("never reads the identifier or text filter from the address", () => {
    expect(parse("?id=A1&text=glu&identifier=A1&messageId=0a1b2c3d").filters).toEqual(
      DEFAULT_FILTERS,
    );
  });

  it("ignores values it does not know, never failing", () => {
    const view = parse(
      "?state=bogus&delivery=nope&quick=nothing&direction=sideways&sort=nope&desc=1&page=-4&channel=%20",
    );
    expect(view).toEqual({ filters: DEFAULT_FILTERS, sort: DEFAULT_SORT, page: 1 });
    expect(parse("?page=abc").page).toBe(1);
    expect(parse("?sort=controlId").sort).toEqual(DEFAULT_SORT);
  });

  it("sorts ascending when only the field is given", () => {
    expect(parse("?sort=channel").sort).toEqual({ field: "channel", descending: false });
  });

  it("writes only what differs from the defaults, and reads back what it wrote", () => {
    const view = {
      filters: filters({ channels: ["a", "b"], state: "duplicate", from: "-7d" }),
      sort: { field: "state" as const, descending: false },
      page: 2,
    };
    const search = toSearchParams(view).toString();
    expect(search).toBe("channel=a&channel=b&state=duplicate&from=-7d&sort=state&page=2");
    expect(parse(search)).toEqual(view);
    const open = { filters: { channels: [] }, sort: DEFAULT_SORT, page: 1 };
    expect(toSearchParams(open).toString()).toBe("range=any");
    expect(parse("?range=any")).toEqual(open);
  });

  it("does not write the identifier, the match mode, the text or the message ID", () => {
    const search = toSearchParams({
      filters: filters({ id: "PID-1", idExact: true, text: "glucose", messageId: "0a1b2c3d" }),
      sort: DEFAULT_SORT,
      page: 1,
    }).toString();
    expect(search).toBe("");
  });
});

describe("what counts as filtering", () => {
  it("is not filtering by default, and is by anything else", () => {
    expect(isFiltered(DEFAULT_FILTERS)).toBe(false);
    expect(isNarrowed(DEFAULT_FILTERS)).toBe(false);
    expect(hasDefaultRange(DEFAULT_FILTERS)).toBe(true);
    for (const next of [
      filters({ channels: ["a"] }),
      filters({ direction: "inbound" }),
      filters({ state: "invalid" }),
      filters({ delivery: "failed" }),
      filters({ quick: "queued" }),
      filters({ id: "A" }),
      filters({ text: "glucose" }),
      filters({ messageId: "0a1b2c3d" }),
    ]) {
      expect(isNarrowed(next)).toBe(true);
      expect(isFiltered(next)).toBe(true);
    }
  });

  it("treats another time range as filtering that does not narrow the other filters", () => {
    const wide = { channels: [] };
    expect(isNarrowed(wide)).toBe(false);
    expect(isFiltered(wide)).toBe(true);
    expect(isFiltered(filters({ from: "-7d" }))).toBe(true);
  });

  it("searches text only from 3 characters", () => {
    expect(isSearchableText(undefined)).toBe(false);
    expect(isSearchableText("ab")).toBe(false);
    expect(isSearchableText(" ab ")).toBe(false);
    expect(isSearchableText("abc")).toBe(true);
  });
});

describe("a message ID prefix", () => {
  it("is 8 to 32 hex digits, dashes optional and any case", () => {
    expect(isMessageIdPrefix(undefined)).toBe(false);
    expect(isMessageIdPrefix("0a1b2c3")).toBe(false);
    expect(isMessageIdPrefix("0a1b2c3d")).toBe(true);
    expect(isMessageIdPrefix("0A1B2C3D-0000")).toBe(true);
    expect(isMessageIdPrefix("0a1b2c3d-0000-4000-8000-000000000001")).toBe(true);
    expect(isMessageIdPrefix("f".repeat(32))).toBe(true);
    expect(isMessageIdPrefix("f".repeat(33))).toBe(false);
    expect(isMessageIdPrefix("0a1b2c3g")).toBe(false);
    expect(isMessageIdPrefix("--------")).toBe(false);
  });
});

describe("times", () => {
  it("resolves relative times when it reads, and reads a time as it is written", () => {
    expect(resolveTime("-1h", NOW)).toBe("2026-11-06T11:00:00.000Z");
    expect(resolveTime("-24h", NOW)).toBe("2026-11-05T12:00:00.000Z");
    expect(resolveTime("-30d", NOW)).toBe("2026-10-07T12:00:00.000Z");
    expect(resolveTime("2026-11-01T00:00:00Z", NOW)).toBe("2026-11-01T00:00:00.000Z");
    expect(resolveTime("tomorrow", NOW)).toBeUndefined();
    expect(resolveTime(undefined, NOW)).toBeUndefined();
  });
});

describe("the engine's query", () => {
  it("asks for the last 24 hours, newest first, one page", () => {
    expect(
      toMessageQuery({ filters: DEFAULT_FILTERS, sort: DEFAULT_SORT, page: 1 }, 100, NOW),
    ).toEqual({
      filter: { since: "2026-11-05T12:00:00.000Z" },
      sort: { field: "receivedAt", descending: true },
      offset: 0,
      limit: 100,
    });
    expect(
      toMessageQuery({ filters: DEFAULT_FILTERS, sort: DEFAULT_SORT, page: 3 }, 50, NOW),
    ).toMatchObject({ offset: 100, limit: 50 });
  });

  it("sends every filter", () => {
    expect(
      toMessageFilter(
        {
          channels: ["a", "b"],
          direction: "b_to_a",
          state: "invalid",
          delivery: "queued",
          from: "-1h",
          to: "2026-11-06T11:30:00Z",
          id: " S-1 ",
          idExact: true,
          text: " glucose ",
        },
        NOW,
      ),
    ).toEqual({
      since: "2026-11-06T11:00:00.000Z",
      until: "2026-11-06T11:30:00.000Z",
      channelIds: ["a", "b"],
      directions: ["b_to_a"],
      states: ["invalid"],
      deliveryStates: ["queued"],
      identifier: { value: "S-1", prefix: false },
      text: "glucose",
    });
  });

  it("matches the start of an identifier unless it is asked to match exactly", () => {
    expect(toMessageFilter(filters({ id: "S-1" }), NOW).identifier).toEqual({
      value: "S-1",
      prefix: true,
    });
  });

  it("sends the start of a message ID, trimmed, once it is one the engine accepts", () => {
    expect(toMessageFilter(filters({ messageId: " 0A1B-2C3D " }), NOW).messageIdPrefix).toBe(
      "0A1B-2C3D",
    );
    for (const messageId of ["", "0a1b2c3", "xyz12345", "0".repeat(33), "msg-0000007"]) {
      expect(toMessageFilter(filters({ messageId }), NOW).messageIdPrefix).toBeUndefined();
    }
  });

  it("leaves a text of fewer than 3 characters out, since the engine would refuse it", () => {
    expect(toMessageFilter(filters({ text: "ab" }), NOW).text).toBeUndefined();
    expect(toMessageFilter(filters({ id: "  " }), NOW).identifier).toBeUndefined();
  });

  it.each([
    ["attention", { deliverySummaries: ["attention"] }],
    ["queued", { deliveryStates: ["queued"] }],
    ["duplicates", { states: ["duplicate"] }],
    ["invalid_aborted", { states: ["invalid", "aborted"] }],
  ] as const)("asks for the %s quick filter's criteria", (quick, criteria) => {
    expect(toMessageFilter(filters({ quick }), NOW)).toEqual({
      since: "2026-11-05T12:00:00.000Z",
      ...criteria,
    });
  });

  it("counts a quick filter within the other filters, with one row asked for", () => {
    expect(
      quickCountQuery(
        filters({ channels: ["a"], state: "routed", text: "glucose" }),
        "attention",
        NOW,
      ),
    ).toEqual({
      filter: {
        since: "2026-11-05T12:00:00.000Z",
        channelIds: ["a"],
        text: "glucose",
        deliverySummaries: ["attention"],
      },
      sort: DEFAULT_SORT,
      offset: 0,
      limit: 1,
    });
  });
});
