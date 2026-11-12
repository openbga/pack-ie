import { describe, expect, it } from "vitest";
import {
  DEFAULT_VIEW,
  describeView,
  isFiltered,
  parseAuditView,
  toAuditFilter,
  toAuditParams,
} from "./auditParams";

const parse = (search: string) => parseAuditView(new URLSearchParams(search));

describe("audit filters in the address", () => {
  it("reads the defaults from an empty address", () => {
    expect(parse("")).toEqual(DEFAULT_VIEW);
    expect(isFiltered(parse(""))).toBe(false);
  });

  it("round-trips every filter", () => {
    const view = {
      range: "custom" as const,
      from: "2026-11-01T00:00:00.000Z",
      to: "2026-11-02T00:00:00.000Z",
      actor: "LAB\\jdoe",
      action: "config_apply" as const,
      objectType: "channel" as const,
      objectId: "ch-1",
      outcome: "rejected" as const,
    };
    expect(parseAuditView(new URLSearchParams(toAuditParams(view)))).toEqual(view);
  });

  it("ignores values it does not know, so a stale address never filters by something unknown", () => {
    expect(parse("range=forever&action=nope&type=x&outcome=maybe&from=2026-01-01")).toEqual(
      DEFAULT_VIEW,
    );
    expect(parse("action=toString&type=constructor")).toEqual(DEFAULT_VIEW);
  });

  it("keeps From and To only for a custom range", () => {
    expect(parse("range=24h&from=2026-01-01T00:00:00Z")).toEqual({ range: "24h" });
    expect(toAuditParams({ range: "24h", from: "2026-01-01T00:00:00Z" })).toEqual({
      range: "24h",
    });
  });

  it("counts presets back from now and leaves All time open", () => {
    const now = Date.UTC(2026, 10, 7, 12);
    expect(toAuditFilter({ range: "1h" }, now).since).toBe("2026-11-07T11:00:00.000Z");
    expect(toAuditFilter({ range: "7d" }, now).since).toBe("2026-10-31T12:00:00.000Z");
    expect(toAuditFilter({ range: "30d" }, now).since).toBe("2026-10-08T12:00:00.000Z");
    expect(toAuditFilter({ range: "all" }, now)).toEqual({});
  });

  it("drops a custom time that is not a time", () => {
    expect(
      toAuditFilter({ range: "custom", from: "yesterday", to: "2026-11-02T00:00:00Z" }, 0),
    ).toEqual({ until: "2026-11-02T00:00:00Z" });
  });

  it("describes the filters in words", () => {
    expect(describeView(DEFAULT_VIEW)).toEqual([]);
    expect(
      describeView({ range: "24h", actor: "jdoe", objectType: "message", objectId: "msg-1" }),
    ).toEqual(["last 24 hours", "actor jdoe", "object type Message", "object ID msg-1"]);
  });
});
