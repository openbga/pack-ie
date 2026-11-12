import { screen, waitFor, within } from "@testing-library/react";
import type { UserEvent } from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { MockEngineApi } from "@/api/mock";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import {
  addCopyRule,
  channelLabel,
  fillDestination,
  group,
  openNewChannel,
  optionsOf,
  pick,
  pickEndpoint,
  textbox,
  typeNote,
  validatedChannel,
} from "./testActions";
import { stubApi, validationFails } from "./testFixtures";

/** A new message channel whose source is an AU instrument on the given endpoint. */
async function openNewAu(api: ReturnType<typeof stubApi> | MockEngineApi, endpoint: RegExp) {
  const rendered = await openNewChannel("Message", api);
  await rendered.user.type(channelLabel(), "AU line");
  const source = group("Source");
  await pickEndpoint(rendered.user, source, "Endpoint", endpoint);
  await pick(rendered.user, source, "Protocol", "AU");
  return { ...rendered, source, au: group("AU instrument settings", source) };
}

/** The demo instrument's parameter sheet: synthetic values, as in the mock catalog. */
async function fillAu(user: UserEvent, au: HTMLElement) {
  await pick(user, au, "Profile", "AU640");
  await user.type(textbox(au, /^Start bytes/), "2");
  await user.type(textbox(au, /^End bytes/), "3");
  await user.click(within(au).getByRole("checkbox", { name: "Check byte (BCC)" }));
  await pick(user, au, "Maximum wire length", "512");
  await pick(user, au, "Class", "Class b");
  await pick(user, au, "Rack digits", "4");
  await user.type(textbox(au, /^Sample ID digits/), "10");
  await user.type(textbox(au, /^Patient information 1 digits/), "10");
  for (const n of [2, 3, 4, 5, 6]) {
    await user.type(textbox(au, new RegExp(`^Patient information ${n} digits`)), "0");
  }
  await pick(user, au, "Result digits", "6");
  await user.type(textbox(au, /^Frame timeout/), "1000");
  await user.type(textbox(au, /^Block timeout/), "3000");
}

const FILLED_AU = {
  profile: "au640",
  framingStart: [2],
  framingEnd: [3],
  bcc: true,
  useEtb: false,
  maxWireLen: 512,
  framingClass: "b",
  unitNo: false,
  rackDigits: 4,
  sampleIdDigits: 10,
  sex: false,
  age: false,
  month: false,
  patientInfoDigits: [10, 0, 0, 0, 0, 0],
  resultDigits: 6,
  frameTimeoutMs: 1000,
  blockTimeoutMs: 3000,
};

describe("AU instrument settings in the connector", () => {
  it("shows every field of the spec, grouped, with the parameter sheet note, and nothing preset", async () => {
    const { au } = await openNewAu(stubApi(), /^Chemistry serial/);
    expect(
      within(au).getByText(
        "Copy these from the instrument's online parameter sheet; there are no defaults.",
      ),
    ).toBeTruthy();
    for (const name of ["Instrument", "Framing", "Message format", "Timing"]) {
      expect(group(name, au)).toBeTruthy();
    }
    expect(within(group("Instrument", au)).getByRole("button", { name: /Profile/ })).toBeTruthy();
    const framing = group("Framing", au);
    for (const label of [/^Start bytes/, /^End bytes/]) {
      expect((textbox(framing, label) as HTMLInputElement).value).toBe("");
    }
    for (const label of ["Check byte (BCC)", "ETB between blocks"]) {
      const box = within(framing).getByRole("checkbox", { name: label }) as HTMLInputElement;
      expect(box.checked).toBe(false);
    }
    expect(within(framing).getByRole("button", { name: /Maximum wire length/ })).toBeTruthy();
    expect(within(framing).getByRole("button", { name: /Class/ })).toBeTruthy();
    expect(
      within(framing).getByText(
        "Class a: no ACK or NAK when receiving. Class b: ACK after the message is stored.",
      ),
    ).toBeTruthy();
    const format = group("Message format", au);
    for (const label of ["Unit number", "Sex", "Age", "Month"]) {
      expect(within(format).getByRole("checkbox", { name: label })).toBeTruthy();
    }
    for (const label of [/Rack digits/, /Result digits/]) {
      expect(within(format).getByRole("button", { name: label })).toBeTruthy();
    }
    expect(textbox(format, /^Sample ID digits/)).toBeTruthy();
    for (const n of [1, 2, 3, 4, 5, 6]) {
      expect(textbox(format, new RegExp(`^Patient information ${n} digits`))).toBeTruthy();
    }
    const timing = group("Timing", au);
    expect(textbox(timing, /^Frame timeout/)).toBeTruthy();
    expect(textbox(timing, /^Block timeout/)).toBeTruthy();
    // Unselected: every choice still asks to be made.
    expect(within(au).getAllByText("Select…").length).toBe(5);
  });

  it("has no accessibility violations", async () => {
    const { container } = await openNewAu(stubApi(), /^Chemistry serial/);
    await expectNoA11yViolations(container);
  });

  it("offers the choices the spec allows", async () => {
    const { user, au } = await openNewAu(stubApi(), /^Chemistry serial/);
    const options = (label: string) => optionsOf(user, au, label);
    expect(await options("Profile")).toEqual(["AU400", "AU640"]);
    expect(await options("Maximum wire length")).toEqual(["256", "512", "1024"]);
    expect(await options("Class")).toEqual(["Class a", "Class b"]);
    expect(await options("Rack digits")).toEqual(["0", "4", "5"]);
    expect(await options("Result digits")).toEqual(["6", "9"]);
  });

  it("refuses an empty form field by field before calling the engine", async () => {
    const api = stubApi();
    const { user, au } = await openNewAu(api, /^Chemistry serial/);
    await user.click(screen.getByRole("button", { name: "Save and apply" }));
    for (const message of [
      "Choose profile.",
      "Enter start bytes.",
      "Enter end bytes.",
      "Choose maximum wire length.",
      "Choose class.",
      "Choose rack digits.",
      "Enter sample id digits.",
      "Enter patient information 1 digits.",
      "Choose result digits.",
      "Enter frame timeout (ms).",
    ]) {
      expect(await within(au).findByText(message)).toBeTruthy();
    }
    expect(api.validateConfig).not.toHaveBeenCalled();
  });

  it.each([
    [/^Start bytes/, "6", "Control bytes may not be 6 (ACK) or 21 (NAK)."],
    [/^Start bytes/, "1,2,3", /^Enter one or two whole numbers/],
    [/^End bytes/, "32", "Each control byte is from 1 to 31."],
    [/^End bytes/, "x", /^Enter one or two whole numbers/],
    [/^Sample ID digits/, "0", "Enter a whole number from 1 to 20."],
    [/^Sample ID digits/, "21", "Enter a whole number from 1 to 20."],
    [/^Patient information 3 digits/, "21", "Enter a whole number from 0 to 20."],
    [/^Frame timeout/, "50", "Enter 100 to 9900 milliseconds, in steps of 100."],
    [/^Block timeout/, "1250", "Enter 100 to 9900 milliseconds, in steps of 100."],
    [/^Block timeout/, "10000", "Enter 100 to 9900 milliseconds, in steps of 100."],
  ])("checks %s against the spec: %j", async (label, typed, message) => {
    const api = stubApi();
    const { user, au } = await openNewAu(api, /^Chemistry serial/);
    await user.type(textbox(au, label), typed);
    await user.click(screen.getByRole("button", { name: "Save and apply" }));
    expect(await within(au).findByText(message)).toBeTruthy();
    expect(api.validateConfig).not.toHaveBeenCalled();
  });

  it("allows ETB only when the end bytes are 3", async () => {
    const api = stubApi();
    const { user, au } = await openNewAu(api, /^Chemistry serial/);
    await user.type(textbox(au, /^End bytes/), "13, 10");
    await user.click(within(au).getByRole("checkbox", { name: "ETB between blocks" }));
    await user.click(screen.getByRole("button", { name: "Save and apply" }));
    expect(
      await within(au).findByText(/ETB can only be used when the end bytes are 3/),
    ).toBeTruthy();
    await user.clear(textbox(au, /^End bytes/));
    await user.type(textbox(au, /^End bytes/), "3");
    await user.click(screen.getByRole("button", { name: "Save and apply" }));
    await waitFor(() =>
      expect(within(au).queryByText(/ETB can only be used when the end bytes are 3/)).toBeNull(),
    );
  });

  it("sends the filled settings as numbers and byte lists", async () => {
    const api = stubApi();
    const { user, au } = await openNewAu(api, /^Chemistry serial/);
    await fillAu(user, au);
    await fillDestination(user, "LIS", /^LIS host/, "HL7 v2");
    await addCopyRule(user);
    const channel = await validatedChannel(user, api, 2);
    expect(channel.source.settings).toEqual({ au: FILLED_AU });
  });

  it("leaves the block timeout out of the settings when it is blank", async () => {
    const api = stubApi();
    const { user, au } = await openNewAu(api, /^Chemistry serial/);
    await fillAu(user, au);
    await user.clear(textbox(au, /^Block timeout/));
    await fillDestination(user, "LIS", /^LIS host/, "HL7 v2");
    await addCopyRule(user);
    const channel = await validatedChannel(user, api, 2);
    const { blockTimeoutMs: _default, ...withoutBlock } = FILLED_AU;
    expect(channel.source.settings).toEqual({ au: withoutBlock });
  });

  it("shows the engine's finding on the AU field it names", async () => {
    const api = stubApi(
      validationFails(
        {
          severity: "error",
          path: "channels[2].source.settings.au.rackDigits",
          message: "Rack digits must be 0, 4 or 5.",
        },
        {
          severity: "error",
          path: "channels[2].source.settings.au.framingStart",
          message: "Use one or two control bytes.",
        },
      ),
    );
    const { user, au } = await openNewAu(api, /^Chemistry serial/);
    await fillAu(user, au);
    await fillDestination(user, "LIS", /^LIS host/, "HL7 v2");
    await addCopyRule(user);
    await user.click(screen.getByRole("button", { name: "Validate" }));
    expect(await within(au).findByText("Rack digits must be 0, 4 or 5.")).toBeTruthy();
    expect(within(au).getByText("Use one or two control bytes.")).toBeTruthy();
  });
});

describe("creating an AU connector against the mock engine", () => {
  it("refuses the empty form, then validates, explains the mapping, and applies", async () => {
    const api = new MockEngineApi({ latency: "none", messageCount: 500, seed: 7 });
    const before = await api.getConfig();
    const { user, router, au } = await openNewAu(api, /^cobas c 311 chem analyzer/);

    // Empty required fields: the form refuses, nothing reaches the engine.
    await user.click(screen.getByRole("button", { name: "Validate" }));
    expect(await within(au).findByText("Choose profile.")).toBeTruthy();
    expect(screen.queryByRole("region", { name: "Validation results" })).toBeNull();

    await fillAu(user, au);
    const destination = await fillDestination(user, "LIS", "LIS HL7 out", "HL7 v2");

    // An HL7 destination of an AU source needs a mapping: the form says so, the engine is not asked.
    const message =
      "Without a mapping, this destination must use the source's protocol (AU). Choose that protocol or add a mapping.";
    expect(await within(destination).findByText(message)).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Validate" }));
    expect(await within(destination).findByText(message)).toBeTruthy();
    expect(screen.queryByRole("region", { name: "Validation results" })).toBeNull();

    await addCopyRule(user);
    expect(within(destination).queryByText(message)).toBeNull();
    await user.click(screen.getByRole("button", { name: "Save and apply" }));
    await typeNote(user);
    await user.click(
      await screen.findByRole("button", { name: `Apply version ${before.version + 1}` }),
    );
    await waitFor(() => expect(router.state.location.pathname).toBe("/channels/au-line"));

    const applied = (await api.getConfig()).channels.find((channel) => channel.id === "au-line");
    expect(applied?.kind === "message" && applied.source).toEqual({
      id: "source",
      endpointId: "ep-c311",
      protocol: "au",
      settings: { au: FILLED_AU },
      duplicateDetection: true,
    });
    expect(applied?.kind === "message" && applied.destinations[0]?.mappings).toEqual([
      { id: "m1", op: "copy", from: "R.4", to: "OBX.5", default: "", required: false },
    ]);
  });
});
