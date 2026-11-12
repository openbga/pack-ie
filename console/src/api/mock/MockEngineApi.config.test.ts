import { describe, expect, it, vi } from "vitest";
import { documentToWire } from "@/api/ipc/configDocument";
import { at, mockApiFactory, rejection } from "./mockTestSupport";

const create = mockApiFactory({ messageCount: 500, seed: 7 });

describe("validateConfig", () => {
  it("accepts the active document without errors", async () => {
    const api = create();
    const result = await api.validateConfig(await api.getConfig());
    expect(result.ok).toBe(true);
    expect(at(result.issues, "error")).toEqual([]);
  });

  it("serializes the seed document with the production serializer, without problems", async () => {
    const api = create();
    expect(documentToWire(await api.getConfig()).issues).toEqual([]);
  });

  it("reports errors with engine paths", async () => {
    const api = create();
    const doc = await api.getConfig();
    doc.endpoints[1] = { ...doc.endpoints[1], id: doc.endpoints[0]?.id ?? "" } as never;
    const tcp = doc.endpoints.findIndex((e) => e.kind === "tcp_client");
    doc.endpoints[tcp] = { ...doc.endpoints[tcp], port: 70_000, label: " " } as never;
    const bridge = doc.channels.findIndex((c) => c.kind === "bridge");
    const bridgeConfig = doc.channels[bridge];
    if (bridgeConfig?.kind !== "bridge") throw new Error("expected a bridge");
    doc.channels[bridge] = { ...bridgeConfig, endpointBId: bridgeConfig.endpointAId };
    const message = doc.channels.findIndex((c) => c.kind === "message");
    const messageConfig = doc.channels[message];
    if (messageConfig?.kind !== "message") throw new Error("expected a message channel");
    doc.channels[message] = {
      ...messageConfig,
      source: { ...messageConfig.source, endpointId: "ep-missing" },
      destinations: [],
    };
    const result = await api.validateConfig(doc);
    expect(result.ok).toBe(false);
    const errors = at(result.issues, "error");
    expect(errors).toContain("endpoints[1].id");
    expect(errors).toContain(`endpoints[${tcp}].port`);
    expect(errors).toContain(`endpoints[${tcp}].label`);
    expect(errors).toContain(`channels[${bridge}].endpointBId`);
    expect(errors).toContain(`channels[${message}].source.endpointId`);
    expect(errors).toContain(`channels[${message}].destinations`);
  });

  it("reports warnings that do not block apply", async () => {
    const api = create();
    const doc = await api.getConfig();
    const listener = doc.endpoints.findIndex((e) => e.kind === "tcp_listener");
    doc.endpoints[listener] = {
      ...doc.endpoints[listener],
      allowlist: [],
      allowAnyPeer: true,
    } as never;
    const lisOut = doc.endpoints.findIndex((e) => e.id === "ep-lis-out");
    doc.endpoints[lisOut] = { ...doc.endpoints[lisOut], enabled: false } as never;
    const au680 = doc.channels.findIndex((c) => c.id === "ch-msg-au680");
    const channel = doc.channels[au680];
    if (channel?.kind !== "message") throw new Error("expected a message channel");
    const [first] = channel.destinations;
    if (first === undefined) throw new Error("expected a destination");
    const twice = first.mappings.map((m) => ("to" in m ? { ...m, to: "X.1" } : m));
    doc.channels[au680] = {
      ...channel,
      destinations: [{ ...first, mappings: twice }, ...channel.destinations.slice(1)],
    };
    const result = await api.validateConfig(doc);
    expect(result.ok).toBe(true);
    const warnings = at(result.issues, "warning");
    expect(warnings).toContain(`endpoints[${listener}].allowAnyPeer`);
    expect(warnings).toContain(`channels[${au680}].destinations[0].endpointId`);
    expect(warnings).toContain(`channels[${au680}].destinations[0].mappings[1].to`);
  });
});

describe("applyConfig", () => {
  it("rejects an invalid document with field errors and applies nothing", async () => {
    const api = create();
    const doc = await api.getConfig();
    const before = await api.listConfigVersions();
    const index = doc.endpoints.findIndex((e) => e.kind === "tcp_client");
    doc.endpoints[index] = { ...doc.endpoints[index], port: 0 } as never;
    const error = await rejection(api.applyConfig(doc, "bad port"));
    expect(error.code).toBe("validation");
    expect(error.fieldErrors.map((f) => f.field)).toContain(`endpoints[${index}].port`);
    expect(error.correlationId).toMatch(/^c-/);
    expect(await api.listConfigVersions()).toEqual(before);
    expect((await api.getConfig()).version).toBe(doc.version);
  });

  it("applies a valid edit: bumps the version, restarts only the changed channel", async () => {
    vi.useFakeTimers();
    const api = create();
    const doc = await api.getConfig();
    const index = doc.channels.findIndex((c) => c.id === "ch-msg-au680");
    const channel = doc.channels[index];
    if (channel?.kind !== "message") throw new Error("expected a message channel");
    doc.channels[index] = { ...channel, source: { ...channel.source, duplicateDetection: false } };
    const result = await api.applyConfig(doc, "stop detecting duplicates");
    expect(result.version).toBe(doc.version + 1);
    expect(result.restartedChannelIds).toEqual(["ch-msg-au680"]);
    expect(result.unchangedChannelCount).toBe(35);
    expect(result.outcomes).toHaveLength(36);
    expect(result.outcomes.find((o) => o.channelId === "ch-msg-au680")).toEqual({
      channelId: "ch-msg-au680",
      outcome: "restarted",
    });

    const snapshot = await api.getSnapshot();
    const running = snapshot.channels.find((c) => c.id === "ch-msg-au680");
    expect(running?.state).toBe("starting");
    expect(snapshot.node.configVersion).toBe(result.version);
    vi.advanceTimersByTime(1600);
    expect((await api.getSnapshot()).channels.find((c) => c.id === "ch-msg-au680")?.state).toBe(
      "running",
    );

    const versions = await api.listConfigVersions();
    expect(versions[0]).toMatchObject({
      version: result.version,
      appliedBy: "LAB\\jdoe",
      note: "stop detecting duplicates",
      verifications: [],
      outcomes: result.outcomes,
      current: true,
    });
    expect(versions.filter((v) => v.current)).toHaveLength(1);
    expect((await api.getConfig()).version).toBe(result.version);
  });

  it("takes a label or retention change in place, without a restart", async () => {
    const api = create();
    const doc = await api.getConfig();
    const index = doc.channels.findIndex((c) => c.id === "ch-msg-au680");
    const channel = doc.channels[index];
    if (channel?.kind !== "message") throw new Error("expected a message channel");
    doc.channels[index] = {
      ...channel,
      label: "AU680 results (renamed)",
      retention: { ...channel.retention, days: 9 },
    };
    const plan = (await api.validateConfig(doc)).plan;
    expect(plan?.channels.filter((c) => c.change !== "unchanged")).toEqual([
      { id: "ch-msg-au680", label: "AU680 results (renamed)", change: "updated" },
    ]);
    const result = await api.applyConfig(doc, "rename and shorten retention");
    expect(result.restartedChannelIds).toEqual([]);
    expect(result.outcomes.filter((o) => o.outcome !== "unchanged")).toEqual([
      { channelId: "ch-msg-au680", outcome: "updated" },
    ]);
    expect(result.unchangedChannelCount).toBe(35);
    const snapshot = await api.getSnapshot();
    expect(snapshot.channels.find((c) => c.id === "ch-msg-au680")?.label).toBe(
      "AU680 results (renamed)",
    );
  });

  it("stops, rather than restarts or starts, a channel the new document disables", async () => {
    const api = create();
    const doc = await api.getConfig();
    const bridge = doc.channels.find((c) => c.kind === "bridge");
    if (bridge?.kind !== "bridge") throw new Error("expected a bridge");
    doc.channels = doc.channels.map((c) =>
      c.id === bridge.id ? { ...bridge, enabled: false } : c,
    );
    const plan = (await api.validateConfig(doc)).plan;
    expect(plan.channels.find((c) => c.id === bridge.id)?.change).toBe("stopped");
    const result = await api.applyConfig(doc, "disable the bridge");
    expect(result.outcomes.find((o) => o.channelId === bridge.id)?.outcome).toBe("stopped");
    expect(result.restartedChannelIds).toEqual([]);
  });

  it("answers a document with errors with an empty plan", async () => {
    const api = create();
    const doc = await api.getConfig();
    const channel = doc.channels[0];
    if (channel === undefined) throw new Error("expected a channel");
    doc.channels = [{ ...channel, id: "" }, ...doc.channels.slice(1)];
    const result = await api.validateConfig(doc);
    expect(result.ok).toBe(false);
    expect(result.plan).toEqual({ channels: [], endpoints: [], alertsChanged: false });
  });

  it("restarts for anything beyond the in-place settings, including a bridge's buffers", async () => {
    const api = create();
    const doc = await api.getConfig();
    const bridge = doc.channels.find((c) => c.kind === "bridge");
    if (bridge?.kind !== "bridge") throw new Error("expected a bridge");
    doc.channels = doc.channels.map((c) =>
      c.id === bridge.id ? { ...bridge, label: "Renamed", bufferBytes: bridge.bufferBytes * 2 } : c,
    );
    const plan = (await api.validateConfig(doc)).plan;
    expect(plan?.channels.find((c) => c.id === bridge.id)?.change).toBe("changed");
  });

  it("ends partial when a channel fails: the new version is active, the channel flagged", async () => {
    const api = create();
    const doc = await api.getConfig();
    const index = doc.channels.findIndex((c) => c.id === "ch-br-au5800");
    const bridge = doc.channels[index];
    if (bridge?.kind !== "bridge") throw new Error("expected a bridge");
    doc.channels[index] = { ...bridge, bufferBytes: bridge.bufferBytes * 2 };
    api.failNextApply("ch-br-au5800", "Cannot bind COM3: access denied");
    const result = await api.applyConfig(doc, "bigger buffer");
    expect(result.failures).toEqual([
      { channelId: "ch-br-au5800", reason: "Cannot bind COM3: access denied" },
    ]);
    expect(result.restartedChannelIds).toEqual([]);
    expect(result.outcomes.find((o) => o.channelId === "ch-br-au5800")).toEqual({
      channelId: "ch-br-au5800",
      outcome: "failed",
      reason: "Cannot bind COM3: access denied",
    });
    expect((await api.getConfig()).version).toBe(result.version);
    expect((await api.listConfigVersions())[0]?.outcomes).toEqual(result.outcomes);
    // One apply only: the next one is whole again.
    const again = await api.applyConfig(await api.getConfig(), "again");
    expect(again.failures).toEqual([]);
  });

  it("adds and removes channels and endpoints and restarts channels of an edited endpoint", async () => {
    const api = create();
    const doc = await api.getConfig();
    const lisOut = doc.endpoints.findIndex((e) => e.id === "ep-lis-out");
    doc.endpoints[lisOut] = { ...doc.endpoints[lisOut], port: 2599 } as never;
    doc.endpoints.push({
      kind: "folder",
      id: "ep-new",
      label: "New export folder",
      enabled: true,
      path: "D:\\Export",
      mode: "watch",
      pollIntervalSec: 5,
      filePattern: "*.csv",
    });
    doc.channels = doc.channels.filter((c) => c.id !== "ch-br-d100");
    const result = await api.applyConfig(doc, "move the LIS port");
    expect(result.restartedChannelIds).toContain("ch-msg-au680");
    expect(result.restartedChannelIds).not.toContain("ch-br-au5800");
    const users = (await api.getEndpoint("ep-lis-out")).channels.map((c) => c.id).sort();
    expect([...result.restartedChannelIds].sort()).toEqual(users);

    const snapshot = await api.getSnapshot();
    expect(snapshot.endpoints.some((e) => e.id === "ep-new")).toBe(true);
    expect(snapshot.channels.some((c) => c.id === "ch-br-d100")).toBe(false);
    expect(snapshot.channels).toHaveLength(35);
    const latest = (await api.listConfigVersions())[0];
    expect(latest).toMatchObject({ note: "move the LIS port", channels: 35 });
    await expect(api.getChannel("ch-br-d100")).rejects.toMatchObject({ code: "not_found" });
  });

  it("rejects a stale document with conflict", async () => {
    const api = create();
    const doc = await api.getConfig();
    await api.applyConfig(doc, "first");
    const error = await rejection(api.applyConfig(doc, "second"));
    expect(error.code).toBe("conflict");
  });

  it("clears the stop of a channel whose configuration error was fixed", async () => {
    const api = create();
    const doc = await api.getConfig();
    const index = doc.endpoints.findIndex((e) => e.id === "ep-outreach-in");
    doc.endpoints[index] = { ...doc.endpoints[index], port: 2591 } as never;
    await api.applyConfig(doc, "move the listener");
    const channel = (await api.getSnapshot()).channels.find((c) => c.id === "ch-msg-outreach");
    expect(channel?.state).not.toBe("stopped");
  });
});
