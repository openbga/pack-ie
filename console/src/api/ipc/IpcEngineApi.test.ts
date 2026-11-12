import { describe, expect, it } from "vitest";
import type { Snapshot } from "../types";
import { IDS } from "./test/configFixtures";
import { settle } from "./test/fakeBridge";
import { HL7_DESTINATION_STATUS } from "./test/hl7Destination";
import { CONNECTED, engine } from "./test/ipcEngine";
import { engineSnapshot } from "./test/snapshotFixtures";

describe("capabilities", () => {
  it("serves messages, alert history and secrets from the engine", () => {
    expect(engine().api.capabilities).toEqual({
      messages: "engine",
      secretsWritable: true,
      alertHistory: true,
    });
  });
});

describe("connection", () => {
  it("maps the connection payload", async () => {
    const { bridge, api } = engine();
    bridge.command("ipc_connection", () => ({
      ...CONNECTED,
      state: "reconnecting",
      error: "refused",
      attempt: 3,
    }));
    await expect(api.getConnection()).resolves.toEqual({
      state: "reconnecting",
      endpoint: "/run/packie.sock",
      detail: "refused",
      since: "2026-11-06T12:00:00Z",
      actor: "LAB\\jdoe",
    });
  });

  it("streams the state and its changes", async () => {
    const { bridge, api } = engine();
    const states: string[] = [];
    const stop = api.subscribeConnection((s) => states.push(`${s.state}:${s.detail}`));
    await settle(10);
    bridge.emit("packie://connection", { ...CONNECTED, state: "disconnected", error: "lost" });
    expect(states).toEqual(["connected:null", "disconnected:lost"]);
    stop();
  });

  it("starts the service and rejects with the command's own words", async () => {
    const { bridge, api } = engine();
    bridge.command("start_service", () => "Service started");
    await expect(api.startService()).resolves.toBe("Service started");
    bridge.command("start_service", () => {
      throw { kind: "internal", message: "starting the service is not implemented in this build" };
    });
    await expect(api.startService()).rejects.toEqual({
      code: "unavailable",
      message: "starting the service is not implemented in this build",
      fieldErrors: [],
      correlationId: "",
    });
  });
});

describe("snapshots", () => {
  it("maps a requested snapshot with the document of its version", async () => {
    const { bridge, api } = engine();
    const snapshot = await api.getSnapshot();
    // Pressure is measured against the document's backlog threshold (500).
    expect(snapshot.channels[1]?.pressure).toBeCloseTo(201 / 500);
    expect(snapshot.channels[0]?.buffers[0]?.capacityBytes).toBe(2_097_152);
    expect(snapshot.node.configVersion).toBe(7);
    // The document is cached for the next snapshot.
    await api.getSnapshot();
    expect(bridge.sent("get_config")).toHaveLength(1);
  });

  it("falls back to the default threshold when the document cannot be read", async () => {
    const { bridge, api } = engine();
    bridge.request("get_config", () => {
      throw { kind: "disconnected", message: "gone" };
    });
    const snapshot = await api.getSnapshot();
    expect(snapshot.channels[1]?.pressure).toBeCloseTo(201 / 1000);
  });

  it("does not ask for a document when the engine has no active version", async () => {
    const { bridge, api } = engine();
    bridge.request("get_snapshot", () => {
      const snapshot = engineSnapshot(7);
      snapshot.node.config = { active_version: null, pending_version: null, last_apply: null };
      return { type: "snapshot", snapshot };
    });
    const snapshot = await api.getSnapshot();
    expect(snapshot.node.configVersion).toBe(0);
    expect(bridge.sent("get_config")).toHaveLength(0);
  });

  it("streams snapshots, and emits again when the document arrives", async () => {
    const { bridge, api } = engine();
    const seen: Snapshot[] = [];
    const stop = api.subscribeSnapshots((s) => seen.push(s));
    await settle();
    expect(bridge.calls.find((c) => c.command === "ipc_subscribe")?.args.topic).toEqual({
      type: "snapshots",
    });
    bridge.emit("packie://event", {
      subscription: 1,
      event: { type: "snapshot", snapshot: engineSnapshot(7) },
    });
    expect(seen).toHaveLength(1);
    expect(seen[0]?.channels[1]?.pressure).toBeCloseTo(201 / 1000);
    await settle(10);
    expect(seen).toHaveLength(2);
    expect(seen[1]?.channels[1]?.pressure).toBeCloseTo(201 / 500);

    // Known version: no new fetch, one emit per snapshot.
    bridge.emit("packie://event", {
      subscription: 1,
      event: { type: "snapshot", snapshot: engineSnapshot(7) },
    });
    await settle(10);
    expect(seen).toHaveLength(3);
    expect(bridge.sent("get_config")).toHaveLength(1);

    stop();
    bridge.emit("packie://event", {
      subscription: 1,
      event: { type: "snapshot", snapshot: engineSnapshot(7) },
    });
    expect(seen).toHaveLength(3);
  });

  it("emits snapshots of an engine without an active version as they are", async () => {
    const { bridge, api } = engine();
    const seen: Snapshot[] = [];
    api.subscribeSnapshots((s) => seen.push(s));
    await settle();
    const snapshot = engineSnapshot(7);
    snapshot.node.config = { active_version: null, pending_version: null, last_apply: null };
    bridge.emit("packie://event", { subscription: 1, event: { type: "snapshot", snapshot } });
    await settle(10);
    expect(seen).toHaveLength(1);
    expect(bridge.sent("get_config")).toHaveLength(0);
  });

  it("asks for the document once while snapshots keep arriving", async () => {
    const { bridge, api } = engine();
    api.subscribeSnapshots(() => undefined);
    await settle();
    for (let i = 0; i < 3; i++) {
      bridge.emit("packie://event", {
        subscription: 1,
        event: { type: "snapshot", snapshot: engineSnapshot(7) },
      });
    }
    await settle(10);
    expect(bridge.sent("get_config")).toHaveLength(1);
  });
});

describe("channel and endpoint detail", () => {
  it("combines the snapshot and the document, with destinations for message channels", async () => {
    const { api } = engine();
    const bridgeDetail = await api.getChannel(IDS.bridge);
    expect(bridgeDetail.config).toMatchObject({
      kind: "bridge",
      id: IDS.bridge,
      pairing: "receiver_then_sender",
    });
    expect(bridgeDetail.destinations).toEqual([]);

    const detail = await api.getChannel(IDS.message);
    expect(detail.snapshot.queue?.depth).toBe(201);
    expect(detail.config).toMatchObject({ kind: "message", label: "Chemistry results" });
    expect(detail.destinations).toEqual([
      HL7_DESTINATION_STATUS,
      expect.objectContaining({
        destinationId: IDS.csv,
        protocol: "delimited_file",
        enabled: false,
      }),
    ]);
  });

  it("lists the channels an endpoint serves with its role in each", async () => {
    const { api } = engine();
    const detail = await api.getEndpoint(IDS.client);
    expect(detail.config).toMatchObject({ kind: "tcp_client", host: "lis.example" });
    expect(detail.channels).toEqual([
      { id: IDS.bridge, label: "Analyzer to LIS", role: "B" },
      { id: IDS.message, label: "Chemistry results", role: "destination" },
      { id: IDS.au, label: "AU results", role: "destination" },
    ]);
    expect((await api.getEndpoint(IDS.listener)).channels).toEqual([
      { id: IDS.message, label: "Chemistry results", role: "source" },
    ]);
    expect((await api.getEndpoint(IDS.serial)).channels).toEqual([
      { id: IDS.bridge, label: "Analyzer to LIS", role: "A" },
    ]);
  });

  it("rejects unknown ids with not_found", async () => {
    const { api } = engine();
    await expect(api.getChannel("nope")).rejects.toMatchObject({ code: "not_found" });
    await expect(api.getEndpoint("nope")).rejects.toMatchObject({ code: "not_found" });
  });
});

describe("commands", () => {
  it.each([
    ["pauseChannel", "pause"],
    ["resumeChannel", "resume"],
    ["restartChannel", "restart"],
  ] as const)("%s sends %s", async (method, command) => {
    const { bridge, api } = engine();
    bridge.request("channel_command", () => ({ type: "done" }));
    await api[method](IDS.bridge);
    expect(bridge.sent("channel_command")).toEqual([
      { type: "channel_command", channel_id: IDS.bridge, command },
    ]);
  });

  it("rejects with the engine's refusal", async () => {
    const { bridge, api } = engine();
    bridge.request("channel_command", () => {
      throw {
        kind: "ipc",
        error: {
          code: "rejected",
          message: "already stopped",
          field_errors: [],
          reason_code: "already_stopped",
        },
      };
    });
    await expect(api.pauseChannel(IDS.bridge)).rejects.toMatchObject({
      code: "conflict",
      message: "already stopped",
    });
  });
});
