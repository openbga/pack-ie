import { describe, expect, it } from "vitest";
import { documentFromWire, documentToWire } from "./configDocument";
import realConfigJson from "./real/config.json";
import { IDS, MAPPING, richConfig } from "./test/configFixtures";
import { bridge, destination, diffPaths, message } from "./test/documentItems";
import type { WireConfig } from "./wire";

/** `packie config show`: the engine answers with every default written out, and `null`s. */
const realConfig = realConfigJson as unknown as WireConfig;
const write = documentToWire;

function rawMessage(raw: WireConfig, index = 1) {
  const found = raw.channels?.[index];
  if (found?.kind !== "message") throw new Error("no message channel");
  return found;
}

/** The fourth filter becomes one the console cannot show (an operator it does not know). */
function hideFourthFilter(raw: ReturnType<typeof richConfig>): void {
  const expressions = rawMessage(raw).filters;
  if (expressions) expressions[3] = { path: "MSH.9", op: "near", value: "x" };
}

describe("what the console cannot show is kept", () => {
  it("keeps filters it cannot show after as many shown ones as preceded them", () => {
    const raw = richConfig();
    hideFourthFilter(raw);
    const doc = documentFromWire(raw);
    const channel = message(doc);
    // The hidden filter followed three shown ones; removing the last of them keeps it in place.
    channel.filters = channel.filters.slice(0, 3);
    const { document } = write(doc, raw);
    const written = document.channels?.[1];
    if (written?.kind !== "message") throw new Error("fixture");
    expect(written.filters?.map((f) => f.path)).toEqual(["PID.8", "PV1.2", "OBR.4", "MSH.9"]);
    expect(written.filters?.[3]).toEqual(rawMessage(raw).filters?.[3]);
  });

  it("keeps a filter it cannot show when the shown ones before it are removed", () => {
    const raw = richConfig();
    hideFourthFilter(raw);
    const doc = documentFromWire(raw);
    message(doc).filters = [];
    const { document } = write(doc, raw);
    const written = document.channels?.[1];
    if (written?.kind !== "message") throw new Error("fixture");
    expect(written.filters).toEqual([rawMessage(raw).filters?.[3]]);
  });

  it("keeps a mapping it cannot show and says so when asked to change it", () => {
    const raw = richConfig();
    const channel = raw.channels?.[1];
    if (channel?.kind !== "message") throw new Error("fixture");
    const first = channel.destinations[0];
    if (!first) throw new Error("fixture");
    first.mapping = { rules: [{ op: "explode", to: "A" }] };
    const doc = documentFromWire(raw);
    expect(destination(doc, 0).mappings).toEqual([]);

    destination(doc, 0).label = "Renamed";
    const renamed = write(doc, raw);
    expect(renamed.issues).toEqual([]);
    const afterRename = renamed.document.channels?.[1];
    expect(afterRename?.kind === "message" && afterRename.destinations[0]?.mapping).toEqual(
      first.mapping,
    );

    destination(doc, 0).mappings = [{ id: "n", op: "constant", value: "X", to: "H.5" }];
    const changed = write(doc, raw);
    expect(changed.issues).toEqual([
      {
        severity: "error",
        path: "channels[1].destinations[0].mappings",
        message: expect.stringContaining("cannot show"),
      },
    ]);
    const kept = changed.document.channels?.[1];
    expect(kept?.kind === "message" && kept.destinations[0]?.mapping).toEqual(first.mapping);
  });

  it("writes the rules the operator did not touch exactly as they were", () => {
    const raw = richConfig();
    const doc = documentFromWire(raw);
    const rules = destination(doc, 0).mappings;
    rules.splice(1, 1);
    const { document } = write(doc, raw);
    const written = document.channels?.[1];
    if (written?.kind !== "message") throw new Error("fixture");
    expect(written.destinations[0]?.mapping?.rules).toEqual([
      MAPPING.rules[0],
      ...MAPPING.rules.slice(2),
    ]);
  });
});

describe("structural edits", () => {
  it("removes and reorders items", () => {
    const raw = richConfig();
    const doc = documentFromWire(raw);
    doc.endpoints.reverse();
    message(doc).destinations.reverse();
    const { document } = write(doc, raw);
    expect(document.endpoints?.map((e) => e.endpoint_id)).toEqual(
      [...(raw.endpoints ?? []).map((e) => e.endpoint_id)].reverse(),
    );
    const channel = document.channels?.[1];
    expect(channel?.kind === "message" && channel.destinations.map((d) => d.label)).toEqual([
      "Archive (as received)",
      "Export CSV",
      "LIS (HL7)",
    ]);
    expect(document.endpoints?.find((e) => e.endpoint_id === IDS.serial)).toEqual(
      raw.endpoints?.[0],
    );
  });

  it("removes a channel and its endpoint", () => {
    const raw = richConfig();
    const doc = documentFromWire(raw);
    doc.channels = doc.channels.filter((c) => c.id !== IDS.au);
    doc.endpoints = doc.endpoints.filter((e) => e.id !== IDS.auListener);
    const { document } = write(doc, raw);
    expect(document.channels?.map((c) => c.channel_id)).toEqual([IDS.bridge, IDS.message]);
    expect(document.endpoints).toHaveLength(6);
  });

  it("treats an endpoint whose kind changed as a new one", () => {
    const raw = richConfig();
    const doc = documentFromWire(raw);
    doc.endpoints[0] = {
      kind: "tcp_client",
      id: IDS.serial,
      label: "Now TCP",
      enabled: true,
      host: "h",
      port: 1,
      connectTimeoutMs: 5000,
      idleTimeoutSeconds: 0,
      keepalive: true,
    };
    const { document } = write(doc, raw);
    expect(document.endpoints?.[0]).toEqual({
      endpoint_id: IDS.serial,
      label: "Now TCP",
      kind: "tcp_client",
      settings: { host: "h", port: 1 },
    });
  });

  it("starts the settings of a destination over when its protocol changes", () => {
    const raw = richConfig();
    const doc = documentFromWire(raw);
    const dest = destination(doc, 0);
    dest.protocol = "astm";
    dest.settings = { astmRole: "instrument" };
    const { document } = write(doc, raw);
    const written = document.channels?.[1];
    const first = written?.kind === "message" ? written.destinations[0] : undefined;
    expect(first).toMatchObject({ protocol: "astm", settings: { role: "instrument" } });
    expect(first?.settings).not.toHaveProperty("sending_application");
  });

  it("reports a template that is not JSON and keeps the old one", () => {
    const raw = richConfig();
    const doc = documentFromWire(raw);
    destination(doc, 1).settings.templateJson = "{ not json";
    const converted = write(doc, raw);
    expect(converted.issues).toEqual([
      {
        severity: "error",
        path: "channels[1].destinations[1].settings.templateJson",
        message: "The template is not valid JSON.",
      },
    ]);
    expect(converted.document.channels?.[1]).toEqual(raw.channels?.[1]);
  });

  it("reports an attempt limit on a bridge, which retries until it connects", () => {
    const raw = richConfig();
    const doc = documentFromWire(raw);
    bridge(doc).retry.maxAttempts = 5;
    const converted = write(doc, raw);
    expect(converted.issues).toEqual([
      {
        severity: "error",
        path: "channels[0].retry.maxAttempts",
        message: expect.stringContaining("retries until it connects"),
      },
    ]);
    expect(converted.document.channels?.[0]).toEqual(raw.channels?.[0]);
  });

  it("reports a destination without an attempt limit", () => {
    const raw = richConfig();
    const doc = documentFromWire(raw);
    destination(doc, 0).retry.maxAttempts = null;
    expect(write(doc, raw).issues).toEqual([
      {
        severity: "error",
        path: "channels[1].destinations[0].retry.maxAttempts",
        message: "Maximum attempts is required.",
      },
    ]);
  });
});

describe("editing a complete document from the engine", () => {
  it("changes only what was edited and keeps the engine's explicit defaults", () => {
    const doc = documentFromWire(realConfig);
    const client = doc.endpoints.find((e) => e.id === "sys-side");
    if (client?.kind !== "tcp_client") throw new Error("fixture");
    client.port = 6000;
    const channel = doc.channels[0];
    if (channel?.kind !== "bridge") throw new Error("fixture");
    channel.label = "Renamed";
    channel.pairingReceiver = null;
    const converted = write(doc, realConfig);
    expect(converted.issues).toEqual([]);
    const expected = structuredClone(realConfig);
    const rawClient = expected.endpoints?.[1];
    if (rawClient?.kind === "tcp_client") rawClient.settings.port = 6000;
    const rawBridge = expected.channels?.[0];
    if (rawBridge) rawBridge.label = "Renamed";
    expect(converted.document).toEqual(expected);
  });

  it("sends nothing the engine did not send, except what the operator changed", () => {
    const doc = documentFromWire(realConfig);
    const converted = write(doc, realConfig);
    expect(diffPaths(converted.document, realConfig)).toEqual([]);
  });
});
