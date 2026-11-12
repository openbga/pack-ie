import { describe, expect, it } from "vitest";
import type { LogEvent, LogFollowNotice } from "../types";
import type { MockEngineApi } from "./index";
import { mockApiFactory } from "./mockTestSupport";

const create = mockApiFactory({ messageCount: 200, seed: 7 });

describe("log events", () => {
  it("name the events of operator commands and configuration applies", async () => {
    const api = create();
    await api.pauseChannel("ch-msg-au680");
    const { events } = await api.queryLogs(
      { event: "operator.command" },
      { before: null, limit: 5 },
    );
    expect(events[0]?.message).toMatch(/by operator/);
  });
});

describe("log queries on the event", () => {
  it("match the whole name, as the engine does, never a prefix", async () => {
    const api = create({ logRingSize: 2000 });
    const exact = await api.queryLogs(
      { event: "delivery.state_changed" },
      { before: null, limit: 500 },
    );
    expect(exact.events.length).toBeGreaterThan(5);
    expect(exact.events.every((e) => e.event === "delivery.state_changed")).toBe(true);
    const prefix = await api.queryLogs({ event: "delivery." }, { before: null, limit: 500 });
    expect(prefix.events).toEqual([]);
    const other = await api.queryLogs({ event: "unknown.event" }, { before: null, limit: 5 });
    expect(other).toEqual({ events: [], olderCursor: null });
  });

  it("combine with the other filters and page back with the cursor", async () => {
    const api = create({ logRingSize: 2000 });
    const first = await api.queryLogs(
      { event: "delivery.state_changed", minLevel: "info" },
      { before: null, limit: 5 },
    );
    expect(first.events).toHaveLength(5);
    expect(first.olderCursor).toBe(first.events[4]?.seq);
    const second = await api.queryLogs(
      { event: "delivery.state_changed", minLevel: "info" },
      { before: first.olderCursor, limit: 5 },
    );
    expect(second.events.every((e) => e.seq < (first.olderCursor as number))).toBe(true);
  });

  it("treat the end of a time range as exclusive", async () => {
    const api = create();
    const all = (await api.queryLogs({}, { before: null, limit: 50 })).events;
    const at = all[10]?.at ?? "";
    const before = await api.queryLogs({ to: at }, { before: null, limit: 50 });
    expect(before.events.every((e) => e.at < at)).toBe(true);
  });
});

describe("an outage of the connection", () => {
  type Seen = { events: LogEvent[]; notice: LogFollowNotice | undefined };
  const follow = (api: MockEngineApi, query = {}) => {
    const seen: Seen[] = [];
    const stop = api.followLogs(query, (events, notice) => seen.push({ events, notice }));
    return { seen, stop };
  };

  it("reports reconnecting, and refuses requests until it ends", async () => {
    const api = create();
    const statuses: string[] = [];
    api.subscribeConnection((s) => statuses.push(s.state));
    api.simulateOutage();
    await expect(api.getSnapshot()).rejects.toMatchObject({ code: "engine_unavailable" });
    expect((await api.getConnection()).state).toBe("reconnecting");
    api.endOutage();
    expect((await api.getSnapshot()).node.nodeId).toBeTruthy();
    expect(statuses).toEqual(["connected", "reconnecting", "connected"]);
  });

  it("gives a follower what was logged meanwhile, once and oldest first, with a notice", async () => {
    const api = create();
    const { seen } = follow(api, { minLevel: "trace" });
    api.publishLogEvents(3);
    api.simulateOutage();
    // The three came before the connection was lost: delivered at once.
    expect(seen.flatMap((s) => s.events)).toHaveLength(3);
    api.publishLogEvents(12);
    expect(seen).toHaveLength(1);
    api.endOutage();
    expect(seen).toHaveLength(2);
    const { events, notice } = seen[1] as Seen;
    expect(events).toHaveLength(12);
    expect(events.map((e) => e.seq)).toEqual([...events.map((e) => e.seq)].sort((a, b) => a - b));
    expect(notice).toMatchObject({ filled: 12, missing: "none" });
    expect(Date.parse(notice?.backAt ?? "")).toBeGreaterThanOrEqual(
      Date.parse(notice?.lostAt ?? ""),
    );
    // Nothing is given twice later.
    api.publishLogEvents(1);
    api.simulateOutage();
    api.endOutage();
    expect(seen.at(-1)?.events).toEqual([]);
    expect(seen.at(-1)?.notice).toMatchObject({ filled: 0, missing: "none" });
  });

  it("applies the follower's query to what it fills in", async () => {
    const api = create();
    const { seen } = follow(api, { event: "node.started" });
    api.simulateOutage();
    api.publishLogEvents(30);
    api.endOutage({ restarted: true });
    const last = seen.at(-1);
    expect(last?.notice?.missing).toBe("restarted");
    expect(last?.events.map((e) => [e.seq, e.event])).toEqual([[1, "node.started"]]);
  });

  it("says the engine restarted, with its log starting again from seq 1", async () => {
    const api = create();
    const { seen } = follow(api, { minLevel: "trace" });
    api.simulateOutage();
    api.endOutage({ restarted: true });
    const [{ events, notice }] = seen as [Seen];
    expect(notice?.missing).toBe("restarted");
    expect(events[0]).toMatchObject({ seq: 1, event: "node.started" });
    const page = await api.queryLogs({}, { before: null, limit: 10 });
    expect(page.events.at(-1)?.seq).toBe(1);
  });

  it("says events were lost when the ring dropped some of the outage", () => {
    const api = create({ logRingSize: 100 });
    const { seen } = follow(api, { minLevel: "trace" });
    api.simulateOutage();
    api.publishLogEvents(150);
    api.endOutage();
    const [{ events, notice }] = seen as [Seen];
    expect(notice).toMatchObject({ missing: "gap", filled: 100 });
    expect(events).toHaveLength(100);
  });

  it("leaves a follower that began during the outage alone", () => {
    const api = create();
    api.simulateOutage();
    const { seen } = follow(api, { minLevel: "trace" });
    api.publishLogEvents(5);
    api.endOutage();
    expect(seen).toEqual([]);
    api.publishLogEvents(2);
  });
});
