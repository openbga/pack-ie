import { describe, expect, it } from "vitest";
import {
  endpointReferences,
  nextId,
  removedDestinations,
  slugify,
  uniqueSlug,
} from "./configDocument";
import { sampleConfig } from "./testFixtures";

describe("ids", () => {
  it("suggests a lower-case slug and keeps it unique", () => {
    expect(slugify("  Chem analyzer #2 (COM3)  ")).toBe("chem-analyzer-2-com3");
    expect(slugify("???")).toBe("");
    expect(uniqueSlug("LIS out", new Set(["lis-out"]))).toBe("lis-out-2");
    expect(uniqueSlug("LIS out", new Set(["lis-out", "lis-out-2"]))).toBe("lis-out-3");
    expect(nextId("m", ["m1", "m3"])).toBe("m2");
  });
});

describe("endpointReferences", () => {
  it("names each channel and role that uses an endpoint", () => {
    const references = endpointReferences(sampleConfig(), "analyzer-in");
    expect(references.map((each) => `${each.channelId}:${each.role}`)).toEqual([
      "chem-bridge:Endpoint B",
      "results:Source",
    ]);
    expect(endpointReferences(sampleConfig(), "lis-out").map((each) => each.role)).toEqual([
      "Destination 1",
    ]);
  });
});

describe("removedDestinations", () => {
  it("names destinations dropped from channels that remain, not those of removed channels", () => {
    const before = sampleConfig();
    const after = sampleConfig();
    const results = after.channels.find((channel) => channel.id === "results");
    if (results?.kind !== "message") throw new Error("fixture");
    results.destinations = [];
    expect(removedDestinations(before, after)).toEqual([
      { channelId: "results", channelLabel: "Results to LIS", destinationLabel: "LIS" },
    ]);
    const withoutChannel = { ...after, channels: after.channels.filter((c) => c.id !== "results") };
    expect(removedDestinations(before, withoutChannel)).toEqual([]);
  });
});
