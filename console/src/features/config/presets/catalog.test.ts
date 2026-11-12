import { describe, expect, it } from "vitest";
import { mappingFromWire } from "@/api/ipc/mappings";
import type { WireMapping } from "@/api/ipc/wire";
import { connectorProfiles, mappingPresets, presetsFor, profilesFor } from "./catalog";

describe("preset catalog", () => {
  it("lists connector profiles by protocol and role", () => {
    expect(connectorProfiles.length).toBeGreaterThan(0);
    expect(profilesFor("hl7_mllp", "destination").map((profile) => profile.id)).toContain(
      "hl7_mllp_end_without_cr",
    );
    expect(profilesFor("astm", "source").every((profile) => profile.protocol === "astm")).toBe(
      true,
    );
    expect(profilesFor("delimited_file", "source")).toEqual([]);
    expect(profilesFor(null, "source")).toEqual([]);
  });

  it("lists mapping presets by the pair of protocols", () => {
    expect(presetsFor("astm", "hl7_mllp").length).toBeGreaterThan(0);
    expect(presetsFor("hl7_mllp", "astm").every((preset) => preset.to === "astm")).toBe(true);
    expect(presetsFor("astm", "astm")).toEqual([]);
  });

  it("holds only mappings the console can show completely", () => {
    for (const preset of mappingPresets) {
      expect(mappingFromWire(preset.mapping as WireMapping), preset.id).not.toBeNull();
      expect(preset.sample.input, preset.id).not.toBe("");
    }
  });
});
