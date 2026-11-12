import { describe, expect, it } from "vitest";
import type { AuSettings, MessageChannelConfig } from "../types";
import {
  at,
  check,
  messageChannelAt,
  messageIndex,
  mockApiFactory,
  patchChannel,
  patchDestination,
} from "./mockTestSupport";

const create = mockApiFactory({ messageCount: 500, seed: 7 });

describe("connector rules", () => {
  async function patchedSource(id: string, patch: Partial<MessageChannelConfig["source"]>) {
    const api = create();
    const doc = await api.getConfig();
    const index = messageIndex(doc, id);
    const channel = messageChannelAt(doc, index);
    patchChannel(doc, index, { source: { ...channel.source, ...patch } });
    return { issues: await check(doc, api), index };
  }

  it.each(["O.2", "O.04", "O.3.0", "O.3.100", "O.100", "P.3", "O.3.1.1", ""])(
    "rejects the sample id path %j of an ASTM source at its field",
    async (path) => {
      const { issues, index } = await patchedSource("ch-msg-xn1000", {
        settings: { astmRole: "computer", sampleIdPath: path },
      });
      expect(at(issues, "error")).toContain(`channels[${index}].source.settings.sampleIdPath`);
    },
  );

  it.each(["O.3", "O.4", "O.4.3", "O.99.99"])("accepts the sample id path %s", async (path) => {
    const { issues, index } = await patchedSource("ch-msg-xn1000", {
      settings: { astmRole: "computer", sampleIdPath: path },
    });
    expect(at(issues, "error")).not.toContain(`channels[${index}].source.settings.sampleIdPath`);
  });

  it.each([
    ["astm", "ep-qc-in"],
    ["au", "ep-orders-sftp"],
    ["hl7_mllp", "ep-c311"],
    ["delimited_file", "ep-xn1000"],
    ["raw", "ep-xn1000"],
  ] as const)(
    "rejects %s over the endpoint %s, whose kind does not carry it",
    async (protocol, endpoint) => {
      const { issues, index } = await patchedSource("ch-msg-xn1000", {
        protocol,
        endpointId: endpoint,
        settings: {},
      });
      expect(at(issues, "error")).toContain(`channels[${index}].source.protocol`);
    },
  );

  it("accepts each protocol over the endpoint kinds the spec lists", async () => {
    const { issues } = await patchedSource("ch-msg-xn1000", { endpointId: "ep-c311" });
    expect(at(issues, "error")).toEqual([]);
  });

  it("rejects a source that expects no acknowledgement, but not a destination", async () => {
    const { issues, index } = await patchedSource("ch-msg-billing", {
      settings: { ackMode: "none" },
    });
    expect(at(issues, "error")).toEqual([`channels[${index}].source.settings.ackMode`]);

    const api = create();
    const doc = await api.getConfig();
    const outreach = messageIndex(doc, "ch-msg-outreach");
    patchDestination(doc, outreach, { settings: { ackMode: "none" } });
    expect(at(await check(doc, api), "error")).toEqual([]);
  });

  it("checks the ASTM link role", async () => {
    const { issues, index } = await patchedSource("ch-msg-xn1000", {
      settings: { astmRole: "boss" as never },
    });
    expect(at(issues, "error")).toEqual([`channels[${index}].source.settings.astmRole`]);
  });

  it("detects duplicates on every channel but one demo channel", async () => {
    const doc = await create().getConfig();
    const without = doc.channels.filter(
      (c) => c.kind === "message" && !c.source.duplicateDetection,
    );
    expect(without.map((c) => c.id)).toEqual(["ch-msg-qc"]);
  });
});

describe("AU connector settings", () => {
  async function auIssues(patch: Partial<AuSettings> | undefined) {
    const api = create();
    const doc = await api.getConfig();
    const index = messageIndex(doc, "ch-msg-au680");
    const channel = messageChannelAt(doc, index);
    const settings = patch === undefined ? {} : { au: { ...channel.source.settings.au, ...patch } };
    patchChannel(doc, index, { source: { ...channel.source, settings } });
    return { issues: await check(doc, api), base: `channels[${index}].source.settings.au` };
  }

  it("accepts the demo AU680's complete settings", async () => {
    expect(at((await auIssues({})).issues, "error")).toEqual([]);
  });

  it("requires the settings at all", async () => {
    const { issues, base } = await auIssues(undefined);
    expect(at(issues, "error")).toEqual([base]);
  });

  it.each([
    ["profile", { profile: "au5800" as never }],
    ["framingStart", { framingStart: [] }],
    ["framingStart", { framingStart: [1, 2, 3] }],
    ["framingStart", { framingStart: [0] }],
    ["framingStart", { framingStart: [32] }],
    ["framingStart", { framingStart: [6] }],
    ["framingEnd", { framingEnd: [21] }],
    ["framingEnd", { framingEnd: [2.5] }],
    ["useEtb", { useEtb: true, framingEnd: [13, 10] }],
    ["maxWireLen", { maxWireLen: 300 as never }],
    ["framingClass", { framingClass: "c" as never }],
    ["rackDigits", { rackDigits: 3 as never }],
    ["sampleIdDigits", { sampleIdDigits: 0 }],
    ["sampleIdDigits", { sampleIdDigits: 21 }],
    ["patientInfoDigits", { patientInfoDigits: [1, 2, 3] }],
    ["patientInfoDigits[2]", { patientInfoDigits: [0, 0, 21, 0, 0, 0] }],
    ["patientInfoDigits[5]", { patientInfoDigits: [0, 0, 0, 0, 0, -1] }],
    ["resultDigits", { resultDigits: 7 as never }],
    ["frameTimeoutMs", { frameTimeoutMs: 50 }],
    ["frameTimeoutMs", { frameTimeoutMs: 10_000 }],
    ["blockTimeoutMs", { blockTimeoutMs: 1250 }],
  ])("reports a bad %s on its own field", async (field, patch) => {
    const { issues, base } = await auIssues(patch);
    expect(at(issues, "error")).toEqual([`${base}.${field}`]);
  });

  it("allows ETB when the end is a single ETX", async () => {
    expect(at((await auIssues({ useEtb: true, framingEnd: [3] })).issues, "error")).toEqual([]);
  });
});

describe("alert thresholds", () => {
  it("validates all four limits", async () => {
    const api = create();
    const doc = await api.getConfig();
    doc.alertThresholds = {
      endpointDownSeconds: 0,
      queueDepthWarning: -1,
      bufferPressurePercent: 101,
      diskFreeMinMib: 0,
    };
    expect(at(await check(doc, api), "error")).toEqual([
      "alertThresholds.endpointDownSeconds",
      "alertThresholds.queueDepthWarning",
      "alertThresholds.bufferPressurePercent",
      "alertThresholds.diskFreeMinMib",
    ]);
  });

  it("starts with the engine defaults", async () => {
    expect((await create().getConfig()).alertThresholds).toEqual({
      endpointDownSeconds: 60,
      queueDepthWarning: 1000,
      bufferPressurePercent: 80,
      diskFreeMinMib: 1024,
    });
  });

  it("opens disk and buffer alerts from the new limits", async () => {
    const api = create();
    const doc = await api.getConfig();
    expect((await api.listAlerts("open")).map((a) => a.kind)).not.toContain("disk_low");
    doc.alertThresholds = {
      ...doc.alertThresholds,
      diskFreeMinMib: 1_000_000,
      bufferPressurePercent: 1,
    };
    await api.applyConfig(doc, "tighten limits");
    const kinds = (await api.listAlerts("open")).map((a) => a.kind);
    expect(kinds).toContain("disk_low");
    expect(kinds).toContain("bridge_buffer_pressure");
  });
});

describe("a bridge tap's retention", () => {
  const withTapRetention = async (tapRetention: {
    days: number | null;
    maxMessages: number | null;
  }) => {
    const api = create();
    const doc = await api.getConfig();
    const index = doc.channels.findIndex((c) => c.kind === "bridge");
    const bridge = doc.channels[index];
    if (bridge?.kind !== "bridge") throw new Error("expected a bridge");
    doc.channels[index] = { ...bridge, tapRetention };
    return { api, doc, index };
  };

  it("accepts the tap's defaults, no limit at all, and the limits' bounds", async () => {
    for (const retention of [
      { days: 30, maxMessages: null },
      { days: null, maxMessages: null },
      { days: 36_500, maxMessages: 1000 },
      { days: 1, maxMessages: 1_000_000_000 },
    ]) {
      const { api, doc } = await withTapRetention(retention);
      expect(at(await check(doc, api), "error")).toEqual([]);
    }
  });

  it("names the field when a limit is out of range", async () => {
    const { api, doc, index } = await withTapRetention({ days: 0, maxMessages: 999 });
    expect(at(await check(doc, api), "error")).toEqual([
      `channels[${index}].tapRetention.days`,
      `channels[${index}].tapRetention.maxMessages`,
    ]);
    const { doc: tooLong } = await withTapRetention({ days: 36_501, maxMessages: null });
    expect(at(await check(tooLong, api), "error")).toEqual([
      `channels[${index}].tapRetention.days`,
    ]);
  });
});

describe("the plan and the problem codes", () => {
  it("plans a valid document the way the engine does: restart, start, stop, untouched", async () => {
    const api = create();
    const doc = await api.getConfig();
    const same = await api.validateConfig(doc);
    expect(same.ok).toBe(true);
    expect(same.plan?.channels.every((item) => item.change === "unchanged")).toBe(true);
    expect(same.plan?.alertsChanged).toBe(false);

    const bridgeIndex = doc.channels.findIndex((c) => c.kind === "bridge");
    const bridge = doc.channels[bridgeIndex];
    if (bridge?.kind !== "bridge") throw new Error("expected a bridge");
    const edited = structuredClone(doc);
    edited.channels[bridgeIndex] = { ...bridge, bufferBytes: bridge.bufferBytes * 2 };
    const added = { ...bridge, id: "ch-new", label: "New bridge" };
    const removed = edited.channels.pop();
    edited.channels.push(added);
    edited.alertThresholds.endpointDownSeconds += 1;
    const plan = (await api.validateConfig(edited)).plan;
    const change = (id: string) => plan?.channels.find((item) => item.id === id)?.change;
    expect(change(bridge.id)).toBe("changed");
    expect(change("ch-new")).toBe("added");
    expect(change(removed?.id ?? "")).toBe("removed");
    expect(plan?.alertsChanged).toBe(true);
    expect(plan?.channels.filter((item) => item.change === "unchanged").length).toBeGreaterThan(0);
  });

  it("restarts the channels that use a changed endpoint, and gives no plan for errors", async () => {
    const api = create();
    const doc = await api.getConfig();
    const bridge = doc.channels.find((c) => c.kind === "bridge");
    if (bridge?.kind !== "bridge") throw new Error("expected a bridge");
    const index = doc.endpoints.findIndex((e) => e.id === bridge.endpointAId);
    const endpoint = doc.endpoints[index];
    if (endpoint === undefined) throw new Error("expected an endpoint");
    const edited = structuredClone(doc);
    edited.endpoints[index] = { ...endpoint, label: `${endpoint.label} renamed` };
    const plan = (await api.validateConfig(edited)).plan;
    expect(plan?.endpoints.find((item) => item.id === endpoint.id)?.change).toBe("changed");
    expect(plan?.channels.find((item) => item.id === bridge.id)?.change).toBe("changed");

    const broken = structuredClone(doc);
    const first = broken.channels.find((c) => c.kind === "bridge");
    if (first?.kind !== "bridge") throw new Error("expected a bridge");
    first.retry = { ...first.retry, initialMs: 5000, maxMs: 1 };
    const result = await api.validateConfig(broken);
    expect(result.ok).toBe(false);
    expect(result.plan).toEqual({ channels: [], endpoints: [], alertsChanged: false });
    expect(result.issues.find((issue) => issue.path.endsWith("retry.maxMs"))?.code).toBe(
      "retry_invalid",
    );
  });
});
