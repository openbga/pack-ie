import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { DestinationConfig } from "@/api/types";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { group, openResults, pick, textbox, validatedChannel } from "../testActions";
import { stubApi } from "../testFixtures";

const open = () => openResults(stubApi());

describe("connector presets", () => {
  it("lists what a preset changes, then writes it into the connector's own field", async () => {
    const { api, user } = await open();
    const source = group("Source");
    expect((textbox(source, /^Characters per frame/) as HTMLInputElement).value).toBe("240");
    await user.click(within(source).getByRole("button", { name: /Instrument presets/ }));
    const dialog = await screen.findByRole("dialog", { name: "Instrument presets" });
    await pick(user, dialog, /Preset/, /long frames/i);
    expect(within(dialog).getByRole("row", { name: /Characters per frame/ }).textContent).toMatch(
      /240.*64000/,
    );
    await expectNoA11yViolations(dialog);
    await user.click(within(dialog).getByRole("button", { name: "Apply 1 setting" }));
    expect((textbox(source, /^Characters per frame/) as HTMLInputElement).value).toBe("64000");
    const channel = await validatedChannel(user, api);
    expect(channel.source.settings.maxFrameChars).toBe(64_000);
  });

  it("changes nothing when cancelled, and says so when everything is already set", async () => {
    const { user } = await open();
    const source = group("Source");
    await user.click(within(source).getByRole("button", { name: /Instrument presets/ }));
    await user.click(await screen.findByRole("button", { name: "Cancel" }));
    expect((textbox(source, /^Characters per frame/) as HTMLInputElement).value).toBe("240");
    await user.clear(textbox(source, /^Characters per frame/));
    await user.type(textbox(source, /^Characters per frame/), "64000");
    await user.click(within(source).getByRole("button", { name: /Instrument presets/ }));
    const dialog = await screen.findByRole("dialog", { name: "Instrument presets" });
    await pick(user, dialog, /Preset/, /long frames/i);
    expect(within(dialog).getByRole("button", { name: "Nothing to change" })).toBeTruthy();
    expect(within(dialog).getByText("already set")).toBeTruthy();
  });
});

describe("mapping presets", () => {
  it("adds the preset's rules after the operator's and keeps their own table names", async () => {
    const { api, user } = await open();
    await user.click(screen.getByRole("button", { name: "Start destination 1 from a preset" }));
    const dialog = await screen.findByRole("dialog", { name: "Mapping presets for destination 1" });
    expect(within(dialog).getByRole("status").textContent).toMatch(/^This adds \d+ rules?/);
    await expectNoA11yViolations(dialog);
    await user.click(within(dialog).getByRole("button", { name: "Add to the mapping" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(group("Destination 1 mapping rule 1")).toBeTruthy();
    expect(
      (textbox(group("Destination 1 mapping rule 1"), /^From$/) as HTMLInputElement).value,
    ).toBe("R.3");
    const channel = await validatedChannel(user, api);
    const mappings = (channel.destinations[0] as DestinationConfig).mappings;
    expect(mappings.length).toBeGreaterThan(2);
    expect(new Set(mappings.map((rule) => rule.id)).size).toBe(mappings.length);
    expect(mappings.slice(0, 2).map((rule) => rule.op)).toEqual(["copy", "constant"]);
  });

  it("replaces only after the operator chooses it, naming what goes", async () => {
    const { api, user } = await open();
    await user.click(screen.getByRole("button", { name: "Start destination 1 from a preset" }));
    const dialog = await screen.findByRole("dialog", { name: "Mapping presets for destination 1" });
    await pick(user, dialog, /You already have rules or tables/, /Replace my rules/);
    expect(within(dialog).getByRole("alert").textContent).toMatch(
      /This removes what you have here2 rules and 0 tables/,
    );
    await user.click(within(dialog).getByRole("button", { name: "Replace and apply" }));
    const channel = await validatedChannel(user, api);
    const mappings = (channel.destinations[0] as DestinationConfig).mappings;
    expect(
      mappings.find((rule) => rule.op === "constant" && "value" in rule && rule.value === "LAB"),
    ).toBeUndefined();
  });

  it("is not offered where no preset suits the protocols", async () => {
    const { user } = await open();
    await pick(user, group("Destination 1"), "Protocol", "ASTM");
    expect(screen.queryByRole("button", { name: "Start destination 1 from a preset" })).toBeNull();
  });
});
