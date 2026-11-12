import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import {
  addCopyRule,
  channelLabel,
  fillDestination,
  group,
  openNewChannel,
  openResults,
  optionsOf,
  pick,
  pickEndpoint,
  validatedChannel,
} from "./testActions";
import { stubApi, validationFails } from "./testFixtures";

describe("destination: its own filters, mapping and policies", () => {
  it("has them in the destination and none at the channel level", async () => {
    await openResults();
    expect(screen.queryByRole("group", { name: "Mappings" })).toBeNull();
    expect(screen.queryByRole("group", { name: "Code tables" })).toBeNull();
    expect(screen.getByRole("group", { name: "Source filters" })).toBeTruthy();
    const destination = group("Destination 1");
    expect(within(destination).getByRole("textbox", { name: /^Label/ })).toBeTruthy();
    expect(within(destination).getByRole("checkbox", { name: "Enabled" })).toBeTruthy();
    for (const legend of [
      "Destination 1 filter rules",
      "Destination 1 mapping rules",
      "Destination 1 code tables",
    ]) {
      expect(within(destination).getByRole("group", { name: legend })).toBeTruthy();
    }
    expect(within(destination).getByText(/Retry policy of destination 1/)).toBeTruthy();
    expect(
      within(destination).getByText(/Leave empty to pass the original message through unchanged/),
    ).toBeTruthy();
    expect(within(destination).getByText(/protocol must then be the source's/)).toBeTruthy();
  });

  it("keeps each destination's mapping apart and sends it in the document to validate", async () => {
    const { api, user } = await openResults();
    await user.click(screen.getByRole("button", { name: "Add destination" }));
    await fillDestination(user, "Archive", /^Chemistry serial/, "ASTM", 2);
    await user.click(screen.getByRole("button", { name: "Add destination 2 mapping rule" }));
    const rule = group("Destination 2 mapping rule 1");
    await user.type(within(rule).getByRole("textbox", { name: /^From/ }), "R.4");
    await user.type(within(rule).getByRole("textbox", { name: /^To$/ }), "R.5");
    await user.click(screen.getByRole("button", { name: "Add destination 2 code table" }));
    await user.type(
      within(group("Destination 2 table 1")).getByRole("textbox", { name: /^Table name/ }),
      "flags",
    );
    // The first destination still has exactly its own two rules.
    expect(screen.getAllByRole("group", { name: /^Destination 1 mapping rule \d/ })).toHaveLength(
      2,
    );
    const channel = await validatedChannel(user, api);
    expect(channel.destinations[0]?.mappings.map((m) => m.id)).toEqual(["m1", "m2"]);
    expect(channel.destinations[0]?.codeTables).toEqual({});
    expect(channel.destinations[1]?.mappings).toEqual([
      { id: "m1", op: "copy", from: "R.4", to: "R.5", default: "", required: false },
    ]);
    expect(channel.destinations[1]?.codeTables).toEqual({ flags: {} });
    expect("mappings" in channel).toBe(false);
    expect("codeTables" in channel).toBe(false);
  });

  it("reads a lookup's table from its own destination's code tables", async () => {
    const { user } = await openResults();
    await user.click(screen.getByRole("button", { name: "Add destination" }));
    await user.click(screen.getByRole("button", { name: "Add destination 1 code table" }));
    await user.type(
      within(group("Destination 1 table 1")).getByRole("textbox", { name: /^Table name/ }),
      "units",
    );
    await user.click(screen.getByRole("button", { name: "Add destination 2 mapping rule" }));
    const rule = group("Destination 2 mapping rule 1");
    await pick(user, rule, "Rule", "lookup");
    expect(
      within(rule).getByText("Add a code table to this destination below first."),
    ).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Add destination 1 mapping rule" }));
    const first = group("Destination 1 mapping rule 3");
    await pick(user, first, "Rule", "lookup");
    expect(await optionsOf(user, first, "Table")).toEqual(["units"]);
  });

  it("edits destination filters, including their own engine findings", async () => {
    const api = stubApi(
      validationFails({
        severity: "error",
        path: "channels[1].destinations[0].filters[0].path",
        message: "Unknown segment XYZ.",
      }),
    );
    const { user } = await openResults(api);
    await user.click(screen.getByRole("button", { name: "Add destination 1 filter" }));
    const filter = group("Destination 1 filter 1");
    await user.type(within(filter).getByRole("textbox", { name: /^Path/ }), "XYZ.1");
    await user.type(within(filter).getByRole("textbox", { name: /^Value/ }), "x");
    const channel = await validatedChannel(user, api);
    expect(await within(filter).findByText("Unknown segment XYZ.")).toBeTruthy();
    expect(channel.destinations[0]?.filters).toEqual([
      { id: "f1", path: "XYZ.1", operator: "equals", value: "x", ignoreCase: false },
    ]);
    expect(channel.filters).toHaveLength(1);
  });

  it("can disable a destination", async () => {
    const { api, user } = await openResults();
    const enabled = within(group("Destination 1")).getByRole("checkbox", { name: "Enabled" });
    expect((enabled as HTMLInputElement).checked).toBe(true);
    expect(screen.getByText(/A disabled destination gets no new deliveries/)).toBeTruthy();
    await user.click(enabled);
    expect((await validatedChannel(user, api)).destinations[0]?.enabled).toBe(false);
  });

  it("puts an engine finding on a destination's nested mapping rule", async () => {
    const api = stubApi(
      validationFails({
        severity: "error",
        path: "channels[1].destinations[0].mappings[1].value",
        message: "Not a valid value.",
      }),
    );
    const { user } = await openResults(api);
    await user.click(screen.getByRole("button", { name: "Validate" }));
    const rule = group("Destination 1 mapping rule 2");
    expect(await within(rule).findByText("Not a valid value.")).toBeTruthy();
  });

  it("has no accessibility violations with two destinations", async () => {
    const { user, container } = await openResults();
    await user.click(screen.getByRole("button", { name: "Add destination" }));
    await expectNoA11yViolations(container);
  });
});

describe("destination: pass-through needs the source's protocol", () => {
  const MESSAGE = (label: string) =>
    `Without a mapping, this destination must use the source's protocol (${label}). Choose that protocol or add a mapping.`;
  const ASTM_MESSAGE = MESSAGE("ASTM");

  /** A new message channel with an ASTM source and a destination that still has no protocol. */
  async function openAstmSource() {
    const rendered = await openNewChannel("Message", stubApi());
    await rendered.user.type(channelLabel(), "Orders");
    const source = group("Source");
    await pickEndpoint(rendered.user, source, "Endpoint", /^Chemistry serial/);
    await pick(rendered.user, source, "Protocol", "ASTM");
    const destination = await fillDestination(rendered.user, "LIS", /^LIS host/);
    return { source, destination, ...rendered };
  }

  it("says nothing until the destination has a protocol, then names the source's", async () => {
    const { destination, user } = await openAstmSource();
    expect(within(destination).queryByText(ASTM_MESSAGE)).toBeNull();
    expect(screen.queryByText("Choose protocol.")).toBeNull();
    await pick(user, destination, "Protocol", "HL7 v2");
    expect(await within(destination).findByText(ASTM_MESSAGE)).toBeTruthy();
  });

  it("goes away when a mapping rule or a code table is added and returns when it is removed", async () => {
    const { destination, user } = await openAstmSource();
    await pick(user, destination, "Protocol", "HL7 v2");
    expect(await within(destination).findByText(ASTM_MESSAGE)).toBeTruthy();

    await addCopyRule(user);
    await waitFor(() => expect(within(destination).queryByText(ASTM_MESSAGE)).toBeNull());
    await user.click(screen.getByRole("button", { name: "Remove destination 1 mapping rule 1" }));
    expect(await within(destination).findByText(ASTM_MESSAGE)).toBeTruthy();

    await user.click(screen.getByRole("button", { name: "Add destination 1 code table" }));
    await waitFor(() => expect(within(destination).queryByText(ASTM_MESSAGE)).toBeNull());
    await user.click(screen.getByRole("button", { name: "Remove destination 1 table 1" }));
    expect(await within(destination).findByText(ASTM_MESSAGE)).toBeTruthy();
  });

  it("follows the destination's protocol and the source's", async () => {
    const { source, destination, user } = await openAstmSource();
    await pick(user, destination, "Protocol", "HL7 v2");
    expect(await within(destination).findByText(ASTM_MESSAGE)).toBeTruthy();

    await pick(user, destination, "Protocol", "ASTM");
    await waitFor(() => expect(within(destination).queryByText(ASTM_MESSAGE)).toBeNull());

    // The source moves away from the destination's protocol: the destination is wrong again.
    await pickEndpoint(user, source, "Endpoint", /^LIS host/);
    await pick(user, source, "Protocol", "HL7 v2");
    expect(await within(destination).findByText(MESSAGE("HL7 v2"))).toBeTruthy();
    await pick(user, destination, "Protocol", "HL7 v2");
    await waitFor(() => expect(within(destination).queryByText(MESSAGE("HL7 v2"))).toBeNull());
  });

  it("is per destination", async () => {
    const { destination, user } = await openAstmSource();
    await pick(user, destination, "Protocol", "HL7 v2");
    await user.click(screen.getByRole("button", { name: "Add destination" }));
    const second = group("Destination 2");
    await pickEndpoint(user, second, "Endpoint", /^Chemistry serial/);
    await pick(user, second, "Protocol", "ASTM");
    expect(await within(destination).findByText(ASTM_MESSAGE)).toBeTruthy();
    expect(within(second).queryByText(ASTM_MESSAGE)).toBeNull();
    await addCopyRule(user, 1);
    await waitFor(() => expect(within(destination).queryByText(ASTM_MESSAGE)).toBeNull());
  });

  it("blocks Validate and Save and apply, and lets both through once mapped", async () => {
    const { api, destination, user } = await openAstmSource();
    await pick(user, destination, "Protocol", "HL7 v2");
    await user.click(screen.getByRole("button", { name: "Validate" }));
    expect(await within(destination).findByText(ASTM_MESSAGE)).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Save and apply" }));
    expect(await within(destination).findByText(ASTM_MESSAGE)).toBeTruthy();
    expect(api.validateConfig).not.toHaveBeenCalled();
    expect(api.applyConfig).not.toHaveBeenCalled();

    await addCopyRule(user);
    await user.click(screen.getByRole("button", { name: "Validate" }));
    await waitFor(() => expect(api.validateConfig).toHaveBeenCalledTimes(1));
  });

  it("still shows the engine's own finding on the destination's protocol", async () => {
    const api = stubApi(
      validationFails({
        severity: "error",
        path: "channels[1].destinations[0].protocol",
        message: "The engine does not accept this protocol here.",
      }),
    );
    const { user } = await openResults(api);
    await user.click(screen.getByRole("button", { name: "Validate" }));
    expect(
      await within(group("Destination 1")).findByText(
        "The engine does not accept this protocol here.",
      ),
    ).toBeTruthy();
  });
});
