import { screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type {
  ConfigDocument,
  DestinationConfig,
  MappingRule,
  MessageChannelConfig,
} from "@/api/types";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { group, openResults, pick, textbox, validatedChannel } from "./testActions";
import { sampleConfig, stubApi } from "./testFixtures";

/** A loop converting each result, and a top-level calculation with a nested formula. */
function calculateDocument(): ConfigDocument {
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
        {
          id: "m2",
          op: "calculate",
          formula: { op: "mul", args: [{ from: "$.4", steps: [] }, { constant: "0.0555" }] },
          to: "$.5",
          scale: 2,
          rounding: "half_even",
          comparator: "carry",
          notation: "text",
          onInvalid: "skip",
          default: "",
          required: false,
        },
      ],
    },
    {
      id: "m3",
      op: "calculate",
      formula: {
        op: "div",
        args: [
          {
            op: "add",
            args: [
              { from: "OBX[1].5", steps: [] },
              { from: "OBX[2].5", steps: [] },
            ],
          },
          { constant: "2" },
        ],
      },
      to: "OBX[3].5",
      scale: 1,
      rounding: "half_up",
      comparator: "reject",
      notation: "text",
      onInvalid: "default",
      default: "n/a",
      required: true,
    },
  ];
  destination.mappings = mappings;
  return doc;
}

const open = (doc: ConfigDocument = calculateDocument()) =>
  openResults(stubApi({ getConfig: async () => doc }));

const value = (scope: HTMLElement, name: RegExp) =>
  (textbox(scope, name) as HTMLInputElement).value;

describe("calculate rule", () => {
  it("shows the formula as text with how it reads, and writes the same document back", async () => {
    const doc = calculateDocument();
    const { api, user } = await open(doc);
    const loopRule = group("Destination 1 loop rule 1.1");
    expect(value(loopRule, /^Formula/)).toBe("$.4 * 0.0555");
    expect(value(loopRule, /^Decimal places/)).toBe("2");
    expect(within(loopRule).getByText(/^Reads as \$\.4 × 0\.0555\./)).toBeTruthy();
    expect(within(loopRule).getByText(/Takes \$\.4 from the message\./)).toBeTruthy();
    const top = group("Destination 1 mapping rule 2");
    expect(value(top, /^Formula/)).toBe("(OBX[1].5 + OBX[2].5) / 2");
    expect(value(top, /^Default/)).toBe("n/a");
    const channel = await validatedChannel(user, api);
    expect((channel.destinations[0] as DestinationConfig).mappings).toEqual(
      ((doc.channels[1] as MessageChannelConfig).destinations[0] as DestinationConfig).mappings,
    );
  });

  it("builds a calculation from the fields, with rounding only once places are set", async () => {
    const { api, user } = await open();
    await user.click(screen.getByRole("button", { name: "Add destination 1 mapping rule" }));
    const rule = group("Destination 1 mapping rule 3");
    await pick(user, rule, /Rule/, "calculate");
    expect(
      within(rule).getByText(/^Calculates a number with exact decimal arithmetic/),
    ).toBeTruthy();
    expect(
      within(rule)
        .getByRole("button", { name: /Rounding/ })
        .hasAttribute("disabled"),
    ).toBe(true);
    await user.type(textbox(rule, /^Formula/), "max(R.4, 0) - 1");
    await user.type(textbox(rule, /^To$/), "OBX.5");
    await user.type(textbox(rule, /^Decimal places/), "3");
    await pick(user, rule, /Rounding/, /^Truncate/);
    await pick(user, rule, /When it cannot be calculated/, "Write nothing");
    const channel = await validatedChannel(user, api);
    const added = (channel.destinations[0] as DestinationConfig).mappings[2];
    expect(added).toMatchObject({
      op: "calculate",
      formula: {
        op: "sub",
        args: [
          { op: "max", args: [{ from: "R.4", steps: [] }, { constant: "0" }] },
          { constant: "1" },
        ],
      },
      to: "OBX.5",
      scale: 3,
      rounding: "truncate",
      onInvalid: "skip",
    });
  });

  it("says what is wrong with a formula at its field, and asks for the default it needs", async () => {
    const { user } = await open();
    const rule = group("Destination 1 mapping rule 2");
    await user.clear(textbox(rule, /^Formula/));
    await user.type(textbox(rule, /^Formula/), "R.4 *");
    await user.click(screen.getByRole("button", { name: "Validate" }));
    expect(
      await within(rule).findByText(/^Expected a number, a path or a bracket at the end/),
    ).toBeTruthy();
    await user.clear(textbox(rule, /^Decimal places/));
    await user.type(textbox(rule, /^Decimal places/), "13");
    await user.click(screen.getByRole("button", { name: "Validate" }));
    expect(await within(rule).findByText("Enter a whole number from 0 to 12.")).toBeTruthy();
  });

  it("has no accessibility violations", async () => {
    await open();
    await expectNoA11yViolations(document.body);
  });
});
