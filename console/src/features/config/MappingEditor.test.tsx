import { screen, within } from "@testing-library/react";
import type { UserEvent } from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type {
  ConfigDocument,
  DestinationConfig,
  MappingRule,
  MessageChannelConfig,
} from "@/api/types";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { branches } from "./ruleForm";
import { followFinding, group, openResults, pick, textbox, validatedChannel } from "./testActions";
import { sampleConfig, stubApi, validationFails } from "./testFixtures";

const fieldValue = (scope: HTMLElement, name: RegExp) =>
  (textbox(scope, name) as HTMLInputElement).value;

/** Rule 1 is a loop over R into OBX holding a sequence, a lookup and a when; then a datetime and a concat. */
function loopDocument(): ConfigDocument {
  const doc = sampleConfig();
  const destination = (doc.channels[1] as MessageChannelConfig)
    .destinations[0] as DestinationConfig;
  const mappings: MappingRule[] = [
    {
      id: "m1",
      op: "for_each",
      over: "R",
      into: "OBX",
      rules: [
        { id: "m2", op: "sequence", to: "$.1", start: 1 },
        {
          id: "m3",
          op: "lookup",
          from: "$.3",
          table: "units",
          to: "$.3[0].1",
          onMissing: "keep",
          default: "",
          ignoreCase: false,
        },
        {
          id: "m4",
          op: "when",
          condition: { path: "$.7", operator: "exists", value: "", ignoreCase: false },
          ...branches<MappingRule>(
            [{ id: "m5", op: "copy", from: "$.7", to: "$.8", default: "", required: false }],
            [{ id: "m6", op: "constant", value: "N", to: "$.8" }],
          ),
        },
      ],
    },
    {
      id: "m7",
      op: "datetime",
      from: "R.13",
      to: "OBR.7",
      input: "yyyyMMddHHmmss",
      output: "yyyy-MM-dd HH:mm:ss",
      fromTimezone: "UTC",
      toTimezone: "Europe/Berlin",
    },
    {
      id: "m8",
      op: "concat",
      parts: [{ from: "P.4" }, { text: " / " }, { from: "P.5" }],
      to: "PID.5",
    },
  ];
  destination.mappings = mappings;
  destination.codeTables = { units: { "mg/dl": "mg/dL", "g/l": "g/L" } };
  return doc;
}

const open = (doc: ConfigDocument = loopDocument()) =>
  openResults(stubApi({ getConfig: async () => doc }));

async function validatedDestination(
  user: UserEvent,
  api: Awaited<ReturnType<typeof open>>["api"],
): Promise<DestinationConfig> {
  const channel = await validatedChannel(user, api);
  return channel.destinations[0] as DestinationConfig;
}

describe("mapping editor", () => {
  it("shows every level with its own labelled lists, and writes the same document back", async () => {
    const doc = loopDocument();
    const { api, user } = await open(doc);
    expect(screen.getAllByRole("group", { name: /^Destination 1 mapping rule \d/ })).toHaveLength(
      3,
    );
    const loop = group("Destination 1 mapping rule 1");
    expect(fieldValue(loop, /^Over/)).toBe("R");
    expect(fieldValue(loop, /^Into/)).toBe("OBX");
    expect(group("Rules inside destination 1 mapping rule 1")).toBeTruthy();
    expect(group("Destination 1 loop rule 1.1")).toBeTruthy();
    expect(group("Destination 1 loop rule 1.3")).toBeTruthy();
    expect(group("Condition of destination 1 loop rule 1.3")).toBeTruthy();
    expect(group("Then rules of destination 1 loop rule 1.3")).toBeTruthy();
    expect(group("Else rules of destination 1 loop rule 1.3")).toBeTruthy();
    expect(fieldValue(group("Destination 1 then rule 1.3.1"), /^To$/)).toBe("$.8");
    expect(fieldValue(group("Destination 1 else rule 1.3.1"), /^Value/)).toBe("N");
    expect(fieldValue(group("Destination 1 mapping rule 3"), /^To$/)).toBe("PID.5");
    expect(fieldValue(group("Part 2 of destination 1 mapping rule 3"), /^Text/)).toBe(" / ");
    const channel = await validatedDestination(user, api);
    expect(channel.mappings).toEqual(
      ((doc.channels[1] as MessageChannelConfig).destinations[0] as DestinationConfig).mappings,
    );
    expect(channel.codeTables).toEqual({ units: { "mg/dl": "mg/dL", "g/l": "g/L" } });
  });

  it("adds a sequence inside a for_each, and moves and removes rules at that level", async () => {
    const { api, user } = await open();
    await user.click(
      screen.getByRole("button", { name: "Add rule inside destination 1 mapping rule 1" }),
    );
    const added = group("Destination 1 loop rule 1.4");
    await pick(user, added, /Rule/, "sequence");
    await user.type(textbox(added, /^To$/), "$.9");
    expect(fieldValue(added, /^Start/)).toBe("1");
    await user.click(screen.getByRole("button", { name: "Move destination 1 loop rule 1.4 up" }));
    await user.click(screen.getByRole("button", { name: "Remove destination 1 loop rule 1.1" }));
    expect(screen.queryByRole("group", { name: "Destination 1 loop rule 1.4" })).toBeNull();
    const channel = await validatedDestination(user, api);
    const loop = channel.mappings[0];
    expect(loop?.op === "for_each" && loop.rules.map((rule) => rule.id)).toEqual([
      "m3",
      "m9",
      "m4",
    ]);
    expect(loop?.op === "for_each" && loop.rules[1]).toEqual({
      id: "m9",
      op: "sequence",
      to: "$.9",
      start: 1,
    });
  });

  it("offers sequence only inside a for_each", async () => {
    const { user } = await open();
    await user.click(screen.getByRole("button", { name: "Add destination 1 mapping rule" }));
    await user.click(
      within(group("Destination 1 mapping rule 4")).getByRole("button", { name: /Rule/ }),
    );
    const top = screen.getAllByRole("option").map((option) => option.textContent);
    expect(top).toEqual([
      "copy",
      "constant",
      "lookup",
      "concat",
      "datetime",
      "calculate",
      "when",
      "for_each",
      "append",
    ]);
    await user.keyboard("{Escape}");
    await user.click(
      screen.getByRole("button", { name: "Add rule inside destination 1 mapping rule 1" }),
    );
    await user.click(
      within(group("Destination 1 loop rule 1.4")).getByRole("button", { name: /Rule/ }),
    );
    const inLoop = screen.getAllByRole("option").map((option) => option.textContent);
    // A loop holds no loop and no append; sequence counts its records.
    expect(inLoop).toEqual([
      "copy",
      "constant",
      "lookup",
      "concat",
      "datetime",
      "calculate",
      "when",
      "sequence",
    ]);
  });

  it("explains the chosen operator in one line and shows its fields", async () => {
    const { user } = await open();
    await user.click(screen.getByRole("button", { name: "Add destination 1 mapping rule" }));
    const rule = group("Destination 1 mapping rule 4");
    expect(within(rule).getByText(/^Copies the text of From to To/)).toBeTruthy();
    await pick(user, rule, /Rule/, "datetime");
    expect(within(rule).getByText(/^Reformats a date or time/)).toBeTruthy();
    for (const label of [
      /^From$/,
      /^To$/,
      /^Input format/,
      /^Output format/,
      /^From timezone/,
      /^To timezone/,
    ]) {
      expect(textbox(rule, label)).toBeTruthy();
    }
    await pick(user, rule, /Rule/, "constant");
    expect(within(rule).getByText("Writes the Value as text to To.")).toBeTruthy();
    expect(within(rule).queryByRole("textbox", { name: /^From$/ })).toBeNull();
    await pick(user, rule, /Rule/, "for_each");
    expect(within(rule).getByText(/^For every occurrence of the record or segment/)).toBeTruthy();
    expect(textbox(rule, /^Over/)).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "Add rule inside destination 1 mapping rule 4" }),
    ).toBeTruthy();
  });

  it("chooses a lookup's table from the code tables", async () => {
    const { api, user } = await open();
    await user.click(screen.getByRole("button", { name: "Add destination 1 mapping rule" }));
    const rule = group("Destination 1 mapping rule 4");
    await pick(user, rule, /Rule/, "lookup");
    expect(within(rule).queryByRole("textbox", { name: /^Default/ })).toBeNull();
    await user.type(textbox(rule, /^From$/), "R.5");
    await user.type(textbox(rule, /^To$/), "OBX.6");
    await user.click(within(rule).getByRole("button", { name: /Table/ }));
    expect(screen.getAllByRole("option").map((option) => option.textContent)).toEqual(["units"]);
    await user.click(screen.getByRole("option", { name: "units" }));
    await pick(user, rule, /On missing/, "Use the default");
    await user.type(textbox(rule, /^Default/), "?");
    const channel = await validatedDestination(user, api);
    expect(channel.mappings[3]).toEqual({
      id: "m9",
      op: "lookup",
      from: "R.5",
      table: "units",
      to: "OBX.6",
      onMissing: "default",
      default: "?",
      ignoreCase: false,
    });
  });

  it("asks for a table before any exists, and keeps a table name that no longer exists visible", async () => {
    const doc = sampleConfig();
    const { user } = await open(doc);
    await user.click(screen.getByRole("button", { name: "Add destination 1 mapping rule" }));
    const rule = group("Destination 1 mapping rule 3");
    await pick(user, rule, /Rule/, "lookup");
    expect(
      within(rule).getByText("Add a code table to this destination below first."),
    ).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Add destination 1 code table" }));
    await user.type(textbox(group("Destination 1 table 1"), /^Table name/), "units");
    await user.click(within(rule).getByRole("button", { name: /Table/ }));
    await user.click(await screen.findByRole("option", { name: "units" }));
    await user.clear(textbox(group("Destination 1 table 1"), /^Table name/));
    await user.type(textbox(group("Destination 1 table 1"), /^Table name/), "unit-names");
    expect(within(rule).getByRole("button", { name: /units \(no such table\)/ })).toBeTruthy();
  });

  it("edits code tables: tables and their rows, with unique names", async () => {
    const { api, user } = await open();
    expect(fieldValue(group("Row 2 of destination 1 table 1"), /^Code/)).toBe("g/l");
    await user.click(screen.getByRole("button", { name: "Add row to destination 1 table 1" }));
    const row = group("Row 3 of destination 1 table 1");
    await user.type(textbox(row, /^Code/), "%");
    await user.type(textbox(row, /^Text/), "percent");
    await user.click(screen.getByRole("button", { name: "Remove row 1 of destination 1 table 1" }));
    await user.click(screen.getByRole("button", { name: "Add destination 1 code table" }));
    await user.type(textbox(group("Destination 1 table 2"), /^Table name/), "units");
    await user.click(screen.getByRole("button", { name: "Validate" }));
    expect(
      await within(group("Destination 1 table 2")).findByText(
        "Another table already has this name.",
      ),
    ).toBeTruthy();
    expect(api.validateConfig).not.toHaveBeenCalled();
    await user.clear(textbox(group("Destination 1 table 2"), /^Table name/));
    await user.type(textbox(group("Destination 1 table 2"), /^Table name/), "flags");
    const channel = await validatedDestination(user, api);
    expect(channel.codeTables).toEqual({
      units: { "g/l": "g/L", "%": "percent" },
      flags: {},
    });
  });

  it("edits the parts of a concat: add, reorder and remove", async () => {
    const { api, user } = await open();
    await user.click(
      screen.getByRole("button", { name: "Add part to destination 1 mapping rule 3" }),
    );
    const part = group("Part 4 of destination 1 mapping rule 3");
    await pick(user, part, /Part type/, "Text");
    await user.type(textbox(part, /^Text/), "!");
    await user.click(
      screen.getByRole("button", { name: "Move part 4 of destination 1 mapping rule 3 up" }),
    );
    await user.click(
      screen.getByRole("button", { name: "Remove part 1 of destination 1 mapping rule 3" }),
    );
    const channel = await validatedDestination(user, api);
    expect(channel.mappings[2]).toMatchObject({
      op: "concat",
      parts: [{ text: " / " }, { text: "!" }, { from: "P.5" }],
    });
  });

  it("shows engine findings on the nested field they name and moves focus to it", async () => {
    const api = stubApi({
      getConfig: async () => loopDocument(),
      ...validationFails(
        {
          severity: "error",
          path: "channels[1].destinations[0].mappings[0].rules[0].to",
          message: "Bad target.",
        },
        {
          severity: "error",
          path: "channels[1].destinations[0].mappings[0].rules[2].then[0].from",
          message: "Unknown field.",
        },
        {
          severity: "error",
          path: "channels[1].destinations[0].mappings[0].rules[2].condition.path",
          message: "Unknown condition field.",
        },
        {
          severity: "error",
          path: "channels[1].destinations[0].mappings[2].parts[0].from",
          message: "No such path.",
        },
        {
          severity: "error",
          path: "channels[1].destinations[0].mappings[1].input",
          message: "Unusable format.",
        },
      ),
    });
    const { user } = await openResults(api);
    await user.click(screen.getByRole("button", { name: "Validate" }));
    expect(
      await within(group("Destination 1 loop rule 1.1")).findByText("Bad target."),
    ).toBeTruthy();
    expect(within(group("Destination 1 then rule 1.3.1")).getByText("Unknown field.")).toBeTruthy();
    expect(
      within(group("Condition of destination 1 loop rule 1.3")).getByText(
        "Unknown condition field.",
      ),
    ).toBeTruthy();
    expect(
      within(group("Part 1 of destination 1 mapping rule 3")).getByText("No such path."),
    ).toBeTruthy();
    expect(
      within(group("Destination 1 mapping rule 2")).getByText("Unusable format."),
    ).toBeTruthy();
    await followFinding(user, "Unknown field.", () =>
      textbox(group("Destination 1 then rule 1.3.1"), /^From$/),
    );
  });

  it("checks required fields of nested rules before calling the engine", async () => {
    const { api, user } = await open();
    await user.click(
      screen.getByRole("button", { name: "Add rule inside destination 1 mapping rule 1" }),
    );
    await user.click(
      screen.getByRole("button", { name: "Add then rule to destination 1 loop rule 1.3" }),
    );
    await user.click(screen.getByRole("button", { name: "Save and apply" }));
    expect(
      await within(group("Destination 1 loop rule 1.4")).findByText("Enter from."),
    ).toBeTruthy();
    expect(within(group("Destination 1 then rule 1.3.2")).getByText("Enter to.")).toBeTruthy();
    expect(api.validateConfig).not.toHaveBeenCalled();
  });

  it("has no accessibility violations with nested rules", async () => {
    const { container } = await open();
    await expectNoA11yViolations(container);
  });
});
