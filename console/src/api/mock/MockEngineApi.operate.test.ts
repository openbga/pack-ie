import { describe, expect, it, vi } from "vitest";
import { channelOf, mockApiFactory, rejection } from "./mockTestSupport";

const create = mockApiFactory({ messageCount: 500, seed: 7 });

describe("commands keep the snapshot consistent", () => {
  it("pause closes the session, disconnects the endpoints and schedules nothing", async () => {
    const api = create();
    for (const id of ["ch-br-au5800", "ch-br-c503"]) {
      await api.pauseChannel(id);
      const channel = await channelOf(api, id);
      expect(channel).toMatchObject({
        state: "stopped",
        stopReason: "paused",
        nextRetryAt: null,
        instanceStartedAt: null,
        pressure: 0,
        tapActive: false,
      });
      expect(channel.bridge?.sessionState).toBe("closed");
      expect(channel.bridge?.endpointA.state).toBe("disconnected");
      expect(channel.bridge?.endpointB.state).toBe("disconnected");
      expect(channel.bridge?.endpointA.consecutiveFailureCount).toBe(0);
      expect(channel.bridge?.endpointB.activePeer).toBeNull();
      expect(channel.endpoints.every((e) => e.state === "disconnected")).toBe(true);
    }
    const snapshot = await api.getSnapshot();
    expect(snapshot.endpoints.find((e) => e.id === "ep-a-au5800")).toMatchObject({
      state: "disconnected",
      heldBy: null,
    });
    await api.pauseChannel("ch-msg-au680");
    const message = await channelOf(api, "ch-msg-au680");
    expect(message.source?.session).toBe("closed");
    expect(message.source?.endpoint.state).toBe("disconnected");
  });

  it("resume starts a new instance from the active version with fresh counters", async () => {
    vi.useFakeTimers();
    const api = create();
    const id = "ch-br-ci4100";
    const before = await channelOf(api, id);
    expect(before.counters.bytesIn).toBeGreaterThan(0);
    await api.pauseChannel(id);
    vi.advanceTimersByTime(10_000);
    await api.resumeChannel(id);
    const channel = await channelOf(api, id);
    const snapshot = await api.getSnapshot();
    expect(channel.state).toBe("starting");
    expect(channel.stopReason).toBeNull();
    expect(channel.configVersion).toBe(snapshot.node.configVersion);
    expect(Date.parse(channel.instanceStartedAt ?? "")).toBe(Date.parse(snapshot.takenAt));
    expect(channel.instanceStartedAt).not.toBe(before.instanceStartedAt);
    expect(channel.counters).toEqual({
      bytesIn: 0,
      bytesOut: 0,
      messagesIn: 0,
      messagesOut: 0,
      retries: 0,
    });
    expect(channel.bridge?.sessionState).toBe("waiting_for_peer");
    expect(channel.bridge?.overflowCount).toBe(0);
    vi.advanceTimersByTime(1600);
    expect((await channelOf(api, id)).bridge?.sessionState).toBe("bridged");
  });

  it("restart resets the counters of the instance and keeps what the store holds", async () => {
    vi.useFakeTimers();
    const api = create();
    const bridge = await channelOf(api, "ch-br-im1600");
    expect(bridge.bridge?.overflowCount).toBe(1);
    await api.restartChannel("ch-br-im1600");
    const restarted = await channelOf(api, "ch-br-im1600");
    expect(restarted.counters.bytesIn).toBe(0);
    expect(restarted.bridge?.overflowCount).toBe(0);
    expect(restarted.buffers.some((b) => b.overflowing)).toBe(false);
    expect(restarted.state).toBe("starting");

    const id = "ch-msg-sta";
    const before = await api.getChannel(id);
    const acknowledged = before.destinations.reduce((n, d) => n + d.counters.acknowledged, 0);
    await api.restartChannel(id);
    const after = await api.getChannel(id);
    expect(after.snapshot.counters.messagesIn).toBe(0);
    expect(after.snapshot.source).toMatchObject({ received: 0, duplicates: 0, filtered: 0 });
    expect(after.destinations.reduce((n, d) => n + d.counters.acknowledged, 0)).toBe(acknowledged);
  });

  it("applying a configuration restarts changed channels onto the new version only", async () => {
    const api = create();
    const config = await api.getConfig();
    const changed = "ch-br-au5800";
    config.channels = config.channels.map((c) =>
      c.id === changed && c.kind === "bridge" ? { ...c, bufferBytes: c.bufferBytes * 2 } : c,
    );
    const result = await api.applyConfig(config, "bigger buffer");
    expect(result.restartedChannelIds).toEqual([changed]);
    expect((await channelOf(api, changed)).configVersion).toBe(result.version);
    expect((await channelOf(api, "ch-br-e801")).configVersion).toBe(config.version);
  });
});

describe("command rejections use the engine's wording", () => {
  it("rejects with conflict or not_found and the engine's text", async () => {
    const api = create();
    const stopped = await rejection(api.pauseChannel("ch-msg-vitros350"));
    expect(stopped).toMatchObject({ code: "conflict", message: "Channel is already stopped" });
    expect(await rejection(api.pauseChannel("ch-msg-worklist"))).toMatchObject({
      code: "conflict",
      message: "Channel is already stopped",
    });
    expect(await rejection(api.resumeChannel("ch-br-au5800"))).toMatchObject({
      code: "conflict",
      message: "Not paused",
    });
    expect(await rejection(api.resumeChannel("ch-msg-worklist"))).toMatchObject({
      code: "conflict",
      message: "Disabled in configuration: enable it in the configuration editor",
    });
    expect(await rejection(api.restartChannel("ch-msg-vitros350"))).toMatchObject({
      code: "conflict",
      message: "Channel is paused: use Resume",
    });
    for (const command of [api.pauseChannel, api.resumeChannel, api.restartChannel]) {
      const missing = await rejection(command.call(api, "ch-gone"));
      expect(missing).toMatchObject({
        code: "not_found",
        message: "No longer exists",
        fieldErrors: [],
      });
      expect(missing.correlationId).toMatch(/^c-[0-9a-f]{6}$/);
    }
  });
});
