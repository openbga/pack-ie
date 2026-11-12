// Opt-in check of the console's mapping layer against a RUNNING engine. Not part of the normal
// suite: it runs only when PACKIE_LIVE_IPC names the engine's socket, for example
//
//   cargo run -p packie -- run --bootstrap config/bootstrap.json     (in the repository root)
//   PACKIE_LIVE_IPC=$PWD/../state/packie.sock npx vitest run src/api/ipc/live.test.tsx   (in console/)
//
// Any engine works. When it has a bridge `bridge-1` ("Instrument to system") with a listener on
// side A and a client on side B, and two peers that keep bytes moving (the instrument sends
// `ping <n>` every 500 ms and the system answers `pong <n>`), the bridge tests run too; without
// it they are skipped and the generic tests act on the first running channel (for example the
// two message channels of tests/scenarios/msg-hl7-in-ack.json). The Messages screens have their
// own suite, `liveMessages.test.tsx`. A small bridge (test/liveHarness.ts) plays the part of the
// Tauri shell (src-tauri) over the real socket, so everything above it, `IpcEngineApi` and the
// pages, is the production code. Every test leaves the engine as it found it: the channel
// running, its label restored (a relabel adds two configuration versions), no secret written.
// The tests of the configuration (test/liveConfigSuite.ts) and the read-only ones
// (test/liveReadSuite.tsx) are registered below, after these.
import { screen, waitFor, within } from "@testing-library/react";
import { beforeAll, describe, expect, it } from "vitest";
import { ChannelPage } from "@/features/channels/ChannelPage";
import { DashboardPage } from "@/features/dashboard/DashboardPage";
import { mockGridLayout } from "@/lib/test/layout";
import { renderRoutes } from "@/lib/test/router";
import { configTests } from "./test/liveConfigSuite";
import {
  BRIDGE,
  BRIDGE_LABEL,
  bridgeChannel,
  connect,
  leaveRunning,
  SOCKET,
  target,
  until,
} from "./test/liveHarness";
import { readTests } from "./test/liveReadSuite";

const decode = (base64: string) => atob(base64);

describe.skipIf(!SOCKET)("against a running engine", () => {
  beforeAll(async () => {
    const { api, close } = await connect();
    try {
      const { channels } = await api.getSnapshot();
      const bridge = channels.find((c) => c.id === BRIDGE);
      if (bridge !== undefined) return;
      const first = channels.find((c) => c.enabled && c.state === "running") ?? channels[0];
      if (first === undefined) throw new Error("the engine has no channels to test against");
      Object.assign(target, { id: first.id, label: first.label, hasBridge: false });
    } finally {
      close();
    }
  });

  it("reads the node, its store figures and its endpoints in the console's model", async () => {
    const { api, close } = await connect();
    try {
      const snapshot = await api.getSnapshot();
      expect(["console", "service"]).toContain(snapshot.node.hosting);
      expect(snapshot.node.nodeName.length).toBeGreaterThan(0);
      expect(snapshot.node.configVersion).toBeGreaterThan(0);
      expect(snapshot.node.totals).not.toBeNull();
      // The store is what this engine says it is: unavailable, not configured, or ok with figures.
      const store = snapshot.node.store;
      expect(store).not.toBeNull();
      expect(["ok", "unavailable", "not_configured"]).toContain(store?.state);
      if (store?.state === "ok") {
        expect(store.messageCount).toEqual(expect.any(Number));
        expect(store.databaseBytes).toEqual(expect.any(Number));
        expect(store.diskFreeBytes).toEqual(expect.any(Number));
      }
      if (store?.diskTotalBytes != null) {
        expect(store.diskTotalBytes).toBeGreaterThanOrEqual(store.diskFreeBytes ?? 0);
      }
      expect(snapshot.channels.length).toBeGreaterThan(0);
      expect(snapshot.endpoints.length).toBeGreaterThan(0);
      for (const endpoint of snapshot.endpoints) expect(endpoint.summary.length).toBeGreaterThan(0);
      for (const channel of snapshot.channels) {
        expect(channel.endpoints.length).toBeGreaterThan(0);
        expect(channel.allowedActions).not.toBeNull();
      }
      const detail = await api.getChannel(target.id);
      expect(detail.config.id).toBe(target.id);
      const endpoint = await api.getEndpoint(snapshot.endpoints[0]?.id ?? "");
      expect(endpoint.channels.length).toBeGreaterThan(0);
    } finally {
      close();
    }
  });
  it("reads the bridge and its endpoints in the console's model", async ({ skip }) => {
    if (!target.hasBridge) return skip();
    const { api, close } = await connect();
    try {
      const channel = await until(
        `${BRIDGE} running with traffic`,
        () => bridgeChannel(api),
        (c) => c.state === "running" && c.counters.bytesIn > 0 && c.counters.bytesOut > 0,
      );
      expect(channel).toMatchObject({
        id: BRIDGE,
        label: BRIDGE_LABEL,
        kind: "bridge",
        enabled: true,
        stopReason: null,
        protocol: null,
        bridge: { sessionState: "bridged", connectStrategy: "parallel" },
        queue: null,
        source: null,
      });
      expect(channel.allowedActions).toContain("pause");
      expect(channel.allowedActions).not.toContain("resume");
      expect(channel.counters.retries).toBeGreaterThanOrEqual(0);
      expect(channel.bridge?.bytesAToB).toBeGreaterThan(0);
      expect(channel.bridge?.bytesBToA).toBeGreaterThan(0);
      expect(channel.buffers.map((b) => b.direction)).toEqual(["a_to_b", "b_to_a"]);
      expect(channel.buffers[0]?.capacityBytes).toBeGreaterThan(0);
      expect(channel.endpoints).toHaveLength(2);
      expect(channel.configVersion).toBeGreaterThan(0);
      expect(channel.instanceStartedAt).not.toBeNull();
      expect(channel.lastActivityAt).not.toBeNull();

      const snapshot = await api.getSnapshot();
      // Other channels may share the engine: only the bridge's two endpoints are checked.
      const ofBridge = snapshot.endpoints.filter((e) => e.channelIds.includes(BRIDGE));
      expect(ofBridge.length).toBeGreaterThanOrEqual(2);
      for (const endpoint of snapshot.endpoints) {
        expect(endpoint.summary.length).toBeGreaterThan(0);
      }
      expect(ofBridge.some((e) => e.heldBy === BRIDGE)).toBe(true);
      expect(ofBridge.some((e) => e.activePeer !== null)).toBe(true);

      const detail = await api.getChannel(BRIDGE);
      expect(detail.config).toMatchObject({ kind: "bridge", id: BRIDGE, label: BRIDGE_LABEL });
      expect(detail.destinations).toEqual([]);
      const endpoint = await api.getEndpoint(snapshot.endpoints[0]?.id ?? "");
      expect(endpoint.channels.map((c) => c.id)).toContain(BRIDGE);
    } finally {
      close();
    }
  });

  it("shows the bridge as running with bytes both ways on the dashboard and the channel page", async ({
    skip,
  }) => {
    if (!target.hasBridge) return skip();
    const { api, close } = await connect();
    try {
      await until(
        `${BRIDGE} running with traffic`,
        () => bridgeChannel(api),
        (c) => c.state === "running" && c.counters.bytesIn > 0 && c.counters.bytesOut > 0,
      );
      mockGridLayout();
      const dashboard = renderRoutes(
        [{ path: "/dashboard", element: <DashboardPage /> }],
        "/dashboard",
        {
          api,
        },
      );
      const grid = await screen.findByRole("grid", { name: "Channels" }, { timeout: 8000 });
      let rowText = "";
      await waitFor(
        () => {
          const row = within(grid)
            .getAllByRole("row")
            .find((each) => (each.textContent ?? "").includes(BRIDGE_LABEL));
          expect(row).toBeDefined();
          rowText = row?.textContent ?? "";
          expect(rowText).toContain("Running");
          // The traffic column: bytes in and bytes out, neither of them empty.
          expect(rowText).not.toMatch(/\b0 B\b/);
          expect(rowText).toMatch(/[0-9.]+ [KMG]?B/);
        },
        { timeout: 8000 },
      );
      // The strip counts the engine's running channels, whatever else it runs.
      const running = (await api.getSnapshot()).channels.filter((c) => c.state === "running");
      const chips = await screen.findByRole("list", { name: "Channels by state" });
      expect(within(chips).getByText(/Running/).textContent).toBe(`${running.length} Running`);
      dashboard.unmount();

      renderRoutes([{ path: "/channels/:id", element: <ChannelPage /> }], `/channels/${BRIDGE}`, {
        api,
      });
      await screen.findByRole(
        "heading",
        { level: 1, name: new RegExp(BRIDGE_LABEL) },
        { timeout: 8000 },
      );
      expect(await screen.findAllByText("Running", {}, { timeout: 8000 })).not.toHaveLength(0);
      const traffic = await screen.findByRole("table", { name: "Bytes by direction" });
      await waitFor(
        () => {
          const rows = within(traffic).getAllByRole("row").slice(1);
          expect(rows).toHaveLength(2);
          for (const row of rows) {
            const bytes = within(row).getAllByRole("cell")[1]?.textContent ?? "";
            expect(bytes).toMatch(/^[0-9.]+ [KMG]?B$/);
            expect(bytes).not.toBe("0 B");
          }
        },
        { timeout: 8000 },
      );
    } finally {
      close();
    }
  });

  it("delivers snapshots with growing counters", async ({ skip }) => {
    if (!target.hasBridge) return skip();
    const { api, close } = await connect();
    const seen: { at: string; bytesIn: number; bytesOut: number; state: string }[] = [];
    const stop = api.subscribeSnapshots((snapshot) => {
      const channel = snapshot.channels.find((c) => c.id === BRIDGE);
      if (channel)
        seen.push({
          at: snapshot.takenAt,
          bytesIn: channel.counters.bytesIn,
          bytesOut: channel.counters.bytesOut,
          state: channel.state,
        });
    });
    try {
      await until(
        "three snapshots",
        async () => seen,
        (list) => list.length >= 3,
        10_000,
      );
      await until(
        "counters that grew",
        async () => seen,
        (list) => {
          const first = list[0];
          const last = list.at(-1);
          return (
            !!first && !!last && last.bytesIn > first.bytesIn && last.bytesOut > first.bytesOut
          );
        },
        10_000,
      );
      const times = seen.map((s) => Date.parse(s.at));
      expect(times).toEqual([...times].sort((a, b) => a - b));
      const first = seen[0];
      const last = seen.at(-1);
      expect(first && last && last.bytesIn > first.bytesIn).toBe(true);
    } finally {
      stop();
      close();
    }
  });

  it("streams the tap: ping one way, pong the other", async ({ skip }) => {
    if (!target.hasBridge) return skip();
    const { api, close } = await connect();
    const segments: { direction: string; text: string }[] = [];
    const errors: string[] = [];
    const stop = api.subscribeTraffic(
      BRIDGE,
      (batch) =>
        segments.push(...batch.map((s) => ({ direction: s.direction, text: decode(s.base64) }))),
      (error) => errors.push(error.message),
    );
    try {
      await until(
        "ping and pong",
        async () => segments,
        (list) =>
          errors.length > 0 ||
          (list.some((s) => s.direction === "a_to_b" && s.text.includes("ping")) &&
            list.some((s) => s.direction === "b_to_a" && s.text.includes("pong"))),
        15_000,
      );
      expect(errors).toEqual([]);
      expect(
        segments.filter((s) => s.text.includes("ping")).every((s) => s.direction === "a_to_b"),
      ).toBe(true);
      expect(
        segments.filter((s) => s.text.includes("pong")).every((s) => s.direction === "b_to_a"),
      ).toBe(true);
      // The tap counts as a viewer while it is open.
      expect((await bridgeChannel(api)).tapActive).toBe(true);
    } finally {
      stop();
      // Closing the transcript is asynchronous; the engine reports no viewer shortly after.
      await until(
        "the tap to close",
        () => bridgeChannel(api),
        (c) => !c.tapActive,
        10_000,
      ).catch(() => undefined);
      close();
    }
  });

  it("pauses and resumes the channel, and leaves it as found", async () => {
    const { api, close } = await connect();
    const followed: string[] = [];
    const stop = api.followLogs({ channelId: target.id }, (events) =>
      followed.push(...events.map((e) => e.message)),
    );
    try {
      await leaveRunning(api);
      await api.pauseChannel(target.id);
      const paused = await until(
        `${target.id} paused`,
        () => bridgeChannel(api),
        (c) => c.state === "stopped",
      );
      expect(paused.stopReason).toBe("paused");
      expect(paused.allowedActions).toContain("resume");
      expect(paused.allowedActions).not.toContain("pause");
      await expect(api.pauseChannel(target.id)).rejects.toMatchObject({ code: "conflict" });

      await api.resumeChannel(target.id);
      const running = await until(
        `${target.id} running again`,
        () => bridgeChannel(api),
        (c) => c.state !== "stopped",
      );
      expect(running.stopReason).toBeNull();
      expect(running.allowedActions).toContain("pause");
      await until(
        "log lines about the channel",
        async () => followed,
        (list) => list.length > 0,
        10_000,
      );
    } finally {
      stop();
      await leaveRunning(api);
      close();
    }
  }, 90_000);

  configTests();
  readTests();
});
