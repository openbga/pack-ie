import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { renderRoutes } from "@/lib/test/router";
import {
  addCopyRule,
  applyAndRead,
  channelLabel,
  channelRoutes,
  fillDestination,
  followFinding,
  group,
  messageChannel,
  openNewChannel,
  openResults,
  pick,
  pickEndpoint,
  removeSavedDestination,
  textbox,
  typeNote,
  validatedChannel,
} from "./testActions";
import { stubApi, stubApiWithFiles, validationFails } from "./testFixtures";

const value = (name: RegExp, scope: HTMLElement = document.body) =>
  (textbox(scope, name) as HTMLInputElement).value;

describe("ChannelEditPage: message channel", () => {
  it("creates a message channel with a source and a destination", async () => {
    const { api, user } = await openNewChannel("Message", stubApi());
    await user.type(channelLabel(), "Orders");
    const source = group("Source");
    await pickEndpoint(user, source, "Endpoint", /^Analyzer link/);
    await pick(user, source, "Protocol", "ASTM");
    const destination = await fillDestination(user, "LIS out", /^LIS host/, "HL7 v2");
    await pick(user, destination, "Acknowledgement mode", "Enhanced");
    await addCopyRule(user);
    const applied = await applyAndRead(user, api);
    expect(messageChannel(applied, "orders")).toMatchObject({
      kind: "message",
      source: {
        id: "source",
        endpointId: "analyzer-in",
        protocol: "astm",
        settings: { astmRole: "computer" },
        duplicateDetection: true,
      },
      destinations: [
        {
          id: "d1",
          label: "LIS out",
          endpointId: "lis-out",
          protocol: "hl7_mllp",
          settings: { ackMode: "enhanced" },
          enabled: true,
          filters: [],
          mappings: [expect.objectContaining({ op: "copy", from: "R.4", to: "OBX.5" })],
          codeTables: {},
          retry: { initialMs: 2000, maxMs: 300_000, maxAttempts: 5 },
        },
      ],
      filters: [],
      retention: { days: null, maxMessages: null },
    });
  });

  it("shows the column template only for the delimited protocol, and checks it as JSON", async () => {
    const { api, user } = await openNewChannel("Message", stubApiWithFiles());
    await user.type(channelLabel(), "Files in");
    const source = group("Source");
    const template = () =>
      within(source).queryByRole("textbox", { name: /^Column template \(JSON\)/ });
    expect(template()).toBeNull();
    await pick(user, source, "Protocol", "Delimited files");
    let box = template() as HTMLTextAreaElement;
    expect(box).toBeTruthy();
    // The starter template is valid, and the message says so.
    expect(JSON.parse(box.value)).toMatchObject({ header_row: true });
    expect(within(source).getByText("Valid JSON object.")).toBeTruthy();
    expect(within(source).queryByRole("textbox", { name: /File name template/ })).toBeNull();
    await user.clear(box);
    await user.type(box, "[[1, 2]");
    expect(
      within(source).getByText("The template must be a JSON object, written with { }."),
    ).toBeTruthy();
    await user.clear(box);
    await user.type(box, "{{ nope");
    expect(within(source).getByText(/^Not valid JSON:/)).toBeTruthy();
    await pick(user, source, "Protocol", "HL7 v2");
    expect(template()).toBeNull();
    expect(within(source).getByRole("button", { name: /Acknowledgement mode/ })).toBeTruthy();
    await pick(user, source, "Protocol", "Delimited files");
    expect(within(source).getByText(/^Not valid JSON:/)).toBeTruthy();
    box = template() as HTMLTextAreaElement;
    await pickEndpoint(user, source, "Endpoint", /^Incoming folder/);
    await fillDestination(user, "LIS", /^LIS host/, "HL7 v2");
    await addCopyRule(user);
    await user.click(screen.getByRole("button", { name: "Save and apply" }));
    await waitFor(() => expect(box.getAttribute("aria-invalid")).toBe("true"));
    expect(within(source).getByText(/^Not valid JSON:/)).toBeTruthy();
    expect(api.validateConfig).not.toHaveBeenCalled();
    await user.clear(box);
    await user.type(box, "{{}");
    const applied = await applyAndRead(user, api);
    expect(messageChannel(applied, "files-in")?.source).toEqual({
      id: "source",
      endpointId: "incoming-folder",
      protocol: "delimited_file",
      settings: { templateJson: "{}" },
      duplicateDetection: true,
    });
  });

  it("adds, reorders and removes mapping rules with buttons", async () => {
    const { api, user } = await openResults();
    const rules = () => screen.getAllByRole("group", { name: /^Destination 1 mapping rule \d/ });
    expect(rules()).toHaveLength(2);
    expect(value(/^To$/, rules()[0])).toBe("OBX.3");
    // The fields follow the operator: a constant has a value, no source.
    expect(within(rules()[1] as HTMLElement).queryByRole("textbox", { name: /^From/ })).toBeNull();
    expect(value(/^Value/, rules()[1])).toBe("LAB");
    await user.click(
      screen.getByRole("button", { name: "Move destination 1 mapping rule 1 down" }),
    );
    expect(value(/^To$/, rules()[0])).toBe("MSH.4");
    expect(screen.getByText("Destination 1 mapping rule 1 moved to position 2 of 2.")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Move destination 1 mapping rule 2 up" }));
    expect(value(/^To$/, rules()[0])).toBe("OBX.3");
    await user.click(screen.getByRole("button", { name: "Add destination 1 mapping rule" }));
    expect(rules()).toHaveLength(3);
    const added = rules()[2] as HTMLElement;
    await user.type(within(added).getByRole("textbox", { name: /^From/ }), "R.4");
    await user.type(within(added).getByRole("textbox", { name: /^To$/ }), "OBX.6");
    await user.click(within(added).getByRole("checkbox", { name: "Required" }));
    await user.click(screen.getByRole("button", { name: "Move destination 1 mapping rule 3 up" }));
    await user.click(screen.getByRole("button", { name: "Remove destination 1 mapping rule 1" }));
    expect(rules()).toHaveLength(2);
    const applied = await applyAndRead(user, api);
    expect(messageChannel(applied, "results")?.destinations[0]?.mappings).toEqual([
      { id: "m3", op: "copy", from: "R.4", to: "OBX.6", default: "", required: true },
      { id: "m2", op: "constant", value: "LAB", to: "MSH.4" },
    ]);
  });

  it("validates each mapping row before calling the engine", async () => {
    const { api, user } = await openResults();
    await user.click(screen.getByRole("button", { name: "Add destination 1 mapping rule" }));
    await user.click(screen.getByRole("button", { name: "Save and apply" }));
    const rule = screen.getByRole("group", { name: "Destination 1 mapping rule 3" });
    expect(await within(rule).findByText("Enter to.")).toBeTruthy();
    expect(within(rule).getByText("Enter from.")).toBeTruthy();
    expect(api.validateConfig).not.toHaveBeenCalled();
  });

  it("edits filters: the operator decides the value, rows can be removed", async () => {
    const { api, user } = await openResults();
    expect(screen.getByText(/A message continues only when all of these match/)).toBeTruthy();
    const filter = () => screen.getByRole("group", { name: "Source filter 1" });
    expect(within(filter()).queryByRole("textbox", { name: /Value/ })).toBeNull();
    await pick(user, filter(), "Operator", "equals");
    await user.type(within(filter()).getByRole("textbox", { name: /Value/ }), "GLU");
    await user.click(screen.getByRole("button", { name: "Add source filter" }));
    const second = () => screen.getByRole("group", { name: "Source filter 2" });
    await user.type(within(second()).getByRole("textbox", { name: /Path/ }), "R.2");
    await user.type(within(second()).getByRole("textbox", { name: /Value/ }), "x");
    await user.click(screen.getByRole("button", { name: "Remove source filter 2" }));
    expect(screen.queryByRole("group", { name: "Source filter 2" })).toBeNull();
    const applied = await applyAndRead(user, api);
    expect(messageChannel(applied, "results")?.filters).toEqual([
      { id: "f1", path: "R.3", operator: "equals", value: "GLU", ignoreCase: false },
    ]);
  });

  it("offers every operator of the engine and hides the value for exists and not_exists", async () => {
    const { user } = await openResults();
    const filter = () => screen.getByRole("group", { name: "Source filter 1" });
    expect(within(filter()).queryByRole("textbox", { name: /Value/ })).toBeNull();
    expect(within(filter()).queryByRole("checkbox", { name: "Ignore case" })).toBeNull();
    await user.click(within(filter()).getByRole("button", { name: /Operator/ }));
    expect(screen.getAllByRole("option")).toHaveLength(13);
    await user.click(screen.getByRole("option", { name: "is absent" }));
    expect(within(filter()).queryByRole("textbox", { name: /Value/ })).toBeNull();
    await pick(user, filter(), "Operator", "is one of");
    expect(within(filter()).getByRole("textbox", { name: /Value/ })).toBeTruthy();
    expect(within(filter()).getByText(/Separate the values with commas/)).toBeTruthy();
    await pick(user, filter(), "Operator", "is greater than");
    expect(
      within(filter()).getByText(/A number; text that is not a number never matches/),
    ).toBeTruthy();
    await pick(user, filter(), "Operator", "is present");
    expect(within(filter()).queryByRole("textbox", { name: /Value/ })).toBeNull();
  });

  it("offers ignore case for text operators only, and saves it", async () => {
    const { user, api } = await openResults();
    const filter = () => screen.getByRole("group", { name: "Source filter 1" });
    const last = async () => (await validatedChannel(user, api)).filters[0];
    await pick(user, filter(), "Operator", "is one of");
    await user.type(within(filter()).getByRole("textbox", { name: /Value/ }), "glu, na");
    await user.click(within(filter()).getByRole("checkbox", { name: "Ignore case" }));
    expect(await last()).toEqual({
      id: "f1",
      path: "R.3",
      operator: "in",
      value: "glu, na",
      ignoreCase: true,
    });
    await pick(user, filter(), "Operator", "is less than");
    await user.clear(within(filter()).getByRole("textbox", { name: /Value/ }));
    await user.type(within(filter()).getByRole("textbox", { name: /Value/ }), "5");
    expect(within(filter()).queryByRole("checkbox", { name: "Ignore case" })).toBeNull();
    expect(await last()).toMatchObject({ operator: "lt", value: "5", ignoreCase: false });
  });

  it("adds and removes destinations, and keeps retention limits optional", async () => {
    const { api, user } = await openResults();
    expect(screen.getByText(/never purged/)).toBeTruthy();
    expect(value(/^Keep for/)).toBe("30");
    await user.clear(screen.getByRole("textbox", { name: /^Keep for/ }));
    await user.type(screen.getByRole("textbox", { name: /^Keep at most/ }), "5000");
    await user.click(screen.getByRole("button", { name: "Add destination" }));
    await fillDestination(user, "Serial out", /^Chemistry serial/, "ASTM", 2);
    await removeSavedDestination(user, 1);
    const applied = await applyAndRead(user, api);
    const channel = messageChannel(applied, "results");
    expect(channel?.destinations).toEqual([
      {
        id: "d2",
        label: "Serial out",
        endpointId: "chem-serial",
        protocol: "astm",
        settings: {
          astmRole: "computer",
          maxFrameChars: 240,
          enqAttempts: 3,
          busyRetryMs: 10_000,
          sampleIdPath: "O.3",
        },
        enabled: true,
        filters: [],
        mappings: [],
        codeTables: {},
        retry: { initialMs: 2000, maxMs: 300_000, maxAttempts: 5 },
      },
    ]);
    expect(channel?.retention).toEqual({ days: null, maxMessages: 5000 });
  });

  it("asks for at least one destination", async () => {
    const { api, user } = await openResults();
    await removeSavedDestination(user, 1);
    await user.click(screen.getByRole("button", { name: "Save and apply" }));
    expect(await screen.findByText("Add at least one destination.")).toBeTruthy();
    expect(api.validateConfig).not.toHaveBeenCalled();
  });

  it("shows an engine finding on the row it names", async () => {
    const api = stubApi(
      validationFails({
        severity: "error",
        path: "channels[1].destinations[0].mappings[1].to",
        message: "Unknown path MSH.4.",
      }),
    );
    const { user } = await openResults(api);
    await user.click(screen.getByRole("button", { name: "Validate" }));
    const rule = screen.getByRole("group", { name: "Destination 1 mapping rule 2" });
    expect(await within(rule).findByText("Unknown path MSH.4.")).toBeTruthy();
    await followFinding(user, "Unknown path MSH.4.", () => textbox(rule, /^To$/));
  });

  it("deletes a channel after explaining what happens", async () => {
    const { api, user } = await openResults();
    await user.click(screen.getByRole("button", { name: "Delete channel…" }));
    const dialog = await screen.findByRole("alertdialog", {
      name: "Delete channel Results to LIS?",
    });
    expect(within(dialog).getByText(/other channels keep running/)).toBeTruthy();
    await typeNote(user);
    await user.click(within(dialog).getByRole("button", { name: "Delete channel" }));
    expect(await screen.findByText("Navigated to /channels")).toBeTruthy();
    const [document] = api.applyConfig.mock.calls[0] ?? [];
    expect(document?.channels.map((channel) => channel.id)).toEqual(["chem-bridge"]);
  });

  it("says so when the channel does not exist", async () => {
    renderRoutes(channelRoutes, "/channels/nope/edit", { api: stubApi() });
    expect(await screen.findByText("This channel was not found")).toBeTruthy();
  });

  it("has no accessibility violations", async () => {
    const { container } = await openResults();
    await expectNoA11yViolations(container);
  });
});
