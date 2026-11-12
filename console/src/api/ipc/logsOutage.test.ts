import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { LogFilter, LogEvent as RawLogEvent } from "@/ipc/generated";
import type { LogEvent, LogFollowNotice } from "../types";
import type { ConnectionPayload } from "./bridge";
import { IpcClient } from "./client";
import { followLogs } from "./logs";
import { FakeBridge, settle } from "./test/fakeBridge";
import { event, T0 } from "./test/logEvents";

const SINCE = "2026-11-06T11:00:00Z";
const status = (state: ConnectionPayload["state"], since: string): ConnectionPayload => ({
  state,
  endpoint: "pipe",
  error: null,
  attempt: 0,
  engine: null,
  since,
});
/** A time between the events `seq` and `seq + 1` of the test ring. */
const between = (seq: number) => new Date(T0 + seq * 1000 + 500).toISOString();

/**
 * A fake engine: a ring it answers `get_logs` from like the engine does (every filter but the
 * level), a snapshot with a start time, and a subscription events can be pushed through.
 */
function engine(initial: RawLogEvent[]) {
  const state = {
    ring: initial,
    startedAt: "2026-11-06T08:00:00Z",
    snapshotFails: false,
    logFailures: 0,
    connection: status("connected", SINCE),
  };
  const filters: LogFilter[] = [];
  const bridge = new FakeBridge()
    .command("ipc_subscribe", () => 5)
    .command("ipc_unsubscribe", () => null)
    .command("ipc_connection", () => state.connection)
    .request("get_snapshot", () => {
      if (state.snapshotFails) throw { kind: "disconnected", message: "gone" };
      return {
        type: "snapshot",
        snapshot: { node: { started_at: state.startedAt } },
      } as never;
    })
    .request("get_logs", (request) => {
      if (request.type !== "get_logs") throw new Error("unexpected");
      if (state.logFailures > 0) {
        state.logFailures -= 1;
        throw { kind: "disconnected", message: "gone" };
      }
      const { filter } = request;
      filters.push(filter);
      const matching = state.ring.filter(
        (e) =>
          (filter.after_seq === null || e.seq > filter.after_seq) &&
          (filter.before_seq === null || e.seq < filter.before_seq) &&
          (filter.since === null || Date.parse(e.at) >= Date.parse(filter.since)) &&
          (filter.until === null || Date.parse(e.at) < Date.parse(filter.until)) &&
          (filter.event === null || e.event === filter.event),
      );
      return { type: "logs", events: matching.slice(-(filter.limit ?? 500)) };
    });
  const publish = (...events: RawLogEvent[]) => {
    state.ring.push(...events);
  };
  const live = (...events: RawLogEvent[]) => {
    for (const e of events)
      bridge.emit("packie://event", { subscription: 5, event: { type: "log", event: e } });
  };
  const move = (next: ConnectionPayload) => {
    state.connection = next;
    bridge.emit("packie://connection", next);
  };
  return { bridge, state, filters, publish, live, move };
}

const range = (from: number, to: number) =>
  Array.from({ length: to - from + 1 }, (_, i) => event(from + i));

describe("followLogs across an outage", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  type Delivery = { seqs: number[]; notice: LogFollowNotice | undefined };

  async function follow(fake: ReturnType<typeof engine>, query = {}) {
    const deliveries: Delivery[] = [];
    const stop = followLogs(new IpcClient(fake.bridge), query, (events, notice) =>
      deliveries.push({ seqs: events.map((e: LogEvent) => e.seq), notice }),
    );
    await settle(30);
    return { deliveries, stop };
  }

  async function outage(fake: ReturnType<typeof engine>, back = status("connected", between(8))) {
    fake.move(status("reconnecting", between(3)));
    await settle(5);
    return async () => {
      fake.move(back);
      await settle(60);
    };
  }

  /** Follows a ring of events 1..n that arrive live, then loses the connection. */
  async function lostAfter(n: number) {
    const fake = engine(range(1, n));
    const { deliveries } = await follow(fake);
    fake.live(...range(1, n));
    vi.advanceTimersByTime(150);
    return { fake, deliveries, reconnect: await outage(fake) };
  }

  it("delivers what was published during the outage once, in order, with a notice", async () => {
    const fake = engine(range(1, 3));
    const { deliveries } = await follow(fake);
    fake.live(event(1), event(2), event(3));
    vi.advanceTimersByTime(150);
    expect(deliveries).toEqual([{ seqs: [1, 2, 3], notice: undefined }]);

    const reconnect = await outage(fake);
    fake.publish(...range(4, 8));
    await reconnect();
    expect(deliveries[1]).toEqual({
      seqs: [4, 5, 6, 7, 8],
      notice: { lostAt: between(3), backAt: between(8), filled: 5, missing: "none" },
    });
    // The backfill asked for what followed the last event delivered, and for nothing else.
    const asked = fake.filters.find((f) => f.after_seq === 3);
    expect(asked).toMatchObject({ after_seq: 3, limit: 10_000, min_level: null });

    // Events that reach the live stream afterwards, repeats among them, arrive once.
    fake.live(event(8), event(9));
    vi.advanceTimersByTime(150);
    expect(deliveries.map((d) => d.seqs)).toEqual([[1, 2, 3], [4, 5, 6, 7, 8], [9]]);
  });

  it("merges live events that arrive while the missed ones are read, without repeats", async () => {
    const fake = engine(range(1, 3));
    const { deliveries } = await follow(fake);
    fake.live(event(1), event(2), event(3));
    vi.advanceTimersByTime(150);
    await outage(fake);
    fake.publish(...range(4, 9));
    fake.move(status("connected", between(8)));
    // Resubscribed before the read finishes: 8 and 9 come live and are also in the ring.
    fake.live(event(8), event(9), event(10));
    fake.publish(event(10));
    await settle(60);
    expect(deliveries[1]?.seqs).toEqual([4, 5, 6, 7, 8, 9, 10]);
    expect(deliveries[1]?.notice).toMatchObject({ filled: 7, missing: "none" });
  });

  it("asks with the query's filters, and notes an outage with nothing missed", async () => {
    const fake = engine([...range(1, 3).map((e) => ({ ...e, event: "delivery.state_changed" }))]);
    const { deliveries } = await follow(fake, { event: "delivery.state_changed" });
    fake.live({ ...event(3), event: "delivery.state_changed" });
    vi.advanceTimersByTime(150);
    const reconnect = await outage(fake);
    fake.publish({ ...event(4), event: "channel.started" });
    await reconnect();
    expect(deliveries[1]).toEqual({
      seqs: [],
      notice: { lostAt: between(3), backAt: between(8), filled: 0, missing: "none" },
    });
    expect(fake.filters.find((f) => f.after_seq === 3)).toMatchObject({
      event: "delivery.state_changed",
    });
  });

  it("marks an engine restart found from the snapshot, and starts the sequence over", async () => {
    const { fake, deliveries, reconnect } = await lostAfter(5);
    // The engine restarted and has already logged more than the console had seen.
    fake.state.startedAt = "2026-11-06T12:30:00Z";
    fake.state.ring = [event(1), event(2), event(3), event(4), event(5), event(6), event(7)];
    await reconnect();
    expect(deliveries[1]?.notice).toMatchObject({ missing: "restarted", filled: 7 });
    expect(deliveries[1]?.seqs).toEqual([1, 2, 3, 4, 5, 6, 7]);
    fake.live(event(8));
    vi.advanceTimersByTime(150);
    expect(deliveries[2]?.seqs).toEqual([8]);
  });

  it("marks a restart when the sequence went backwards and no snapshot can say", async () => {
    const fake = engine(range(1, 5));
    fake.state.snapshotFails = true;
    const { deliveries } = await follow(fake);
    fake.live(...range(1, 5));
    vi.advanceTimersByTime(150);
    const reconnect = await outage(fake);
    fake.state.ring = [event(1), event(2)];
    await reconnect();
    expect(deliveries[1]).toMatchObject({ seqs: [1, 2], notice: { missing: "restarted" } });
  });

  it("marks a gap when the ring no longer holds events from before the outage", async () => {
    const { fake, deliveries, reconnect } = await lostAfter(3);
    // The ring wrapped: 4 to 19 are gone, and nothing older than the outage is left in it.
    fake.state.ring = range(20, 30);
    await reconnect();
    expect(deliveries[1]?.seqs).toEqual(range(20, 30).map((e) => e.seq));
    expect(deliveries[1]?.notice).toMatchObject({ filled: 11, missing: "gap" });
  });

  it("marks a gap when more was missed than one read returns", async () => {
    const { fake, deliveries, reconnect } = await lostAfter(3);
    fake.publish(...range(4, 10_010));
    await reconnect();
    expect(deliveries[1]?.seqs).toHaveLength(10_000);
    expect(deliveries[1]?.seqs[0]).toBe(11);
    expect(deliveries[1]?.notice).toMatchObject({ missing: "gap", filled: 10_000 });
  });

  it("tries again when the connection drops during the read, and notes the outage once", async () => {
    const { fake, deliveries, reconnect } = await lostAfter(3);
    fake.publish(...range(4, 6));
    fake.state.logFailures = 3;
    const first = reconnect();
    fake.move(status("reconnecting", between(6)));
    await first;
    expect(deliveries).toHaveLength(1);
    fake.move(status("connected", between(9)));
    await settle(60);
    expect(deliveries).toHaveLength(2);
    expect(deliveries[1]).toMatchObject({
      seqs: [4, 5, 6],
      notice: { lostAt: between(3), backAt: between(9), filled: 3 },
    });
  });

  it("says events may be missing when the engine cannot answer while connected", async () => {
    const { fake, deliveries, reconnect } = await lostAfter(3);
    fake.state.logFailures = 100;
    await reconnect();
    expect(deliveries[1]).toMatchObject({ seqs: [], notice: { missing: "gap", filled: 0 } });
  });

  it("does not treat a connection that was never up as an outage", async () => {
    const fake = engine(range(1, 3));
    fake.state.connection = status("disconnected", SINCE);
    const { deliveries } = await follow(fake);
    fake.move(status("connected", between(2)));
    await settle(60);
    expect(deliveries).toEqual([]);
    expect(fake.bridge.sent("get_logs")).toEqual([]);
  });

  it("stops listening to the connection and the stream when stopped", async () => {
    const fake = engine(range(1, 3));
    const { stop, deliveries } = await follow(fake);
    stop();
    fake.move(status("reconnecting", between(3)));
    fake.move(status("connected", between(4)));
    await settle(60);
    expect(deliveries).toEqual([]);
    expect(fake.bridge.sent("get_logs")).toEqual([]);
  });
});
