import { describe, expect, it } from "vitest";
import type { ConfigDocument, MessageChannelConfig } from "@/api/types";
import { compareDocuments, hasDifferences } from "./documentCompare";
import { sampleConfig } from "./testFixtures";

function message(document: ConfigDocument): MessageChannelConfig {
  const channel = document.channels.find((each) => each.id === "results");
  if (channel?.kind !== "message") throw new Error("fixture");
  return channel;
}

describe("compareDocuments", () => {
  it("finds nothing between a document and a copy of it", () => {
    expect(hasDifferences(compareDocuments(sampleConfig(), sampleConfig()))).toBe(false);
  });

  it("lists a changed port as field: old → new, an added channel and a removed one", () => {
    const active = sampleConfig();
    const version = sampleConfig();
    const lis = version.endpoints.find((endpoint) => endpoint.id === "lis-out");
    if (lis?.kind !== "tcp_client") throw new Error("fixture");
    lis.port = 3002;
    const results = message(version);
    version.channels = [
      ...version.channels.filter((channel) => channel.id !== "chem-bridge"),
      { ...results, id: "billing", label: "Billing" },
    ];
    const comparison = compareDocuments(active, version);
    expect(comparison.endpoints).toEqual([
      {
        id: "lis-out",
        label: "LIS host",
        kind: "changed",
        fields: [{ label: "Port", before: "3001", after: "3002" }],
      },
    ]);
    expect(comparison.channels.map(({ id, kind }) => `${kind} ${id}`)).toEqual([
      "added billing",
      "removed chem-bridge",
    ]);
    expect(comparison.thresholds).toEqual([]);
  });

  it("names labelled fields and endpoints in words, and nested settings by path", () => {
    const active = sampleConfig();
    const version = sampleConfig();
    const bridge = version.channels.find((channel) => channel.id === "chem-bridge");
    if (bridge?.kind !== "bridge") throw new Error("fixture");
    bridge.endpointBId = "lis-out";
    bridge.retry = { ...bridge.retry, initialMs: 2000, maxAttempts: null };
    bridge.pairing = "receiver_then_sender";
    version.alertThresholds.endpointDownSeconds = 30;
    const comparison = compareDocuments(active, version);
    expect(comparison.channels[0]?.fields).toEqual([
      { label: "Endpoint B", before: "Analyzer link", after: "LIS host" },
      { label: "Pairing", before: "parallel", after: "receiver then sender" },
      { label: "Retry › Initial backoff (ms)", before: "1000", after: "2000" },
      { label: "Retry › Maximum attempts", before: "10", after: "none" },
    ]);
    expect(comparison.thresholds).toEqual([
      { label: "Endpoint down limit (s)", before: "120", after: "30" },
    ]);
  });

  it("summarises rule arrays as counts, with the rules behind them", () => {
    const active = sampleConfig();
    const version = sampleConfig();
    const destination = message(version).destinations[0];
    if (destination === undefined) throw new Error("fixture");
    destination.mappings = [
      ...destination.mappings.slice(0, 1),
      { id: "m3", op: "constant", value: "LAB2", to: "MSH.5" },
      { id: "m4", op: "sequence", to: "OBX.1", start: 1 },
    ];
    destination.retry = { ...destination.retry, maxAttempts: 9 };
    const [channel] = compareDocuments(active, version).channels;
    expect(channel?.fields).toEqual([
      {
        label: "Destination LIS › Mappings",
        before: "2 rules",
        after: "3 rules",
        detail: [
          { label: "Rule m3", before: "not present", after: "op constant, value LAB2, to MSH.5" },
          { label: "Rule m4", before: "not present", after: "op sequence, to OBX.1, start 1" },
          { label: "Rule m2", before: "op constant, value LAB, to MSH.4", after: "removed" },
        ],
      },
      { label: "Destination LIS › Retry › Maximum attempts", before: "5", after: "9" },
    ]);
  });

  it("shows lists of values as lists and keeps code table names as they are", () => {
    const active = sampleConfig();
    const version = sampleConfig();
    const listener = version.endpoints.find((endpoint) => endpoint.id === "analyzer-in");
    if (listener?.kind !== "tcp_listener") throw new Error("fixture");
    listener.allowlist = ["10.0.4.20", "10.0.4.21"];
    const destination = message(version).destinations[0];
    if (destination === undefined) throw new Error("fixture");
    destination.codeTables = { sex_codes: { F: "female" } };
    const comparison = compareDocuments(active, version);
    expect(comparison.endpoints[0]?.fields).toEqual([
      { label: "Allowlist", before: "10.0.4.20", after: "10.0.4.20, 10.0.4.21" },
    ]);
    expect(comparison.channels[0]?.fields).toEqual([
      { label: "Destination LIS › Code tables › sex_codes", before: "none", after: "F: female" },
    ]);
  });
});
