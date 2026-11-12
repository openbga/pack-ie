import { describe, expect, it } from "vitest";
import { emptyRule } from "../ruleForm";
import { emptySettings } from "../settingsForm";
import { planMapping, planProfile, planSettings, sourceHints } from "./apply";
import { connectorProfiles, mappingPresets, presetsFor } from "./catalog";

const profile = (id: string) => {
  const found = connectorProfiles.find((each) => each.id === id);
  if (!found) throw new Error(id);
  return found;
};

describe("applying a connector profile", () => {
  it("lists each setting with what it is now and what it becomes", () => {
    const plan = planProfile(profile("hl7_mllp_end_without_cr"), emptySettings());
    expect(plan.unknown).toEqual([]);
    expect(plan.changes).toEqual([
      {
        field: "requireEndCr",
        label: "Carriage return after the end block",
        now: true,
        becomes: false,
        changes: true,
      },
    ]);
  });

  it("marks what is already set, and compares lists by their codes", () => {
    const current = { ...emptySettings(), requireEndCr: false, ackMessageTypes: "ACK,  ORL" };
    const plan = planSettings(
      { require_end_cr: false, ack_message_types: ["ACK", "ORL"] },
      current,
    );
    expect(plan.changes.map((change) => change.changes)).toEqual([false, false]);
  });

  it("turns numbers into the form's text and names a setting it does not know", () => {
    const plan = planSettings(
      { max_frame_chars: 64000, enq_attempts: 7, warp_factor: 9 },
      emptySettings(),
    );
    expect(plan.changes.map((change) => [change.field, change.becomes])).toEqual([
      ["maxFrameChars", "64000"],
      ["enqAttempts", "7"],
    ]);
    expect(plan.unknown).toEqual(["warp_factor"]);
  });

  it("names a setting whose value has the wrong shape instead of applying it", () => {
    expect(planSettings({ require_end_cr: "no" }, emptySettings()).unknown).toEqual([
      "require_end_cr",
    ]);
  });
});

describe("applying a mapping preset", () => {
  const preset = presetsFor("astm", "hl7_mllp")[0];
  if (!preset) throw new Error("catalog has no ASTM to HL7 preset");
  const own = { ...emptyRule("m1", "constant"), value: "LAB", to: "MSH.4" };
  const current = (rules = [own]) => ({
    mappings: rules,
    codeTables: [{ name: "tests", rows: [{ code: "GLU", text: "Glucose" }] }],
    requireSingleGroup: false,
    require: [],
    settings: emptySettings(),
  });

  it("adds the preset's rules after the operator's, with ids nothing else uses", () => {
    const plan = planMapping(preset, current(), "add");
    expect(plan?.mappings[0]).toBe(own);
    const ids = JSON.stringify(plan?.mappings).match(/"ruleId":"[^"]+"/g) ?? [];
    expect(new Set(ids).size).toBe(ids.length);
    expect(plan?.rulesAdded).toBe((plan?.mappings.length ?? 0) - 1);
    expect(plan?.rulesRemoved).toBe(0);
  });

  it("never overwrites a table the operator already has under the same name", () => {
    const plan = planMapping(preset, current(), "add");
    expect(plan?.tablesKept).toContain("tests");
    expect(plan?.codeTables).toEqual(current().codeTables);
  });

  it("replaces only when asked, and counts what goes", () => {
    const plan = planMapping(preset, current(), "replace");
    expect(plan?.mappings.every((rule) => rule.ruleId !== "m1" || rule !== own)).toBe(true);
    expect(plan?.rulesRemoved).toBe(1);
    expect(plan?.tablesRemoved).toBe(1);
    expect(plan?.tablesAdded).toContain("tests");
    expect(plan?.codeTables.find((table) => table.name === "tests")?.rows).toEqual([]);
  });

  it("reports the destination settings it would write", () => {
    const plan = planMapping(preset, current(), "add");
    expect(plan?.settings.changes.map((change) => change.field)).toEqual(["hl7Version"]);
  });

  it("every preset in the catalog can be applied to an empty destination", () => {
    for (const each of mappingPresets) {
      const plan = planMapping(
        each,
        {
          mappings: [],
          codeTables: [],
          requireSingleGroup: false,
          require: [],
          settings: emptySettings(),
        },
        "add",
      );
      expect(plan, each.id).not.toBeNull();
      expect(plan?.rulesAdded, each.id).toBeGreaterThan(0);
      expect(plan?.settings.unknown, each.id).toEqual([]);
    }
  });

  it("takes the order download presets' requirements and repeat loop as they are", () => {
    const orders = presetsFor("hl7_mllp", "astm");
    expect(orders.length).toBeGreaterThan(0);
    for (const each of orders) {
      const plan = planMapping(
        each,
        {
          mappings: [],
          codeTables: [],
          requireSingleGroup: false,
          require: [],
          settings: emptySettings(),
        },
        "add",
      );
      expect(plan?.requirementsAdded, each.id).toBeGreaterThan(0);
      expect(
        plan?.require.some((entry) => entry.kind === "count"),
        each.id,
      ).toBe(true);
      expect(
        plan?.require.some((entry) => entry.kind === "same"),
        each.id,
      ).toBe(true);
      expect(JSON.stringify(plan?.mappings).includes('"mode":"repeat"'), each.id).toBe(true);
    }
  });

  it("does not add a requirement the operator already has", () => {
    const [order] = presetsFor("hl7_mllp", "astm");
    if (!order) throw new Error("catalog has no order preset");
    const first = planMapping(
      order,
      {
        mappings: [],
        codeTables: [],
        requireSingleGroup: false,
        require: [],
        settings: emptySettings(),
      },
      "add",
    );
    const again = planMapping(
      order,
      {
        mappings: [],
        codeTables: [],
        requireSingleGroup: false,
        require: first?.require ?? [],
        settings: emptySettings(),
      },
      "add",
    );
    expect(again?.requirementsAdded).toBe(0);
    expect(again?.require).toEqual(first?.require);
  });
});

describe("sample id path of the ASTM source", () => {
  it("lists the source preset's setting as a change from O.3", () => {
    const plan = planProfile(profile("astm_specimen_id_in_o4_component_3"), emptySettings());
    expect(plan.unknown).toEqual([]);
    expect(plan.changes).toEqual([
      {
        field: "sampleIdPath",
        label: "Sample id path",
        now: "O.3",
        becomes: "O.4.3",
        changes: true,
      },
    ]);
  });

  it("points a mapping preset to the source preset that sets what its example was read with", () => {
    const preset = mappingPresets.find(
      (each) => each.id === "astm_results_to_hl7_oru_instrument_specimen_id",
    );
    if (!preset) throw new Error("catalog has no specimen id mapping preset");
    const hints = sourceHints(preset, emptySettings(), connectorProfiles);
    expect(hints.map((hint) => [hint.change.field, hint.change.becomes])).toEqual([
      ["sampleIdPath", "O.4.3"],
    ]);
    expect(hints[0]?.profile).toBe(profile("astm_specimen_id_in_o4_component_3").label);
    // Nothing to point to once the source has it.
    expect(
      sourceHints(preset, { ...emptySettings(), sampleIdPath: "O.4.3" }, connectorProfiles),
    ).toEqual([]);
  });

  it("gives no hint for a preset whose example reads the source as it is, or an AU sheet", () => {
    for (const id of ["astm_results_to_hl7_oru", "au_results_to_hl7_oru"]) {
      const preset = mappingPresets.find((each) => each.id === id);
      expect(preset && sourceHints(preset, emptySettings(), connectorProfiles), id).toEqual([]);
    }
  });
});
