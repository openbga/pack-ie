import { screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { MessageChannelConfig } from "@/api/types";
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
  textbox,
  validatedChannel,
} from "./testActions";
import { configWithFiles, stubApi, stubApiWithFiles, validationFails } from "./testFixtures";

/** What a connector carries for the settings the engine defaults. */
const ASTM_DEFAULTS = {
  maxFrameChars: 240,
  enqAttempts: 3,
  busyRetryMs: 10_000,
  sampleIdPath: "O.3",
};
const HL7_DEFAULTS = {
  hl7Version: "2.5.1",
  requireEndCr: true,
  ackMessageTypes: ["ACK"],
  replyType: "ack",
};

const openNewMessage = () => openNewChannel("Message", stubApiWithFiles());

describe("source: duplicate detection and protocol by endpoint kind", () => {
  it("detects duplicates by default, explains it and saves the choice", async () => {
    const { api, user } = await openNewMessage();
    const source = group("Source");
    const box = within(source).getByRole("checkbox", { name: "Detect duplicate messages" });
    expect((box as HTMLInputElement).checked).toBe(true);
    expect(
      within(source).getByText(
        "A repeat of a message (same source identity and content) is recorded and shown, not processed twice.",
      ),
    ).toBeTruthy();
    await user.click(box);
    await user.type(channelLabel(), "Files");
    await pickEndpoint(user, source, "Endpoint", /^Incoming folder/);
    await pick(user, source, "Protocol", "Delimited files");
    await fillDestination(user, "LIS", /^LIS host/, "HL7 v2");
    await addCopyRule(user);
    const channel = await validatedChannel(user, api, 2);
    expect(channel.source.duplicateDetection).toBe(false);
  });

  it("offers only the protocols that suit the chosen endpoint kind, never raw", async () => {
    const { user } = await openNewMessage();
    const source = group("Source");
    expect(await optionsOf(user, source, "Protocol")).toEqual([
      "ASTM",
      "AU",
      "HL7 v2",
      "Delimited files",
    ]);
    await pickEndpoint(user, source, "Endpoint", /^Chemistry serial/);
    expect(await optionsOf(user, source, "Protocol")).toEqual(["ASTM", "AU"]);
    await pickEndpoint(user, source, "Endpoint", /^LIS host/);
    expect(await optionsOf(user, source, "Protocol")).toEqual(["ASTM", "AU", "HL7 v2"]);
    await pickEndpoint(user, source, "Endpoint", /^Analyzer link/);
    expect(await optionsOf(user, source, "Protocol")).toEqual(["ASTM", "AU", "HL7 v2"]);
    await pickEndpoint(user, source, "Endpoint", /^Incoming folder/);
    expect(await optionsOf(user, source, "Protocol")).toEqual(["Delimited files"]);
    await pickEndpoint(user, source, "Endpoint", /^Partner SFTP/);
    expect(await optionsOf(user, source, "Protocol")).toEqual(["Delimited files"]);
    const destination = group("Destination 1");
    await pickEndpoint(user, destination, "Endpoint", /^Chemistry serial/);
    expect(await optionsOf(user, destination, "Protocol")).toEqual(["ASTM", "AU"]);
  });

  it("names a protocol that no longer suits a changed endpoint and refuses to save it", async () => {
    const { api, user } = await openNewMessage();
    const source = group("Source");
    await user.type(channelLabel(), "Mixed");
    await pickEndpoint(user, source, "Endpoint", /^Chemistry serial/);
    await pick(user, source, "Protocol", "ASTM");
    await pickEndpoint(user, source, "Endpoint", /^Incoming folder/);
    expect(await optionsOf(user, source, "Protocol")).toEqual([
      "Delimited files",
      "ASTM (does not suit this endpoint)",
    ]);
    await user.click(screen.getByRole("button", { name: "Save and apply" }));
    expect(
      await within(source).findByText("ASTM cannot be used with this kind of endpoint."),
    ).toBeTruthy();
    expect(api.validateConfig).not.toHaveBeenCalled();
  });
});

describe("HL7 and ASTM connector settings", () => {
  async function newChannelWith(sourceEndpoint: RegExp, destinationEndpoint: RegExp) {
    const rendered = await openNewMessage();
    await rendered.user.type(channelLabel(), "Links");
    await pickEndpoint(rendered.user, group("Source"), "Endpoint", sourceEndpoint);
    await fillDestination(rendered.user, "Out", destinationEndpoint);
    return rendered;
  }

  it("offers a link role for ASTM, computer first, with the contention rule", async () => {
    const { api, user } = await newChannelWith(/^Chemistry serial/, /^LIS host/);
    const source = group("Source");
    await pick(user, source, "Protocol", "ASTM");
    expect(within(source).getByRole("button", { name: /Link role/ }).textContent).toContain(
      "Computer",
    );
    expect(await optionsOf(user, source, "Link role")).toEqual(["Computer", "Instrument"]);
    expect(within(source).getByText(/the computer yields and sends after receiving/)).toBeTruthy();
    await pick(user, source, "Link role", "Instrument");
    const destination = group("Destination 1");
    await pick(user, destination, "Protocol", "ASTM");
    const channel = await validatedChannel(user, api, 2);
    expect(channel.source.settings).toEqual({ astmRole: "instrument", ...ASTM_DEFAULTS });
    expect(channel.destinations[0]?.settings).toEqual({ astmRole: "computer", ...ASTM_DEFAULTS });
  });

  it("lets only a destination expect no acknowledgement", async () => {
    const { api, user } = await newChannelWith(/^LIS host/, /^LIS host/);
    const source = group("Source");
    await pick(user, source, "Protocol", "HL7 v2");
    expect(await optionsOf(user, source, "Acknowledgement mode")).toEqual(["Original", "Enhanced"]);
    const destination = group("Destination 1");
    await pick(user, destination, "Protocol", "HL7 v2");
    expect(await optionsOf(user, destination, "Acknowledgement mode")).toEqual([
      "Original",
      "Enhanced",
      "None",
    ]);
    await pick(user, destination, "Acknowledgement mode", "None");
    const channel = await validatedChannel(user, api, 2);
    expect(channel.source.settings).toEqual({ ackMode: "original", ...HL7_DEFAULTS });
    expect(channel.destinations[0]?.settings).toEqual({ ackMode: "none", ...HL7_DEFAULTS });
  });

  it("shows the engine's refusal of a source with no acknowledgement on its field", async () => {
    const api = stubApi({
      getConfig: async () => {
        const doc = configWithFiles();
        const channel = doc.channels[1] as MessageChannelConfig;
        channel.source = {
          ...channel.source,
          endpointId: "lis-out",
          protocol: "hl7_mllp",
          settings: { ackMode: "none" },
        };
        return doc;
      },
      ...validationFails({
        severity: "error",
        path: "channels[1].source.settings.ackMode",
        message: "A source must acknowledge.",
      }),
    });
    const { user } = await openResults(api);
    const source = group("Source");
    expect(await optionsOf(user, source, "Acknowledgement mode")).toContain(
      "None (not allowed for a source)",
    );
    await user.click(screen.getByRole("button", { name: "Validate" }));
    expect(await within(source).findByText("A source must acknowledge.")).toBeTruthy();
  });
});

describe("sample id path of an ASTM source", () => {
  it("is a source field with its default, checked as the engine checks it, and not on a destination", async () => {
    const { api, user } = await openResults();
    const source = group("Source");
    expect((textbox(source, /^Sample id path/) as HTMLInputElement).value).toBe("O.3");
    expect(within(source).getByText(/O\.4\.3 is component 3 of field 4/)).toBeTruthy();
    await pick(user, group("Destination 1"), "Protocol", "ASTM");
    expect(
      within(group("Destination 1")).queryByRole("textbox", { name: /^Sample id path/ }),
    ).toBeNull();
    for (const bad of ["O.2", "O.04", "O.3.0", "P.3", "O.100"]) {
      await user.clear(textbox(source, /^Sample id path/));
      await user.type(textbox(source, /^Sample id path/), bad);
      await user.click(screen.getByRole("button", { name: "Validate" }));
      expect(
        await within(source).findByText(/^Use O\. and a field from 3 to 99/),
        bad,
      ).toBeTruthy();
    }
    await user.clear(textbox(source, /^Sample id path/));
    await user.type(textbox(source, /^Sample id path/), "O.4.3");
    const channel = await validatedChannel(user, api);
    expect(channel.source.settings.sampleIdPath).toBe("O.4.3");
    expect(channel.destinations[0]?.settings.sampleIdPath).toBe("O.3");
  });
});
