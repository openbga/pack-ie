import { describe, expect, it, vi } from "vitest";
import type { AlertKind, ChannelState, EndpointKind, Snapshot, StopReason } from "../types";
import type { MockEngineApi } from "./index";
import { messageChannel, mockApiFactory } from "./mockTestSupport";

const create = mockApiFactory({ messageCount: 5000, seed: 7 });

describe("snapshot", () => {
  it("describes one lab computer with channels in every state", async () => {
    const snapshot = await create().getSnapshot();
    expect(snapshot.node.nodeId).toBe("lab-pc-01");
    expect(snapshot.node.nodeName).toBe("Main Lab Interface PC");
    expect(snapshot.channels.filter((c) => c.kind === "bridge").length).toBeGreaterThan(0);
    expect(snapshot.channels.filter((c) => c.kind === "message").length).toBeGreaterThan(0);

    const states = new Set<ChannelState>(snapshot.channels.map((c) => c.state));
    for (const state of ["running", "degraded", "retrying", "starting", "stopped"] as const) {
      expect(states).toContain(state);
    }
    const reasons = new Set<StopReason | null>(snapshot.channels.map((c) => c.stopReason));
    for (const reason of ["paused", "disabled", "configuration", "fatal"] as const) {
      expect(reasons).toContain(reason);
    }
    const running = snapshot.channels.filter((c) => c.state === "running").length;
    expect(running).toBeGreaterThan(snapshot.channels.length / 2);
    const kinds = new Set<EndpointKind>(snapshot.endpoints.map((e) => e.kind));
    expect(kinds.size).toBe(5);
  });

  it("keeps endpoint states consistent with channel states and explains failures", async () => {
    const snapshot = await create().getSnapshot();
    for (const channel of snapshot.channels) {
      if (channel.state !== "degraded" && channel.state !== "retrying") continue;
      if (channel.bridge?.sessionState === "waiting_for_peer") {
        // A listener waiting for its instrument is retrying without a failure to explain.
        expect(channel.endpoints.some((e) => e.state === "listening")).toBe(true);
      } else if (channel.buffers.some((b) => b.overflowing)) {
        expect(channel.state).toBe("degraded");
        expect(channel.lastError?.nextAction).toBeTruthy();
      } else {
        expect(channel.endpoints.some((e) => ["error", "retrying"].includes(e.state))).toBe(true);
        expect(channel.lastError?.summary).toBeTruthy();
        expect(channel.lastError?.nextAction).toBeTruthy();
      }
    }
    for (const endpoint of snapshot.endpoints.filter((e) => e.state === "error")) {
      expect(endpoint.lastError?.nextAction).toBeTruthy();
    }
    const stopped = snapshot.channels.filter(
      (c) => c.stopReason === "configuration" || c.stopReason === "fatal",
    );
    expect(stopped).toHaveLength(2);
    for (const channel of stopped) expect(channel.lastError?.nextAction).toBeTruthy();
    expect(snapshot.node.state).toBe("degraded");
    expect(snapshot.channels.some((c) => (c.queue?.needsAttention ?? 0) > 0)).toBe(true);
  });

  it("matches alerts: open alerts equal listAlerts and every kind is covered", async () => {
    const api = create();
    const snapshot = await api.getSnapshot();
    const open = await api.listAlerts("open");
    expect(snapshot.openAlerts.map((a) => a.id)).toEqual(open.map((a) => a.id));
    expect(open.length).toBeGreaterThan(3);
    expect(open.every((a) => a.state === "open" && a.clearedAt === null)).toBe(true);
    const order = { critical: 0, warning: 1, info: 2 };
    for (let i = 1; i < open.length; i++) {
      expect(order[open[i]?.severity ?? "info"]).toBeGreaterThanOrEqual(
        order[open[i - 1]?.severity ?? "info"],
      );
    }

    const all = await api.listAlerts("all");
    const cleared = await api.listAlerts("cleared");
    expect(cleared.length).toBeGreaterThanOrEqual(10);
    expect(cleared.every((a) => a.state === "cleared" && a.clearedAt !== null)).toBe(true);
    expect(all).toHaveLength(open.length + cleared.length);
    const kinds = new Set<AlertKind>(all.map((a) => a.kind));
    for (const kind of [
      "endpoint_down",
      "delivery_needs_attention",
      "delivery_backlog",
      "bridge_buffer_pressure",
      "disk_low",
      "no_peer_connected",
    ] as const) {
      expect(kinds).toContain(kind);
    }
    expect(new Set(all.map((a) => a.severity))).toContain("critical");
    expect(new Set(all.map((a) => a.severity))).toContain("warning");
  });

  it("is stable for a seed and different for another", async () => {
    const a = await create({ seed: 3 }).listMessages({
      filter: {},
      sort: { field: "receivedAt", descending: true },
      offset: 0,
      limit: 20,
    });
    const b = await create({ seed: 3 }).listMessages({
      filter: {},
      sort: { field: "receivedAt", descending: true },
      offset: 0,
      limit: 20,
    });
    const c = await create({ seed: 4 }).listMessages({
      filter: {},
      sort: { field: "receivedAt", descending: true },
      offset: 0,
      limit: 20,
    });
    const strip = (rows: typeof a.rows) => rows.map((r) => ({ ...r, receivedAt: "" }));
    expect(strip(a.rows)).toEqual(strip(b.rows));
    expect(strip(a.rows)).not.toEqual(strip(c.rows));
  });
});

describe("live snapshots", () => {
  it("publishes at once and every second, ticks counters, and stops on unsubscribe", async () => {
    vi.useFakeTimers();
    const api = create();
    const received: Snapshot[] = [];
    const stop = api.subscribeSnapshots((s) => received.push(s));
    await Promise.resolve();
    expect(received).toHaveLength(1);
    vi.advanceTimersByTime(1000);
    expect(received).toHaveLength(2);
    vi.advanceTimersByTime(6000);
    expect(received).toHaveLength(8);

    const first = received[0] as Snapshot;
    const last = received[received.length - 1] as Snapshot;
    const total = (s: Snapshot) => s.channels.reduce((sum, c) => sum + c.counters.messagesIn, 0);
    expect(total(last)).toBeGreaterThan(total(first));
    const active = last.channels.find((c) => c.state === "running" && c.lastActivityAt !== null);
    expect(active?.lastActivityAt).toBeTruthy();
    expect(last.takenAt > first.takenAt).toBe(true);

    stop();
    stop();
    vi.advanceTimersByTime(5000);
    expect(received).toHaveLength(8);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("keeps node state and open alerts consistent in every published snapshot", async () => {
    vi.useFakeTimers();
    const api = create();
    const received: Snapshot[] = [];
    api.subscribeSnapshots((s) => received.push(s));
    vi.advanceTimersByTime(120_000);
    for (const snapshot of received) {
      const unhealthy =
        snapshot.channels.some((c) => c.state === "degraded" || c.state === "retrying") ||
        snapshot.endpoints.some((e) => e.state === "error" || e.state === "retrying");
      if (unhealthy) expect(snapshot.node.state).toBe("degraded");
    }
    const open = await api.listAlerts("open");
    expect(open.map((a) => a.id)).toEqual(
      (received[received.length - 1] as Snapshot).openAlerts.map((a) => a.id),
    );
  });
});

describe("simulation over time", () => {
  it("flips endpoints and channels now and then, opening and clearing alerts", async () => {
    vi.useFakeTimers();
    const api = create();
    const before = await api.listAlerts("all");
    api.subscribeSnapshots(() => {});
    vi.advanceTimersByTime(20 * 60_000);
    const after = await api.listAlerts("all");
    expect(after.length).toBeGreaterThan(before.length);
    const ids = new Set(before.map((a) => a.id));
    const fresh = after.filter((a) => !ids.has(a.id));
    expect(fresh.some((a) => a.state === "cleared")).toBe(true);
    const logs = await api.queryLogs({ text: "alert opened" }, { before: null, limit: 10 });
    expect(logs.events.length).toBeGreaterThan(0);
  });

  it("uses the clock it is given", async () => {
    const api = create({ now: () => Date.parse("2030-01-02T03:04:05.000Z") });
    const snapshot = await api.getSnapshot();
    expect(snapshot.takenAt).toBe("2030-01-02T03:04:05.000Z");
    const newest = await api.listMessages({
      filter: {},
      sort: { field: "receivedAt", descending: true },
      offset: 0,
      limit: 1,
    });
    expect(newest.rows[0]?.receivedAt.startsWith("2030-01-02T03:0")).toBe(true);
  });
});

describe("channel commands", () => {
  const id = "ch-msg-au680";

  async function state(api: MockEngineApi, channelId = id) {
    const snapshot = await api.getSnapshot();
    return snapshot.channels.find((c) => c.id === channelId);
  }

  it("pauses, resumes through starting, and rejects impossible transitions", async () => {
    vi.useFakeTimers();
    const api = create();
    await api.pauseChannel(id);
    expect(await state(api)).toMatchObject({ state: "stopped", stopReason: "paused" });
    await expect(api.pauseChannel(id)).rejects.toMatchObject({ code: "conflict" });
    await expect(api.restartChannel(id)).rejects.toMatchObject({ code: "conflict" });

    await api.resumeChannel(id);
    expect(await state(api)).toMatchObject({ state: "starting", stopReason: null });
    vi.advanceTimersByTime(1600);
    expect(await state(api)).toMatchObject({ state: "running" });
    await expect(api.resumeChannel(id)).rejects.toMatchObject({ code: "conflict" });
  });

  it("restarts through starting and refuses unknown, disabled and config-stopped channels", async () => {
    vi.useFakeTimers();
    const api = create();
    await api.restartChannel(id);
    expect((await state(api))?.state).toBe("starting");
    vi.advanceTimersByTime(1600);
    expect((await state(api))?.state).toBe("running");

    const missing = api.pauseChannel("nope");
    await expect(missing).rejects.toMatchObject({ code: "not_found", fieldErrors: [] });
    await expect(api.resumeChannel("ch-msg-worklist")).rejects.toMatchObject({ code: "conflict" });
    await expect(api.restartChannel("ch-msg-outreach")).rejects.toMatchObject({ code: "conflict" });
    await api.restartChannel("ch-br-d100");
    expect((await state(api, "ch-br-d100"))?.state).toBe("starting");
  });

  it("publishes a snapshot right after a command", async () => {
    vi.useFakeTimers();
    const api = create();
    const received: Snapshot[] = [];
    api.subscribeSnapshots((s) => received.push(s));
    await Promise.resolve();
    await api.pauseChannel(id);
    const last = received[received.length - 1];
    expect(last?.channels.find((c) => c.id === id)?.stopReason).toBe("paused");
  });
});

describe("details", () => {
  it("returns channel detail consistent with the configuration", async () => {
    const api = create();
    const config = await api.getConfig();
    const detail = await api.getChannel("ch-msg-au680");
    expect(detail.config).toEqual(config.channels.find((c) => c.id === "ch-msg-au680"));
    const channel = messageChannel(config, "ch-msg-au680");
    expect(detail.destinations).toHaveLength(channel.destinations.length);
    const snapshot = await api.getSnapshot();
    const queue = snapshot.channels.find((c) => c.id === "ch-msg-sta")?.queue;
    const sta = await api.getChannel("ch-msg-sta");
    const pending = sta.destinations.reduce((sum, d) => sum + d.queued + d.sending, 0);
    expect(pending).toBe(queue?.depth);
    const failed = sta.destinations.reduce(
      (sum, d) => sum + Object.values(d.unresolved).reduce((n, count) => n + count, 0),
      0,
    );
    expect(failed).toBe(queue?.needsAttention);
    expect(sta.destinations.some((d) => d.lastError !== null)).toBe(true);
    const bridge = await api.getChannel("ch-br-au5800");
    expect(bridge.destinations).toEqual([]);
    await expect(api.getChannel("nope")).rejects.toMatchObject({ code: "not_found" });
  });

  it("returns endpoint detail with its config and the channels that use it", async () => {
    const api = create();
    const detail = await api.getEndpoint("ep-lis-out");
    expect(detail.config.kind).toBe("tcp_client");
    expect(detail.snapshot.id).toBe("ep-lis-out");
    expect(detail.channels.length).toBeGreaterThan(5);
    expect(detail.channels.every((c) => c.role === "Destination")).toBe(true);
    expect(detail.snapshot.channelIds.sort()).toEqual(detail.channels.map((c) => c.id).sort());
    await expect(api.getEndpoint("nope")).rejects.toMatchObject({ code: "not_found" });
  });
});

describe("connection", () => {
  it("rejects everything with engine_unavailable until the service starts", async () => {
    const api = create({ offline: true });
    expect(await api.getConnection()).toMatchObject({
      state: "disconnected",
      endpoint: "mock://engine",
      detail: "No engine is running (demo data).",
    });
    const error = await api.getSnapshot().then(
      () => null,
      (e: unknown) => e,
    );
    expect(error).toMatchObject({ code: "engine_unavailable", fieldErrors: [] });
    expect((error as { correlationId: string }).correlationId).toMatch(/^c-[0-9a-f]{6}$/);
    await expect(api.listAlerts("open")).rejects.toMatchObject({ code: "engine_unavailable" });
    await expect(api.getConfig()).rejects.toMatchObject({ code: "engine_unavailable" });

    expect(await api.startService()).toBe("Started the demo engine.");
    expect(await api.getConnection()).toMatchObject({
      state: "connected",
      endpoint: "mock://engine",
    });
    expect(await api.startService()).toBe("The engine is already running.");
    expect((await api.getSnapshot()).channels).toHaveLength(36);
  });

  it("streams the connection state: now, on each change, and not after unsubscribing", async () => {
    const api = create({ offline: true });
    const states: string[] = [];
    const stop = api.subscribeConnection((status) => states.push(status.state));
    expect(states).toEqual(["disconnected"]);
    await api.startService();
    expect(states).toEqual(["disconnected", "connected"]);
    stop();
    stop();
    const other = create({ offline: true });
    const seen: string[] = [];
    other.subscribeConnection((status) => seen.push(status.state))();
    await other.startService();
    expect(seen).toEqual(["disconnected"]);
  });
});

describe("settings", () => {
  it("lists, sets and deletes secrets and reports engine info", async () => {
    const api = create();
    const secrets = await api.listSecrets();
    expect(secrets.length).toBeGreaterThanOrEqual(4);
    expect(secrets.some((s) => !s.hasValue)).toBe(true);
    const flagged = secrets.find((s) => s.name === "reflab-sftp-password");
    expect(flagged?.usedByEndpointIds).toEqual(["ep-reflab-sftp"]);

    await api.setSecret("clinic-sftp-key", "synthetic-value");
    expect((await api.listSecrets()).find((s) => s.name === "clinic-sftp-key")?.hasValue).toBe(
      true,
    );
    await api.setSecret("new-secret", "synthetic-value");
    expect((await api.listSecrets()).map((s) => s.name)).toContain("new-secret");
    await api.deleteSecret("new-secret");
    expect((await api.listSecrets()).map((s) => s.name)).not.toContain("new-secret");
    await expect(api.deleteSecret("new-secret")).rejects.toMatchObject({ code: "not_found" });
    await expect(api.setSecret("", "")).rejects.toMatchObject({ code: "validation" });

    expect((await api.getEngineSettings()).nodeId).toBe("lab-pc-01");
    expect((await api.getEngineInfo()).protocolVersion).toBe(1);
  });
});
