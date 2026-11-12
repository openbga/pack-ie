// The contract's remaining rule shapes in the editor: text steps, find operands, append and
// combined conditions, each read from a document, shown in words and written back unchanged.
import { screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type {
  ConfigDocument,
  DestinationConfig,
  FilterRule,
  MappingRule,
  MessageChannelConfig,
} from "@/api/types";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { group, openResults, pick, textbox, validatedChannel } from "./testActions";
import { sampleConfig, stubApi } from "./testFixtures";

const equalsCode = (code: string) => ({
  path: "$.3.1",
  operator: "equals" as const,
  value: code,
  ignoreCase: false,
});

const find = (code: string) => ({
  find: "OBX",
  where: equalsCode(code),
  from: "$.5",
  steps: [],
});

function contractDocument(): ConfigDocument {
  const doc = sampleConfig();
  const destination = (doc.channels[1] as MessageChannelConfig)
    .destinations[0] as DestinationConfig;
  const mappings: MappingRule[] = [
    {
      id: "m1",
      op: "append",
      into: "OBX",
      rules: [
        {
          id: "m2",
          op: "calculate",
          formula: {
            op: "sub",
            args: [find("NA"), { op: "add", args: [find("CL"), find("HCO3")] }],
          },
          to: "$.5",
          scale: null,
          rounding: "half_up",
          comparator: "carry",
          notation: "text",
          onInvalid: "skip",
          default: "",
          required: false,
        },
        { id: "m3", op: "sequence", to: "$.1", start: 1 },
        { id: "m4", op: "constant", value: "AG^Anion gap", to: "$.3" },
      ],
    },
    {
      id: "m5",
      op: "copy",
      from: "PID.3",
      to: "PID.4",
      default: "",
      required: false,
      steps: [
        { step: "strip_leading", chars: "0" },
        { step: "split", separator: "/", take: -1 },
      ],
    },
    {
      id: "m6",
      op: "calculate",
      formula: { from: "OBX[1].5", steps: [{ step: "replace", find: ",", with: "." }] },
      to: "OBX[1].5",
      scale: 1,
      rounding: "half_even",
      comparator: "reject",
      notation: "text",
      onInvalid: "keep",
      default: "",
      required: false,
    },
  ];
  destination.mappings = mappings;
  return doc;
}

const open = (doc: ConfigDocument = contractDocument()) =>
  openResults(stubApi({ getConfig: async () => doc }));

const value = (scope: HTMLElement, name: RegExp) =>
  (textbox(scope, name) as HTMLInputElement).value;

describe("append, find operands and text steps", () => {
  it("shows an appended record with its named operands, and writes the same document back", async () => {
    const doc = contractDocument();
    const { api, user } = await open(doc);
    const append = group("Destination 1 mapping rule 1");
    expect(value(append, /^Into/)).toBe("OBX");
    const calculate = group("Destination 1 appended rule 1.1");
    expect(value(calculate, /^Formula/)).toBe("@na - (@cl + @hco3)");
    expect(within(calculate).getByText(/^Reads as @na − \(@cl \+ @hco3\)\./)).toBeTruthy();
    const operand = group("Operand 1 of destination 1 appended rule 1.1");
    expect(value(operand, /^Name/)).toBe("na");
    expect(value(operand, /^Record or segment/)).toBe("OBX");
    expect(value(operand, /^Value path/)).toBe("$.5");
    expect(value(operand, /^Path/)).toBe("$.3.1");
    expect(value(operand, /^Value$/)).toBe("NA");
    expect(group("Operand 3 of destination 1 appended rule 1.1")).toBeTruthy();
    const channel = await validatedChannel(user, api);
    expect((channel.destinations[0] as DestinationConfig).mappings).toEqual(
      ((doc.channels[1] as MessageChannelConfig).destinations[0] as DestinationConfig).mappings,
    );
  });

  it("shows the steps of a rule in order and offers each step's own fields", async () => {
    await open();
    const rule = group("Destination 1 mapping rule 2");
    expect(within(rule).getByRole("group", { name: "Text steps of destination 1 mapping rule 2" }));
    const first = group("Step 1 of destination 1 mapping rule 2");
    expect(value(first, /^Characters/)).toBe("0");
    const second = group("Step 2 of destination 1 mapping rule 2");
    expect(value(second, /^Separator/)).toBe("/");
    expect(value(second, /^Part number/)).toBe("-1");
  });

  it("adds a step and a second operand, and writes them as the contract has them", async () => {
    const { api, user } = await open();
    const rule = group("Destination 1 mapping rule 2");
    await user.click(
      within(rule).getByRole("button", { name: "Add text step to destination 1 mapping rule 2" }),
    );
    const step = group("Step 3 of destination 1 mapping rule 2");
    await pick(user, step, /Step/, "Replace text");
    await user.type(textbox(step, /^Find/), "-");
    const channel = await validatedChannel(user, api);
    const copy = (channel.destinations[0] as DestinationConfig).mappings[1];
    expect(copy).toMatchObject({
      op: "copy",
      steps: [
        { step: "strip_leading", chars: "0" },
        { step: "split", separator: "/", take: -1 },
        { step: "replace", find: "-", with: "" },
      ],
    });
  });

  it("explains a formula that names an operand nobody defined", async () => {
    const { user } = await open();
    const calculate = group("Destination 1 appended rule 1.1");
    await user.clear(textbox(calculate, /^Formula/));
    await user.type(textbox(calculate, /^Formula/), "@na - @k");
    await user.click(screen.getByRole("button", { name: "Validate" }));
    expect(await within(calculate).findByText(/^No operand is named @k/)).toBeTruthy();
  });

  it("allows writing the value unchanged only for one operand", async () => {
    const { user } = await open();
    const calculate = group("Destination 1 appended rule 1.1");
    await pick(user, calculate, /When it cannot be calculated/, "Write the value unchanged");
    await user.click(screen.getByRole("button", { name: "Validate" }));
    expect(
      await within(calculate).findByText(
        "Writing the value unchanged needs a formula with exactly one operand.",
      ),
    ).toBeTruthy();
  });

  it("offers only what an appended record can hold", async () => {
    const { user } = await open();
    await user.click(
      screen.getByRole("button", {
        name: "Add rule to the record added by destination 1 mapping rule 1",
      }),
    );
    await user.click(
      within(group("Destination 1 appended rule 1.4")).getByRole("button", { name: /Rule/ }),
    );
    expect(screen.getAllByRole("option").map((option) => option.textContent)).toEqual([
      "copy",
      "constant",
      "lookup",
      "concat",
      "datetime",
      "calculate",
      "sequence",
    ]);
  });

  it("has no accessibility violations", async () => {
    await open();
    await expectNoA11yViolations(document.body);
  });
});

describe("combined conditions", () => {
  function filteredDocument(): ConfigDocument {
    const doc = sampleConfig();
    const channel = doc.channels[1] as MessageChannelConfig;
    const filters: FilterRule[] = [
      {
        id: "f1",
        not: {
          any: [
            { path: "O.3", operator: "starts_with", value: "CTL", ignoreCase: true },
            { path: "O.3", operator: "starts_with", value: "QC", ignoreCase: true },
          ],
        },
      },
    ];
    channel.filters = filters;
    return doc;
  }

  it("shows a negated combination and writes it back", async () => {
    const doc = filteredDocument();
    const { api, user } = await open(doc);
    const filter = group("Source filter 1");
    expect(within(filter).getByRole("button", { name: /This does not hold/ })).toBeTruthy();
    const inner = group("Not source filter 1");
    expect(within(inner).getByRole("button", { name: /Any of these holds/ })).toBeTruthy();
    expect(value(group("Condition 2 of negated condition of source filter 1"), /^Value/)).toBe(
      "QC",
    );
    const channel = await validatedChannel(user, api);
    expect(channel.filters).toEqual((doc.channels[1] as MessageChannelConfig).filters);
  });

  it("combines a comparison with another by wrapping it", async () => {
    const doc = sampleConfig();
    (doc.channels[1] as MessageChannelConfig).filters = [];
    const { api, user } = await open(doc);
    await user.click(screen.getByRole("button", { name: "Add source filter" }));
    const filter = group("Source filter 1");
    await user.type(textbox(filter, /^Path/), "PID.8");
    await user.type(textbox(filter, /^Value/), "F");
    await pick(user, filter, /Condition/, "All of these hold");
    // The comparison typed so far is the first condition of the combination.
    expect(value(group("Condition 1 of source filter 1"), /^Path/)).toBe("PID.8");
    await user.click(screen.getByRole("button", { name: "Add condition to source filter 1" }));
    const second = group("Condition 2 of source filter 1");
    await user.type(textbox(second, /^Path/), "PV1.2");
    await user.type(textbox(second, /^Value/), "I");
    const channel = await validatedChannel(user, api);
    expect(channel.filters[0]).toMatchObject({
      all: [
        { path: "PID.8", operator: "equals", value: "F" },
        { path: "PV1.2", operator: "equals", value: "I" },
      ],
    });
  });
});

describe("rewriting in place and structured numerics", () => {
  /** HL7 in, HL7 out: every OBX converted where it is, the way the contract's recipe has it. */
  function sameProtocolDocument(): ConfigDocument {
    const doc = sampleConfig();
    const channel = doc.channels[1] as MessageChannelConfig;
    channel.source.protocol = "hl7_mllp";
    channel.source.settings = { ackMode: "original" };
    const destination = channel.destinations[0] as DestinationConfig;
    destination.mappings = [
      {
        id: "m1",
        op: "for_each",
        over: "OBX",
        into: "OBX",
        mode: "update",
        rules: [
          {
            id: "m2",
            op: "calculate",
            formula: { op: "mul", args: [{ from: "$.5", steps: [] }, { constant: "0.0555" }] },
            to: "$.5",
            scale: 2,
            rounding: "half_up",
            comparator: "carry",
            notation: "sn",
            onInvalid: "keep",
            default: "",
            required: false,
          },
        ],
      },
    ];
    return doc;
  }

  it("shows the loop's mode and the notation, and writes the same document back", async () => {
    const doc = sameProtocolDocument();
    const { api, user } = await open(doc);
    const loop = group("Destination 1 mapping rule 1");
    expect(
      within(loop).getByRole("button", { name: /Rewrite each record found in place/ }),
    ).toBeTruthy();
    // Into is the records looped over, so it is not asked for.
    expect(within(loop).queryByRole("textbox", { name: /^Into/ })).toBeNull();
    const calculate = group("Destination 1 loop rule 1.1");
    expect(within(calculate).getByRole("button", { name: /HL7 structured numeric/ })).toBeTruthy();
    const channel = await validatedChannel(user, api);
    expect((channel.destinations[0] as DestinationConfig).mappings).toEqual(
      ((doc.channels[1] as MessageChannelConfig).destinations[0] as DestinationConfig).mappings,
    );
  });

  it("offers the structured numeric and in-place rewriting only where they apply", async () => {
    await open();
    const calculate = group("Destination 1 appended rule 1.1");
    expect(within(calculate).queryByRole("button", { name: /Number written as/ })).toBeNull();
    const loop = group("Destination 1 mapping rule 2");
    expect(loop).toBeTruthy();
  });

  it("stops an in-place rewrite where the destination is of another protocol", async () => {
    const doc = sameProtocolDocument();
    ((doc.channels[1] as MessageChannelConfig).destinations[0] as DestinationConfig).protocol =
      "astm";
    const { user } = await open(doc);
    await user.click(screen.getByRole("button", { name: "Validate" }));
    expect(
      await within(group("Destination 1 mapping rule 1")).findByText(
        /^Rewriting in place needs the same protocol/,
      ),
    ).toBeTruthy();
  });
});

describe("source requirements and repeated fields", () => {
  it("edits the requirements of a destination and checks their limits", async () => {
    const { api, user } = await open();
    await user.click(screen.getByRole("button", { name: "Add requirement to destination 1" }));
    const first = group("Requirement 1 of destination 1");
    await user.type(textbox(first, /^Record or segment/), "OBR");
    await user.type(textbox(first, /^Minimum/), "5");
    await user.type(textbox(first, /^Maximum/), "2");
    await user.click(screen.getByRole("button", { name: "Validate" }));
    expect(await within(first).findAllByText("The minimum is more than the maximum.")).toBeTruthy();
    await user.clear(textbox(first, /^Maximum/));
    await user.type(textbox(first, /^Maximum/), "18");
    await user.click(screen.getByRole("button", { name: "Add requirement to destination 1" }));
    const second = group("Requirement 2 of destination 1");
    await pick(user, second, /Check/, "Same value in every record");
    await user.type(textbox(second, /^Path/), "OBR.3.1");
    const channel = await validatedChannel(user, api);
    expect((channel.destinations[0] as DestinationConfig).require).toEqual([
      { count: "OBR", min: 5, max: 18 },
      { same: "OBR.3.1" },
    ]);
  });

  it("writes each record as a repetition of one field", async () => {
    const doc = sampleConfig();
    const channel = doc.channels[1] as MessageChannelConfig;
    channel.source.protocol = "hl7_mllp";
    channel.source.settings = { ackMode: "original" };
    (channel.destinations[0] as DestinationConfig).protocol = "astm";
    (channel.destinations[0] as DestinationConfig).settings = { astmRole: "computer" };
    (channel.destinations[0] as DestinationConfig).mappings = [
      {
        id: "m1",
        op: "for_each",
        over: "OBR",
        into: "O.5",
        mode: "repeat",
        rules: [{ id: "m2", op: "copy", from: "$.4.1", to: "$.5.4", default: "", required: true }],
      },
    ];
    const { api, user } = await open(doc);
    const loop = group("Destination 1 mapping rule 1");
    expect(
      within(loop).getByRole("button", { name: /Write each one as a repetition/ }),
    ).toBeTruthy();
    expect((textbox(loop, /^Into field/) as HTMLInputElement).value).toBe("O.5");
    const written = await validatedChannel(user, api);
    expect((written.destinations[0] as DestinationConfig).mappings).toEqual(
      (channel.destinations[0] as DestinationConfig).mappings,
    );
  });
});
