import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import runtimeJson from "../../../../config/runtime.json";
import type { MessageChannelConfig } from "../types";
import { documentFromWire, documentToWire } from "./configDocument";
import realConfigJson from "./real/config.json";
import sparseConfigJson from "./real/sparse.json";
import { DELIMITED_SETTINGS, IDS, richConfig } from "./test/configFixtures";
import { bridge, message } from "./test/documentItems";
import { documentProblems } from "./test/specSchema";
import { isRecord } from "./util";
import type { WireConfig } from "./wire";

const repoConfig = runtimeJson as unknown as WireConfig;
/** A hand-written document that leaves every default out (the sample once shipped in `config/`). */
const sparseConfig = sparseConfigJson as unknown as WireConfig;
/** `packie config show`: the engine answers with every default written out, and `null`s. */
const realConfig = realConfigJson as unknown as WireConfig;

// ── Fixtures: the scenarios' runtime documents and the specification's examples ──────────

type Json = Record<string, unknown>;

const scenarioFiles = import.meta.glob("../../../../tests/scenarios/*.json", {
  eager: true,
  import: "default",
}) as Record<string, { engine: { runtime: Json } }>;

/** What the scenario runner puts in for `${...}`: numbers for ports, text for the rest. */
function substitute(value: unknown, key = "", counter = { next: 41_000 }): unknown {
  if (typeof value === "string" && /^\$\{.*\}$/.test(value)) {
    if (key === "port") return counter.next++;
    return key === "port_name" ? "/dev/ttyPACKIE0" : `/tmp/packie/${value.slice(2, -1)}`;
  }
  if (Array.isArray(value)) return value.map((item) => substitute(item, key, counter));
  if (isRecord(value)) {
    return Object.fromEntries(
      Object.entries(value).map(([k, v]) => [k, substitute(v, k, counter)]),
    );
  }
  return value;
}

const scenarios: [string, WireConfig][] = Object.entries(scenarioFiles).map(([path, scenario]) => [
  path.split("/").pop() ?? path,
  substitute(scenario.engine.runtime) as WireConfig,
]);

/** The JSON examples of section 12 of the configuration specification. */
const specExamples: [string, WireConfig][] = (() => {
  // Tests run in the console's directory; the specification is next to it.
  const specMarkdown = readFileSync("../docs/spec/configuration.md", "utf8");
  const section = specMarkdown.slice(specMarkdown.indexOf("## 12. Examples"));
  const blocks = [...section.matchAll(/```json\n([\s\S]*?)\n```/g)];
  return blocks.map((block, i) => [
    `spec example ${i + 1}`,
    JSON.parse(block[1] ?? "{}") as WireConfig,
  ]);
})();

const FIXTURES: [string, WireConfig][] = [
  ["config/runtime.json", repoConfig],
  ["a sparse document", sparseConfig],
  ["a complete document as the engine returns it", realConfig],
  ["a document with every endpoint and channel kind", richConfig()],
  ...specExamples,
  ...scenarios,
];

const write = documentToWire;

// ── Round trips ─────────────────────────────────────────────────────────────────────────

describe("fixtures", () => {
  it("include every scenario and the specification's examples", () => {
    expect(scenarios.length).toBeGreaterThanOrEqual(20);
    expect(specExamples).toHaveLength(2);
  });
});

describe("document round trip", () => {
  it.each(FIXTURES)("%s: written back unchanged", (_name, raw) => {
    const converted = write(documentFromWire(raw), raw);
    expect(converted.issues).toEqual([]);
    expect(converted.document).toEqual(raw);
  });

  it.each(FIXTURES)("%s: has the strict shape the engine accepts", (_name, raw) => {
    expect(documentProblems(raw)).toEqual([]);
  });

  it.each(FIXTURES)("%s: written without the original, it has that shape too", (_name, raw) => {
    expect(documentProblems(write(documentFromWire(raw)).document)).toEqual([]);
  });

  it.each(FIXTURES)("%s: written without the original, it reads the same", (_name, raw) => {
    const doc = documentFromWire(raw);
    const converted = write(doc);
    expect(converted.issues).toEqual([]);
    expect(documentFromWire(converted.document)).toEqual(doc);
  });

  it("reads the defaults a sparse document leaves out", () => {
    const doc = documentFromWire(sparseConfig);
    expect(doc.version).toBe(0);
    expect(doc.endpoints[0]).toMatchObject({
      kind: "serial",
      id: "hema-serial",
      port: "COM3",
      baudRate: 9600,
      dataBits: 8,
      parity: "none",
      stopBits: 1,
      flowControl: "none",
    });
    expect(doc.endpoints[1]).toMatchObject({
      kind: "tcp_listener",
      allowlist: [],
      allowAnyPeer: false,
      keepalive: true,
      idleTimeoutSeconds: 0,
    });
    expect(doc.endpoints[2]).toMatchObject({ allowlist: ["192.168.10.0/24"] });
    expect(doc.endpoints[3]).toMatchObject({
      kind: "tcp_client",
      connectTimeoutMs: 5000,
      keepalive: true,
    });
    expect(doc.channels[0]).toMatchObject({
      kind: "bridge",
      id: "hema-bridge",
      pairing: "parallel",
      pairingReceiver: null,
      bufferBytes: 10 * 1024 * 1024,
      retry: { initialMs: 1000, maxMs: 60_000, maxAttempts: null },
      tapFramer: "astm",
    });
    const channel = doc.channels[1] as MessageChannelConfig;
    expect(channel.source).toEqual({
      id: "source",
      endpointId: "chem-astm",
      protocol: "astm",
      // What the sparse document leaves out reads as the engine's defaults.
      settings: {
        astmRole: "computer",
        maxFrameChars: 240,
        enqAttempts: 3,
        busyRetryMs: 10_000,
        sampleIdPath: "O.3",
      },
      duplicateDetection: true,
    });
    expect(channel.filters).toEqual([]);
    const lis = channel.destinations[0];
    expect(lis).toMatchObject({
      id: "lis",
      label: "LIS (HL7 ORU)",
      enabled: true,
      protocol: "hl7_mllp",
      settings: { ackMode: "original" },
      filters: [],
      codeTables: {},
      retry: { initialMs: 1000, maxMs: 60_000, maxAttempts: 10 },
    });
    expect(lis?.mappings.map((m) => m.op)).toEqual(["constant", "copy", "copy"]);
    expect(channel.retention).toEqual({ days: 90, maxMessages: null });
    expect(doc.alertThresholds).toEqual({
      endpointDownSeconds: 60,
      queueDepthWarning: 1000,
      bufferPressurePercent: 80,
      diskFreeMinMib: 1024,
    });
  });

  it("reads every modeled field of the rich document", () => {
    const doc = documentFromWire(richConfig());
    expect(doc.version).toBe(7);
    expect(doc.endpoints.map((e) => e.kind)).toEqual([
      "serial",
      "tcp_listener",
      "tcp_listener",
      "tcp_client",
      "folder",
      "sftp",
      "folder",
    ]);
    expect(doc.endpoints[0]).toMatchObject({
      port: "COM3",
      baudRate: 19_200,
      dataBits: 7,
      parity: "even",
      stopBits: 2,
      flowControl: "rts_cts",
    });
    expect(doc.endpoints[1]).toMatchObject({
      bindAddress: "0.0.0.0",
      port: 5200,
      allowlist: ["192.168.10.0/24", "10.0.0.5"],
      idleTimeoutSeconds: 120,
      keepalive: false,
    });
    expect(doc.endpoints[3]).toMatchObject({ connectTimeoutMs: 7000, idleTimeoutSeconds: 30 });
    expect(doc.endpoints[4]).toMatchObject({
      mode: "poll",
      pollIntervalSec: 15,
      filePattern: "*.csv",
      enabled: false,
    });
    expect(doc.endpoints[5]).toMatchObject({
      authMethod: "private_key",
      secretName: "lab-key",
      passphraseSecretName: "lab-key-pass",
      remotePath: "/out",
      pollIntervalSec: 45,
      port: 2222,
      filePattern: "*.hl7",
    });
    expect(doc.endpoints[6]).toMatchObject({ path: "/data/out", mode: "watch", filePattern: "*" });
    expect(bridge(doc)).toMatchObject({
      pairing: "receiver_then_sender",
      pairingReceiver: "a",
      bufferBytes: 2_097_152,
      retry: { initialMs: 2000, maxMs: 30_000, maxAttempts: null },
      tapFramer: "hl7_mllp",
    });
    expect(doc.alertThresholds).toEqual({
      endpointDownSeconds: 90,
      queueDepthWarning: 500,
      bufferPressurePercent: 70,
      diskFreeMinMib: 2048,
    });
  });

  it("reads the message channels of the rich document", () => {
    const doc = documentFromWire(richConfig());
    const channel = message(doc);
    expect(channel.source).toMatchObject({
      id: "source",
      protocol: "astm",
      settings: { astmRole: "instrument" },
      duplicateDetection: false,
    });
    // The `any` filter is shown as a combination of its two comparisons.
    expect(channel.filters).toMatchObject([
      { path: "PID.8", operator: "equals", value: "F", ignoreCase: false },
      { path: "PV1.2", operator: "in", value: "I, E", ignoreCase: false },
      { path: "OBR.4", operator: "exists", value: "", ignoreCase: false },
      {
        any: [
          { path: "MSH.9", operator: "equals", value: "ORU^R01" },
          { path: "MSH.9", operator: "equals", value: "ORU^R03" },
        ],
      },
      { path: "PID.5", operator: "starts_with", value: "doe", ignoreCase: true },
    ]);
    const [hl7, csv, pass] = channel.destinations;
    expect(hl7).toMatchObject({
      id: IDS.hl7,
      protocol: "hl7_mllp",
      settings: { ackMode: "enhanced" },
      enabled: true,
      retry: { initialMs: 1500, maxMs: 120_000, maxAttempts: 8 },
    });
    expect(hl7?.filters).toMatchObject([{ path: "OBX.3", operator: "not_in", value: "X, Y" }]);
    expect(hl7?.mappings).toHaveLength(9);
    expect(hl7?.codeTables).toEqual({ sex: { M: "male", F: "female" }, empty: {} });
    expect(JSON.parse(csv?.settings.templateJson ?? "null")).toEqual(DELIMITED_SETTINGS);
    expect(csv).toMatchObject({
      enabled: false,
      mappings: [{ op: "copy", to: "col.sample_id" }, { op: "copy" }],
      // The channel's delivery policy is the default of a destination that has no overrides.
      retry: { initialMs: 2500, maxMs: 90_000, maxAttempts: 7 },
    });
    expect(pass).toMatchObject({
      protocol: "astm",
      settings: { astmRole: "computer" },
      mappings: [],
      codeTables: {},
    });
    expect(channel.retention).toEqual({ days: 14, maxMessages: 100_000 });
  });

  it("reads the AU instrument settings", () => {
    const doc = documentFromWire(richConfig());
    const channel = message(doc, IDS.au);
    expect(channel.enabled).toBe(false);
    expect(channel.retention).toEqual({ days: null, maxMessages: null });
    expect(channel.source.settings.au).toEqual({
      profile: "au640",
      framingStart: [2],
      framingEnd: [3],
      bcc: true,
      useEtb: true,
      maxWireLen: 512,
      framingClass: "b",
      unitNo: true,
      rackDigits: 4,
      sampleIdDigits: 10,
      sex: true,
      age: false,
      month: true,
      patientInfoDigits: [10, 0, 0, 0, 0, 0],
      resultDigits: 6,
      frameTimeoutMs: 3000,
      blockTimeoutMs: 5000,
    });
  });

  it("shows AU settings that are not all there as not set up", () => {
    const raw = richConfig();
    const au = raw.channels?.[2];
    if (au?.kind !== "message") throw new Error("fixture");
    au.source.settings = { profile: "au400" };
    expect(message(documentFromWire(raw), IDS.au).source.settings).toEqual({});
  });
});
