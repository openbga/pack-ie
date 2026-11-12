import { describe, expect, it } from "vitest";
import type { ConfigIssue, DestinationConfig, MappingRule } from "../types";
import { whenRule } from "./mappingCatalog";
import {
  at,
  check,
  destinationOf,
  messageChannelAt,
  messageIndex,
  mockApiFactory,
  patchDestination,
} from "./mockTestSupport";

const create = mockApiFactory({ messageCount: 500, seed: 7 });

/** Replaces the mappings of the XN-1000 channel's destination; returns the issues and the index. */
async function withMappings(
  mappings: MappingRule[],
  codeTables?: DestinationConfig["codeTables"],
): Promise<{ issues: ConfigIssue[]; index: number }> {
  const api = create();
  const doc = await api.getConfig();
  const index = messageIndex(doc);
  const destination = destinationOf(messageChannelAt(doc, index));
  patchDestination(doc, index, { mappings, codeTables: codeTables ?? destination.codeTables });
  return { issues: await check(doc, api), index };
}

function copy(to: string, from = "R.3", extra: Partial<MappingRule> = {}): MappingRule {
  return {
    id: `m-${to}`,
    op: "copy",
    from,
    to,
    default: "",
    required: false,
    ...extra,
  } as MappingRule;
}

function loop(rules: MappingRule[]): MappingRule {
  return { id: "loop", op: "for_each", over: "R", into: "OBX", rules };
}

describe("mapping rules", () => {
  it("reports required fields of each op at the rule's own path", async () => {
    const { issues, index } = await withMappings([
      copy("", ""),
      {
        id: "a",
        op: "lookup",
        from: "R.3",
        table: "",
        to: "OBX.3",
        onMissing: "error",
        default: "",
        ignoreCase: false,
      },
      { id: "b", op: "concat", parts: [], to: "OBX.4" },
      {
        id: "c",
        op: "datetime",
        from: "R.13",
        to: "OBR.7",
        input: "",
        output: "",
        fromTimezone: "",
        toTimezone: "",
      },
      loop([]),
      { id: "d", op: "for_each", over: "", into: "", rules: [copy("$.5", "$.4")] },
    ]);
    const errors = at(issues, "error");
    const base = `channels[${index}].destinations[0].mappings`;
    expect(errors).toEqual(
      expect.arrayContaining([
        `${base}[0].from`,
        `${base}[0].to`,
        `${base}[1].table`,
        `${base}[2].parts`,
        `${base}[3].input`,
        `${base}[3].output`,
        `${base}[5].over`,
        `${base}[5].into`,
      ]),
    );
    expect(errors).not.toContain(`${base}[4].over`);
    expect(at(issues, "warning")).toContain(`${base}[4].rules`);
  });

  it("requires a lookup's table to exist in the code tables", async () => {
    const lookup: MappingRule = {
      id: "l",
      op: "lookup",
      from: "R.3",
      table: "known",
      to: "OBX.3",
      onMissing: "error",
      default: "",
      ignoreCase: false,
    };
    const bad = await withMappings([{ ...lookup, table: "unknown" }], { known: { A: "1" } });
    expect(at(bad.issues, "error")).toEqual([
      `channels[${bad.index}].destinations[0].mappings[0].table`,
    ]);
    const good = await withMappings([lookup], { known: { A: "1" } });
    expect(at(good.issues, "error")).toEqual([]);
    const needsDefault = await withMappings([{ ...lookup, onMissing: "default" }], {
      known: { A: "1" },
    });
    expect(at(needsDefault.issues, "error")).toEqual([
      `channels[${needsDefault.index}].destinations[0].mappings[0].default`,
    ]);
  });

  it("allows sequence and $. paths only inside for_each", async () => {
    const outside = await withMappings([
      { id: "s", op: "sequence", to: "OBX.1", start: 1 },
      copy("OBX.5", "$.4"),
    ]);
    const base = `channels[${outside.index}].destinations[0].mappings`;
    expect(at(outside.issues, "error")).toEqual([`${base}[0].op`, `${base}[1].from`]);

    const inside = await withMappings([
      loop([{ id: "s", op: "sequence", to: "$.1", start: 1 }, copy("$.5", "$.4")]),
    ]);
    expect(at(inside.issues, "error")).toEqual([]);
  });

  it("names nested rules by rules, then and else", async () => {
    const when = whenRule(
      "w",
      { path: "$.7", operator: "equals", value: "", ignoreCase: false },
      [copy("", "$.7")],
      [copy("$.8", "bad path")],
    );
    const { issues, index } = await withMappings([loop([copy("$.5", "$.4"), when])]);
    const base = `channels[${index}].destinations[0].mappings[0].rules[1]`;
    expect(at(issues, "error")).toEqual([
      `${base}.condition.value`,
      `${base}.then[0].to`,
      `${base}.else[0].from`,
    ]);
  });

  it("checks time zones and the parts of a concat", async () => {
    const { issues, index } = await withMappings([
      {
        id: "t",
        op: "datetime",
        from: "OBR.7",
        to: "OBR.7",
        input: "yyyyMMdd",
        output: "yyyy-MM-dd",
        fromTimezone: "Not/AZone",
        toTimezone: "Europe/Berlin",
      },
      {
        id: "c",
        op: "concat",
        parts: [{ from: "" }, { text: "" }, { text: " " }, { from: "OBR.4.1" }],
        to: "col.test",
      },
    ]);
    const base = `channels[${index}].destinations[0].mappings`;
    expect(at(issues, "error")).toEqual([
      `${base}[0].fromTimezone`,
      `${base}[1].parts[0].from`,
      `${base}[1].parts[1].text`,
    ]);
  });

  it("warns when two top-level rules write the same target", async () => {
    const { issues, index } = await withMappings([copy("PID.5"), copy("PID.5", "R.4")]);
    expect(at(issues, "error")).toEqual([]);
    expect(at(issues, "warning")).toContain(`channels[${index}].destinations[0].mappings[1].to`);
  });

  it("requires code tables to have names and codes", async () => {
    const { issues, index } = await withMappings([copy("PID.5")], {
      "": { A: "1" },
      ok: { "": "x" },
    });
    expect(at(issues, "error")).toEqual([
      `channels[${index}].destinations[0].codeTables`,
      `channels[${index}].destinations[0].codeTables.ok`,
    ]);
  });
});

describe("destination filters, mappings and pass-through", () => {
  it("needs a mapping when a destination's protocol differs from the source's", async () => {
    const api = create();
    const doc = await api.getConfig();
    const index = messageIndex(doc);
    patchDestination(doc, index, { mappings: [], codeTables: {} });
    const issues = await check(doc, api);
    expect(at(issues, "error")).toEqual([`channels[${index}].destinations[0].protocol`]);
    expect(issues.find((i) => i.severity === "error")?.message).toContain(
      "needs a mapping: its protocol differs from the source's",
    );
    patchDestination(doc, index, { protocol: "astm", settings: { astmRole: "computer" } });
    expect(at(await check(doc, api), "error")).toEqual([]);
  });

  it("accepts the catalog's own pass-through destinations", async () => {
    const doc = await create().getConfig();
    const channel = messageChannelAt(doc, messageIndex(doc, "ch-msg-outreach"));
    expect(destinationOf(channel).mappings).toEqual([]);
    expect(destinationOf(channel).protocol).toBe(channel.source.protocol);
  });

  it("names destination filters by their own path and checks each destination's tables", async () => {
    const api = create();
    const doc = await api.getConfig();
    const index = messageIndex(doc, "ch-msg-sta");
    const channel = messageChannelAt(doc, index);
    expect(channel.destinations).toHaveLength(2);
    patchDestination(
      doc,
      index,
      { filters: [{ id: "f", path: "PID..3", operator: "equals", value: "", ignoreCase: false }] },
      1,
    );
    expect(at(await check(doc, api), "error")).toEqual([
      `channels[${index}].destinations[1].filters[0].path`,
      `channels[${index}].destinations[1].filters[0].value`,
    ]);
    // Tables belong to one destination: the second has none, so its lookup finds no table.
    const lookup = destinationOf(channel).mappings.find((m) => m.op === "for_each");
    if (lookup === undefined) throw new Error("expected a loop");
    patchDestination(doc, index, { filters: [], mappings: [lookup], codeTables: {} }, 1);
    const errors = at(await check(doc, api), "error");
    expect(errors).toEqual([`channels[${index}].destinations[1].mappings[0].rules[2].table`]);
  });
});
