import { screen, waitFor, within } from "@testing-library/react";
import type { UserEvent } from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { renderRoutes } from "@/lib/test/router";
import {
  applyAndRead,
  channelLabel,
  channelRoutes,
  group,
  openChannel,
  openNewChannel,
  pick,
  pickEndpoint,
  textbox,
} from "./testActions";
import { stubApi, stubApiWithFiles, validationFails } from "./testFixtures";

const value = (name: RegExp) => (textbox(document.body, name) as HTMLInputElement).value;

/** Picks the endpoints of a new bridge: the chemistry serial line and the LIS host. */
async function pickBridgeEnds(user: UserEvent) {
  await pickEndpoint(user, document.body, "Endpoint A", /^Chemistry serial/);
  await pickEndpoint(user, document.body, "Endpoint B", /^LIS host/);
}

describe("ChannelEditPage: bridge", () => {
  it("offers both kinds on create and shows the fields of the chosen one", async () => {
    const { user } = renderRoutes(channelRoutes, "/channels/new", { api: stubApi() });
    await screen.findByRole("button", { name: /Kind/ });
    expect(screen.getByText("Choose a kind to see its settings.")).toBeTruthy();
    await pick(user, document.body, "Kind", "Bridge");
    expect(screen.getByRole("button", { name: /Endpoint A/ })).toBeTruthy();
    expect(screen.getByRole("textbox", { name: /Buffer size/ })).toBeTruthy();
    expect(screen.queryByRole("group", { name: "Source filters" })).toBeNull();
    await pick(user, document.body, "Kind", "Message");
    expect(screen.queryByRole("button", { name: /Endpoint A/ })).toBeNull();
    for (const title of ["Source", "Source filters", "Destinations", "Retention"]) {
      expect(group(title)).toBeTruthy();
    }
  });

  it("lists endpoints with their kind and state, and refuses the same endpoint on both sides", async () => {
    const { api, user } = await openNewChannel("Bridge", stubApi());
    await user.type(channelLabel(), "Same both");
    await user.click(screen.getByRole("button", { name: /Endpoint A/ }));
    const analyzer = await screen.findByRole("option", { name: "Analyzer link" });
    expect(analyzer.textContent).toContain("TCP listener · Listening");
    expect(screen.getByRole("option", { name: "LIS host" }).textContent).toContain("TCP client");
    await user.click(analyzer);
    await pickEndpoint(user, document.body, "Endpoint B", /^Analyzer link/);
    await user.click(screen.getByRole("button", { name: "Save and apply" }));
    expect(await screen.findByText("Endpoint B must differ from endpoint A.")).toBeTruthy();
    expect(api.applyConfig).not.toHaveBeenCalled();
    await pickEndpoint(user, document.body, "Endpoint B", /^LIS host/);
    expect(screen.queryByText("Endpoint B must differ from endpoint A.")).toBeNull();
    const applied = await applyAndRead(user, api);
    expect(applied?.channels[2]).toMatchObject({
      kind: "bridge",
      id: "same-both",
      endpointAId: "analyzer-in",
      endpointBId: "lis-out",
      pairing: "parallel",
      bufferBytes: 65536,
      tapFramer: "none",
    });
    expect(await screen.findByText("Navigated to /channels/same-both")).toBeTruthy();
  });

  it("finds an endpoint by typing part of its name, kind or state, and keeps the choice visible", async () => {
    const { user } = await openNewChannel("Bridge", stubApiWithFiles());
    const field = screen.getByRole("combobox", { name: /Endpoint A/ });
    await user.type(field, "listening");
    expect(
      within(screen.getByRole("listbox"))
        .getAllByRole("option")
        .map((option) => option.textContent),
    ).toEqual(["Analyzer linkTCP listener · Listening · analyzer-in"]);
    await user.clear(field);
    await user.type(field, "sftp");
    await user.keyboard("{ArrowDown}{Enter}");
    expect(field).toHaveProperty("value", "Partner SFTP (SFTP)");
  });

  it("asks for the receiver only with the receiver-first strategy, and requires it", async () => {
    const { api, user } = await openNewChannel("Bridge", stubApi());
    await user.type(channelLabel(), "Receiver first");
    await pickBridgeEnds(user);
    expect(screen.queryByRole("button", { name: /Receiver$/ })).toBeNull();
    await pick(user, document.body, "Pairing strategy", "Receiver first, then sender");
    expect(screen.getByRole("button", { name: /Receiver$/ })).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Save and apply" }));
    expect(await screen.findByText("Choose receiver.")).toBeTruthy();
    expect(api.validateConfig).not.toHaveBeenCalled();
    await pick(user, document.body, "Receiver$", "Endpoint B");
    const applied = await applyAndRead(user, api);
    expect(applied?.channels[2]).toMatchObject({
      pairing: "receiver_then_sender",
      pairingReceiver: "b",
    });
  });

  it("forgets the receiver when another strategy is chosen again", async () => {
    const { api, user } = await openNewChannel("Bridge", stubApi());
    await user.type(channelLabel(), "Back to parallel");
    await pickBridgeEnds(user);
    await pick(user, document.body, "Pairing strategy", "Receiver first, then sender");
    await pick(user, document.body, "Receiver$", "Endpoint A");
    await pick(user, document.body, "Pairing strategy", "Connect both sides independently");
    expect(screen.queryByRole("button", { name: /Receiver$/ })).toBeNull();
    const applied = await applyAndRead(user, api);
    expect(applied?.channels[2]).toMatchObject({ pairing: "parallel", pairingReceiver: null });
  });

  it("offers the AU tap framer and retries for ever unless attempts are given", async () => {
    const { api, user } = await openNewChannel("Bridge", stubApi());
    await user.type(channelLabel(), "AU tap");
    await pickBridgeEnds(user);
    await user.click(screen.getByRole("button", { name: /Tap framer/ }));
    expect(screen.getAllByRole("option").map((option) => option.textContent)).toEqual([
      "None (bytes only)",
      "ASTM",
      "AU",
      "HL7 MLLP",
    ]);
    await user.click(screen.getByRole("option", { name: "AU" }));
    expect(screen.getByText("Leave empty to keep retrying for ever.")).toBeTruthy();
    expect(value(/^Maximum attempts/)).toBe("");
    const applied = await applyAndRead(user, api);
    expect(applied?.channels[2]).toMatchObject({
      tapFramer: "au",
      retry: { initialMs: 1000, maxMs: 60_000, maxAttempts: null },
    });
  });

  it("keeps the attempts of an existing bridge", async () => {
    const { api, user } = await openChannel("chem-bridge", "Chemistry bridge");
    expect(value(/^Maximum attempts/)).toBe("10");
    await user.clear(screen.getByRole("textbox", { name: /^Maximum attempts/ }));
    const applied = await applyAndRead(user, api);
    expect(applied?.channels[0]).toMatchObject({ retry: { maxAttempts: null } });
  });

  it("puts an engine finding on a bridge field", async () => {
    const api = stubApi(
      validationFails({
        severity: "error",
        path: "channels[0].bufferBytes",
        message: "The buffer is too small.",
      }),
    );
    const { user } = await openChannel("chem-bridge", "Chemistry bridge", api);
    await user.click(screen.getByRole("button", { name: "Validate" }));
    const field = await screen.findByRole("textbox", { name: /Buffer size/ });
    await waitFor(() => expect(field.getAttribute("aria-invalid")).toBe("true"));
    expect(screen.getAllByText("The buffer is too small.").length).toBeGreaterThan(0);
  });
});
