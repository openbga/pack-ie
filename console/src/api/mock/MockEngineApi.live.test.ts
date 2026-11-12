import { describe, expect, it, vi } from "vitest";
import type { LogEvent, TrafficSegment } from "../types";
import { mockApiFactory } from "./mockTestSupport";

const create = mockApiFactory({ messageCount: 5000, seed: 7 });

describe("logs", () => {
  it("returns newest first with cursor paging over a bounded ring", async () => {
    const api = create({ logRingSize: 300 });
    const first = await api.queryLogs({}, { before: null, limit: 100 });
    expect(first.events).toHaveLength(100);
    for (let i = 1; i < first.events.length; i++) {
      expect(first.events[i]?.seq).toBeLessThan(first.events[i - 1]?.seq ?? 0);
    }
    expect(first.olderCursor).toBe(first.events[99]?.seq);
    const second = await api.queryLogs({}, { before: first.olderCursor, limit: 100 });
    const third = await api.queryLogs({}, { before: second.olderCursor, limit: 100 });
    expect(third.events).toHaveLength(100);
    expect(third.olderCursor).toBeNull();
    const seqs = [...first.events, ...second.events, ...third.events].map((e) => e.seq);
    expect(new Set(seqs).size).toBe(300);
    expect(seqs).toEqual([...seqs].sort((a, b) => b - a));
  });

  it("filters by level, channel, endpoint, time range and text", async () => {
    const api = create();
    const warn = await api.queryLogs({ minLevel: "warn" }, { before: null, limit: 500 });
    expect(warn.events.length).toBeGreaterThan(100);
    expect(warn.events.every((e) => e.level === "warn" || e.level === "error")).toBe(true);
    const levels = new Set(
      (await api.queryLogs({}, { before: null, limit: 3000 })).events.map((e) => e.level),
    );
    expect(levels.size).toBe(5);

    const channelId = "ch-msg-au680";
    const channel = await api.queryLogs({ channelId }, { before: null, limit: 200 });
    expect(channel.events.length).toBeGreaterThan(20);
    expect(channel.events.every((e) => e.channelId === channelId)).toBe(true);
    const sample = channel.events.find((e) => e.endpointId !== null);
    const endpoint = await api.queryLogs(
      { endpointId: sample?.endpointId ?? "" },
      { before: null, limit: 50 },
    );
    expect(endpoint.events.every((e) => e.endpointId === sample?.endpointId)).toBe(true);

    const all = (await api.queryLogs({}, { before: null, limit: 1000 })).events;
    const to = all[100]?.at ?? "";
    const from = all[300]?.at ?? "";
    const ranged = await api.queryLogs({ from, to }, { before: null, limit: 1000 });
    expect(ranged.events.every((e) => e.at >= from && e.at <= to)).toBe(true);
    expect(ranged.events.length).toBeGreaterThanOrEqual(200);

    const connected = await api.queryLogs({ text: "CONNECTED TO" }, { before: null, limit: 50 });
    expect(connected.events.length).toBeGreaterThan(0);
    expect(connected.events.every((e) => /connected to \S+:\d+|connected to/.test(e.message))).toBe(
      true,
    );
    // The engine looks in field names and values (and the target), not only the message.
    const byField = await api.queryLogs({ text: "ACK_LEVEL" }, { before: null, limit: 50 });
    expect(byField.events.length).toBeGreaterThan(0);
    expect(byField.events.every((e) => "ack_level" in e.fields)).toBe(true);
    const byTarget = await api.queryLogs({ text: "packie_transport" }, { before: null, limit: 50 });
    expect(byTarget.events.length).toBeGreaterThan(0);
    expect(byTarget.events.every((e) => e.target.includes("packie_transport"))).toBe(true);
    const bodies = JSON.stringify(all.map((e) => [e.message, e.fields]));
    expect(bodies).not.toContain("MSH|");
    expect(bodies).not.toContain("OBX|");
  });

  it("follows new events in batches, oldest first, and adds them to the ring", async () => {
    vi.useFakeTimers();
    const api = create({ logRingSize: 200 });
    const batches: LogEvent[][] = [];
    const stop = api.followLogs({}, (events) => batches.push(events));
    expect(batches).toHaveLength(0);
    vi.advanceTimersByTime(10_000);
    expect(batches.length).toBeGreaterThanOrEqual(10);
    expect(batches.length).toBeLessThanOrEqual(21);
    const events = batches.flat();
    const perSecond = events.length / 10;
    expect(perSecond).toBeGreaterThan(1.5);
    expect(perSecond).toBeLessThan(11);
    for (const batch of batches) {
      for (let i = 1; i < batch.length; i++) {
        expect(batch[i]?.seq).toBeGreaterThan(batch[i - 1]?.seq ?? 0);
      }
    }
    const newest = await api.queryLogs({}, { before: null, limit: 1 });
    expect(newest.events[0]?.seq).toBe(events[events.length - 1]?.seq);
    const all = await api.queryLogs({}, { before: null, limit: 1000 });
    expect(all.events.length).toBeLessThanOrEqual(200);

    stop();
    const count = batches.length;
    vi.advanceTimersByTime(5000);
    expect(batches).toHaveLength(count);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("applies the query to followed events", async () => {
    vi.useFakeTimers();
    const api = create();
    const seen: LogEvent[] = [];
    const stop = api.followLogs({ minLevel: "info", channelId: "ch-msg-au680" }, (e) =>
      seen.push(...e),
    );
    vi.advanceTimersByTime(120_000);
    stop();
    expect(
      seen.every(
        (e) => e.channelId === "ch-msg-au680" && e.level !== "debug" && e.level !== "trace",
      ),
    ).toBe(true);
  });
});

describe("traffic", () => {
  async function collect(channelId: string, ms = 8000) {
    vi.useFakeTimers();
    const api = create();
    const segments: TrafficSegment[] = [];
    const stop = api.subscribeTraffic(channelId, (batch) => segments.push(...batch));
    vi.advanceTimersByTime(ms);
    return { api, segments, stop };
  }

  it("emits ASTM control characters and checksummed frames in both directions", async () => {
    const { segments, stop } = await collect("ch-msg-xn1000", 20_000);
    expect(segments.length).toBeGreaterThan(8);
    const decoded = segments.map((s) => atob(s.base64));
    expect(decoded).toContain("\x05");
    expect(decoded).toContain("\x06");
    expect(decoded.some((d) => d.startsWith("\x02"))).toBe(true);
    expect(new Set(segments.map((s) => s.direction))).toEqual(new Set(["a_to_b", "b_to_a"]));
    for (let i = 1; i < segments.length; i++) {
      expect(segments[i]?.seq).toBeGreaterThan(segments[i - 1]?.seq ?? 0);
    }
    stop();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("frames HL7 with MLLP and shows the tap in the snapshot while watched", async () => {
    const { api, segments, stop } = await collect("ch-msg-billing");
    const decoded = segments.map((s) => atob(s.base64));
    const message = decoded.find((d) => d.startsWith("\x0bMSH|"));
    expect(message?.endsWith("\x1c\r")).toBe(true);
    expect(decoded.some((d) => d.includes("MSA|AA"))).toBe(true);
    const tapped = (await api.getSnapshot()).channels.find((c) => c.id === "ch-msg-billing");
    expect(tapped?.tapActive).toBe(true);
    stop();
    stop();
    expect(
      (await api.getSnapshot()).channels.find((c) => c.id === "ch-msg-billing")?.tapActive,
    ).toBe(false);
  });

  it("emits nothing for an unknown channel", async () => {
    const { segments, stop } = await collect("ch-nope");
    expect(segments).toEqual([]);
    stop();
  });
});
