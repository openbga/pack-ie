import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { LogFilter } from "@/ipc/generated";
import { IpcClient } from "./client";
import { followLogs, LogPager, logEventFromIpc } from "./logs";
import { FakeBridge, settle } from "./test/fakeBridge";
import { event } from "./test/logEvents";

function ringBridge(n: number) {
  const bridge = new FakeBridge();
  const ring = Array.from({ length: n }, (_, i) => event(i + 1));
  const filters: LogFilter[] = [];
  bridge.request("get_logs", (request) => {
    if (request.type !== "get_logs") throw new Error("unexpected");
    const { filter } = request;
    filters.push(filter);
    const before = filter.before_seq ?? Number.POSITIVE_INFINITY;
    const matching = ring.filter((e) => e.seq < before);
    return { type: "logs", events: matching.slice(-(filter.limit ?? 500)) };
  });
  return { bridge, filters, ring };
}

describe("log events", () => {
  it("maps the fields the console shows", () => {
    expect(
      logEventFromIpc(
        event(3, {
          level: "warn",
          channel_id: "c",
          endpoint_id: "e",
          message_id: "m",
          event: "delivery.state_changed",
          fields: { a: "b" },
        }),
      ),
    ).toEqual({
      seq: 3,
      at: "2026-11-06T12:00:03.000Z",
      level: "warn",
      target: "packie::test",
      event: "delivery.state_changed",
      channelId: "c",
      endpointId: "e",
      messageId: "m",
      message: "event 3",
      fields: { a: "b" },
    });
  });
});

describe("queryLogs", () => {
  it("asks with the query as a filter and serves newest first", async () => {
    const { bridge, filters } = ringBridge(30);
    const pager = new LogPager(new IpcClient(bridge));
    const page = await pager.query(
      {
        minLevel: "warn",
        from: "2026-11-06T00:00:00Z",
        to: "2026-11-07T00:00:00Z",
        channelId: "c",
        endpointId: "e",
        event: "channel.started",
        text: "x",
      },
      { before: null, limit: 10 },
    );
    expect(filters[0]).toEqual({
      min_level: "warn",
      since: "2026-11-06T00:00:00Z",
      until: "2026-11-07T00:00:00Z",
      channel_id: "c",
      endpoint_id: "e",
      text: "x",
      event: "channel.started",
      after_seq: null,
      before_seq: null,
      limit: 10,
    });
    expect(page.events.map((e) => e.seq)).toEqual([30, 29, 28, 27, 26, 25, 24, 23, 22, 21]);
    expect(page.olderCursor).toBe(21);
  });

  it("pages back with `before_seq` from the cursor of the page before", async () => {
    const { bridge, filters } = ringBridge(45);
    const pager = new LogPager(new IpcClient(bridge));
    const first = await pager.query({}, { before: null, limit: 20 });
    const second = await pager.query({}, { before: first.olderCursor, limit: 20 });
    const third = await pager.query({}, { before: second.olderCursor, limit: 20 });
    expect(first.events.map((e) => e.seq)).toEqual(Array.from({ length: 20 }, (_, i) => 45 - i));
    expect(second.events.map((e) => e.seq)).toEqual(Array.from({ length: 20 }, (_, i) => 25 - i));
    expect(third.events.map((e) => e.seq)).toEqual([5, 4, 3, 2, 1]);
    expect(first.olderCursor).toBe(26);
    expect(second.olderCursor).toBe(6);
    expect(third.olderCursor).toBeNull();
    // One request per page, asking for exactly the page: no over-fetching, no time bound.
    expect(filters).toHaveLength(3);
    expect(filters[1]).toMatchObject({ limit: 20, before_seq: 26, until: null, event: null });
    expect(filters[2]).toMatchObject({ limit: 20, before_seq: 6, until: null });
  });

  it("ends with an empty page when the last full page held the oldest event", async () => {
    const { bridge } = ringBridge(20);
    const pager = new LogPager(new IpcClient(bridge));
    const first = await pager.query({}, { before: null, limit: 20 });
    expect(first.olderCursor).toBe(1);
    expect(await pager.query({}, { before: first.olderCursor, limit: 20 })).toEqual({
      events: [],
      olderCursor: null,
    });
  });

  it("puts events the engine recorded out of sequence in order, and pages from the smallest", async () => {
    const bridge = new FakeBridge();
    bridge.request("get_logs", () => ({
      type: "logs",
      events: [event(28), event(30), event(29), event(31)],
    }));
    const page = await new LogPager(new IpcClient(bridge)).query({}, { before: null, limit: 4 });
    expect(page.events.map((e) => e.seq)).toEqual([31, 30, 29, 28]);
    expect(page.olderCursor).toBe(28);
  });

  it("returns an empty last page", async () => {
    const { bridge } = ringBridge(0);
    const pager = new LogPager(new IpcClient(bridge));
    expect(await pager.query({}, { before: null, limit: 10 })).toEqual({
      events: [],
      olderCursor: null,
    });
  });

  it("rejects with an engine error when the engine fails", async () => {
    const bridge = new FakeBridge().request("get_logs", () => {
      throw { kind: "disconnected", message: "gone" };
    });
    const pager = new LogPager(new IpcClient(bridge));
    await expect(pager.query({}, { before: null, limit: 10 })).rejects.toMatchObject({
      code: "engine_unavailable",
    });
  });
});

describe("followLogs", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("subscribes with the filter and delivers events in batches, oldest first", async () => {
    const bridge = new FakeBridge()
      .command("ipc_subscribe", () => 5)
      .command("ipc_unsubscribe", () => null);
    const batches: number[][] = [];
    const stop = followLogs(
      new IpcClient(bridge),
      { minLevel: "error", channelId: "c", event: "delivery.state_changed" },
      (events) => batches.push(events.map((e) => e.seq)),
    );
    await settle();
    expect(bridge.calls.find((c) => c.command === "ipc_subscribe")?.args.topic).toEqual({
      type: "logs",
      filter: {
        min_level: "error",
        since: null,
        until: null,
        channel_id: "c",
        endpoint_id: null,
        text: null,
        event: "delivery.state_changed",
        after_seq: null,
        before_seq: null,
        limit: null,
      },
    });
    for (const seq of [1, 2, 3]) {
      bridge.emit("packie://event", { subscription: 5, event: { type: "log", event: event(seq) } });
    }
    bridge.emit("packie://event", { subscription: 5, event: { type: "lagged", missed: 4 } });
    expect(batches).toEqual([]);
    vi.advanceTimersByTime(150);
    expect(batches).toEqual([[1, 2, 3]]);
    bridge.emit("packie://event", { subscription: 5, event: { type: "log", event: event(4) } });
    vi.advanceTimersByTime(150);
    expect(batches).toEqual([[1, 2, 3], [4]]);

    bridge.emit("packie://event", { subscription: 5, event: { type: "log", event: event(5) } });
    stop();
    vi.advanceTimersByTime(500);
    expect(batches).toHaveLength(2);
    expect(
      bridge.calls.some((c) => c.command === "ipc_unsubscribe" && c.args.subscription === 5),
    ).toBe(true);
  });
});
