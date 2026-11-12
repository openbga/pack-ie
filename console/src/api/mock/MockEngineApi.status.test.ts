import { describe, expect, it, vi } from "vitest";
import type { ChannelSnapshot, EndpointSnapshot, Snapshot } from "../types";
import { mockApiFactory } from "./mockTestSupport";

const create = mockApiFactory({ messageCount: 500, seed: 7 });

function channelOf(snapshot: Snapshot, id: string): ChannelSnapshot {
  const found = snapshot.channels.find((c) => c.id === id);
  if (found === undefined) throw new Error(`no channel ${id}`);
  return found;
}

function endpointOf(snapshot: Snapshot, id: string): EndpointSnapshot {
  const found = snapshot.endpoints.find((e) => e.id === id);
  if (found === undefined) throw new Error(`no endpoint ${id}`);
  return found;
}

function bridges(snapshot: Snapshot): ChannelSnapshot[] {
  return snapshot.channels.filter((c) => c.bridge !== null);
}

function expectConsistentSession(channel: ChannelSnapshot): void {
  const bridge = channel.bridge;
  if (bridge === null) return;
  const sides = [bridge.endpointA, bridge.endpointB];
  if (channel.state === "running" || channel.state === "degraded") {
    expect(bridge.sessionState).toBe("bridged");
    expect(sides.every((side) => side.state === "connected")).toBe(true);
  }
  if (channel.state === "stopped") {
    expect(bridge.sessionState).toBe("closed");
    expect(sides.every((side) => side.state === "disconnected")).toBe(true);
    expect(channel.instanceStartedAt).toBeNull();
  }
  if (bridge.sessionState === "waiting_for_peer" && channel.state === "retrying") {
    expect(bridge.endpointA.state).toBe("listening");
    expect(bridge.endpointA.activePeer).toBeNull();
  }
  expect(channel.endpoints.map((e) => e.state)).toEqual(sides.map((side) => side.state));
  expect([bridge.endpointA.role, bridge.endpointB.role]).toEqual(["a", "b"]);
}

describe("bridge status", () => {
  it("keeps the session state consistent with channel and endpoint states", async () => {
    const snapshot = await create().getSnapshot();
    for (const channel of bridges(snapshot)) expectConsistentSession(channel);
    const sessions = new Set(bridges(snapshot).map((c) => c.bridge?.sessionState));
    expect(sessions).toContain("bridged");
    expect(sessions).toContain("closed");
    expect(sessions).toContain("waiting_for_peer");
  });

  it("reports tap counts only with a framer, and the receiver from the pairing", async () => {
    const api = create();
    const snapshot = await api.getSnapshot();
    const config = await api.getConfig();
    for (const channel of bridges(snapshot)) {
      const bridge = channel.bridge;
      const setting = config.channels.find((c) => c.id === channel.id);
      if (bridge === null || setting?.kind !== "bridge") continue;
      const framed = setting.tapFramer !== "none";
      expect(bridge.tap === null).toBe(!framed);
      expect(bridge.messagesAToB === null).toBe(!framed);
      expect(bridge.messagesBToA === null).toBe(!framed);
      expect(bridge.framingErrors === null).toBe(!framed);
      expect(bridge.tapDropped === null).toBe(!framed);
      expect(bridge.tap?.framer ?? "none").toBe(setting.tapFramer);
      expect(bridge.connectStrategy).toBe(setting.pairing);
      expect(bridge.receiver).toBe(
        setting.pairing === "receiver_then_sender" ? setting.pairingReceiver : null,
      );
      expect(bridge.bytesAToB).toBe(channel.counters.bytesIn);
      expect(bridge.bytesBToA).toBe(channel.counters.bytesOut);
    }
    const receivers = bridges(snapshot).map((c) => c.bridge?.receiver);
    expect(receivers).toContain("a");
  });

  it("shows an overflow episode as a degraded bridge with a full, overflowing buffer", async () => {
    const snapshot = await create().getSnapshot();
    const degraded = bridges(snapshot).filter((c) => c.state === "degraded");
    expect(degraded.length).toBeGreaterThan(0);
    for (const channel of degraded) {
      expect(channel.bridge?.overflowCount).toBeGreaterThanOrEqual(1);
      expect(channel.buffers.some((b) => b.overflowing)).toBe(true);
      expect(channel.pressure).toBeGreaterThanOrEqual(0.95);
      expect(channel.recentErrors.length).toBeGreaterThan(0);
    }
  });

  it("makes buffers, segments and pressure agree", async () => {
    const snapshot = await create().getSnapshot();
    for (const channel of bridges(snapshot)) {
      expect(channel.buffers.map((b) => b.direction)).toEqual(["a_to_b", "b_to_a"]);
      for (const buffer of channel.buffers) {
        expect(buffer.usedBytes).toBeLessThanOrEqual(buffer.capacityBytes);
        expect(buffer.maxSegments).toBeGreaterThan(0);
        expect(buffer.segments).toBeLessThanOrEqual(buffer.maxSegments ?? 0);
        expect(buffer.segments === 0).toBe(buffer.usedBytes === 0);
      }
      const fullest = Math.max(...channel.buffers.map((b) => b.usedBytes / b.capacityBytes));
      expect(channel.pressure).toBeCloseTo(fullest, 2);
      if (channel.state === "stopped") expect(channel.pressure).toBe(0);
      if (channel.buffers.some((b) => b.overflowing)) {
        expect(channel.bridge?.overflowCount).toBeGreaterThanOrEqual(1);
      }
    }
  });

  it("grows per-direction bytes with traffic and ends an overflow episode", async () => {
    vi.useFakeTimers();
    const api = create();
    const first = await api.getSnapshot();
    const id = "ch-br-au5800";
    const before = channelOf(first, id).bridge;
    api.subscribeSnapshots(() => {});
    vi.advanceTimersByTime(120_000);
    const after = await api.getSnapshot();
    const now = channelOf(after, id).bridge;
    expect(now?.bytesAToB ?? 0).toBeGreaterThan(before?.bytesAToB ?? 0);
    expect(now?.bytesBToA ?? 0).toBeGreaterThan(before?.bytesBToA ?? 0);
    expect(now?.messagesAToB ?? 0).toBeGreaterThan(before?.messagesAToB ?? 0);
    const episode = channelOf(after, "ch-br-im1600");
    expect(episode.buffers.some((b) => b.overflowing)).toBe(false);
    expect(episode.bridge?.overflowCount).toBeGreaterThanOrEqual(1);
  });
});

describe("source status", () => {
  it("gives every message channel a source consistent with its state", async () => {
    const api = create();
    const snapshot = await api.getSnapshot();
    const config = await api.getConfig();
    const messages = snapshot.channels.filter((c) => c.kind === "message");
    const sessions = new Set<string>();
    for (const channel of messages) {
      const source = channel.source;
      expect(source).not.toBeNull();
      if (source === null) continue;
      sessions.add(source.session);
      expect(channel.bridge).toBeNull();
      expect(source.endpoint.role).toBe("source");
      expect(source.endpoint.endpointId).toBe(channel.endpoints[0]?.endpointId);
      expect(source.protocol).toBe(channel.protocol);
      expect(source.received).toBe(channel.counters.messagesIn);
      expect(source.duplicates + source.invalid + source.filtered + source.aborted).toBeLessThan(
        source.received,
      );
      expect(source.lastReceivedAt).toBe(channel.lastActivityAt);
      if (channel.state === "stopped") expect(source.session).toBe("closed");
      const setting = config.channels.find((c) => c.id === channel.id);
      if (setting?.kind === "message" && !setting.source.duplicateDetection) {
        expect(source.duplicates).toBe(0);
      }
    }
    expect(sessions).toContain("closed");
    expect(sessions).toContain("idle");
  });

  it("counts received messages and their outcomes as traffic arrives", async () => {
    vi.useFakeTimers();
    const api = create();
    const id = "ch-msg-au680";
    const before = channelOf(await api.getSnapshot(), id).source;
    api.subscribeSnapshots(() => {});
    vi.advanceTimersByTime(300_000);
    const channel = channelOf(await api.getSnapshot(), id);
    const after = channel.source;
    expect(after?.received ?? 0).toBeGreaterThan(before?.received ?? 0);
    expect(after?.received).toBe(channel.counters.messagesIn);
    const outcomes = (s: typeof after) =>
      (s?.duplicates ?? 0) + (s?.invalid ?? 0) + (s?.filtered ?? 0) + (s?.aborted ?? 0);
    expect(outcomes(after)).toBeGreaterThanOrEqual(outcomes(before));
    expect(Date.parse(after?.lastReceivedAt ?? "")).toBeGreaterThan(
      Date.parse(before?.lastReceivedAt ?? ""),
    );
  });
});

describe("retries", () => {
  it("reports streaks, history and the next retry while a channel retries", async () => {
    const snapshot = await create().getSnapshot();
    const takenAt = Date.parse(snapshot.takenAt);
    const channel = channelOf(snapshot, "ch-br-c503");
    expect(channel.state).toBe("retrying");
    expect(channel.bridge?.sessionState).toBe("closed");
    expect(channel.bridge?.endpointB.consecutiveFailureCount).toBeGreaterThan(0);
    expect(channel.bridge?.endpointA.consecutiveFailureCount).toBe(0);
    const retryAt = Date.parse(channel.nextRetryAt ?? "");
    expect(retryAt).toBeGreaterThan(takenAt);
    expect(retryAt - takenAt).toBeLessThanOrEqual(25_000);
    expect(channel.recentErrors.length).toBeGreaterThan(1);
    expect(channel.recentErrors.length).toBeLessThanOrEqual(10);
    const times = channel.recentErrors.map((e) => e.at);
    expect(times).toEqual([...times].sort().reverse());
    const endpoint = endpointOf(snapshot, "ep-mw-2");
    expect(endpoint.state).toBe("retrying");
    expect(endpoint.consecutiveFailureCount).toBe(
      channel.bridge?.endpointB.consecutiveFailureCount,
    );
    expect(endpoint.nextRetryAt).toBe(channel.nextRetryAt);
    expect(endpoint.heldBy).toBeNull();
    expect(endpoint.activePeer).toBeNull();
  });

  it("schedules the next retry once one has passed, and the streak grows", async () => {
    vi.useFakeTimers();
    const api = create();
    const first = endpointOf(await api.getSnapshot(), "ep-a-acltop");
    api.subscribeSnapshots(() => {});
    const seen = new Set<string>();
    for (let i = 0; i < 60; i++) {
      vi.advanceTimersByTime(1000);
      const now = Date.now();
      const snapshot = await api.getSnapshot();
      const endpoint = endpointOf(snapshot, "ep-a-acltop");
      const next = Date.parse(endpoint.nextRetryAt ?? "");
      expect(next).toBeGreaterThan(now);
      expect(next - now).toBeLessThanOrEqual(21_000);
      expect(channelOf(snapshot, "ch-br-acltop").nextRetryAt).toBe(endpoint.nextRetryAt);
      seen.add(endpoint.nextRetryAt ?? "");
    }
    expect(seen.size).toBeGreaterThanOrEqual(3);
    const last = endpointOf(await api.getSnapshot(), "ep-a-acltop");
    expect(last.consecutiveFailureCount).toBeGreaterThan(first.consecutiveFailureCount);
  });

  it("clears the streak and the retry when the endpoint recovers", async () => {
    vi.useFakeTimers();
    const api = create();
    const config = await api.getConfig();
    const serial = config.endpoints.find((e) => e.id === "ep-a-acltop");
    if (serial?.kind !== "serial") throw new Error("expected a serial endpoint");
    config.endpoints = config.endpoints.map((e) =>
      e.id === serial.id ? { ...serial, port: "COM19" } : e,
    );
    await api.applyConfig(config, "fix the port");
    const snapshot = await api.getSnapshot();
    const endpoint = endpointOf(snapshot, "ep-a-acltop");
    expect(endpoint.consecutiveFailureCount).toBe(0);
    expect(endpoint.nextRetryAt).toBeNull();
    expect(channelOf(snapshot, "ch-br-acltop").nextRetryAt).toBeNull();
    expect(channelOf(snapshot, "ch-br-acltop").bridge?.endpointA.consecutiveFailureCount).toBe(0);
  });

  it("keeps a bounded, newest-first error history that grows with failed attempts", async () => {
    vi.useFakeTimers();
    const api = create();
    api.subscribeSnapshots(() => {});
    vi.advanceTimersByTime(120_000);
    const channel = channelOf(await api.getSnapshot(), "ch-br-c503");
    expect(channel.recentErrors).toHaveLength(10);
    const times = channel.recentErrors.map((e) => e.at);
    expect(times).toEqual([...times].sort().reverse());
    expect(channel.counters.retries).toBeGreaterThan(0);
  });
});

describe("endpoint ownership", () => {
  it("names the channel holding a serial port or listener socket", async () => {
    const snapshot = await create().getSnapshot();
    for (const endpoint of snapshot.endpoints) {
      if (endpoint.heldBy === null) continue;
      expect(["serial", "tcp_listener"]).toContain(endpoint.kind);
      expect(endpoint.channelIds).toContain(endpoint.heldBy);
      expect(channelOf(snapshot, endpoint.heldBy).state).not.toBe("stopped");
    }
    const serial = endpointOf(snapshot, "ep-a-au5800");
    expect(serial.heldBy).toBe("ch-br-au5800");
    expect(serial.activePeer).toBeNull();
    const listener = endpointOf(snapshot, "ep-a-xn3000");
    expect(listener.state).toBe("listening");
    expect(listener.heldBy).toBe("ch-br-xn3000");
    expect(listener.activePeer).toBeNull();
    expect(endpointOf(snapshot, "ep-a-d100").heldBy).toBeNull();
    const client = endpointOf(snapshot, "ep-mw-1");
    expect(client.activePeer).toBe("10.0.4.12:3001");
    expect(client.heldBy).toBeNull();
  });

  it("gives a connected listener a peer address", async () => {
    const snapshot = await create().getSnapshot();
    const connected = snapshot.endpoints.filter(
      (e) => e.kind === "tcp_listener" && e.state === "connected",
    );
    expect(connected.length).toBeGreaterThan(0);
    for (const endpoint of connected) expect(endpoint.activePeer).toMatch(/^[\d.]+:\d+$/);
  });
});
