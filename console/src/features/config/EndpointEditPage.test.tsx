import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { renderRoutes } from "@/lib/test/router";
import {
  endpointRoutes,
  fillTcpClient,
  openEndpoint,
  openNewEndpoint,
  pick,
  typeNote,
} from "./testActions";
import { engineFailure, sampleConfig, stubApi, validationFails } from "./testFixtures";

describe("EndpointEditPage", () => {
  it("shows the connection fields of the chosen kind", async () => {
    const { user } = await openNewEndpoint(stubApi());
    expect(screen.getByRole("heading", { level: 1, name: "New endpoint" })).toBeTruthy();
    expect(screen.getByText("Editing")).toBeTruthy();
    expect(screen.getByText("Choose a kind to see its connection settings.")).toBeTruthy();
    await pick(user, document.body, "Kind", "Serial");
    expect(screen.getByRole("textbox", { name: /^Baud rate/ })).toBeTruthy();
    expect(screen.queryByRole("textbox", { name: /^Host/ })).toBeNull();
    await pick(user, document.body, "Kind", "TCP listener");
    expect(screen.getByRole("textbox", { name: /^Bind address/ })).toBeTruthy();
    expect(screen.getByText("No peer allowlist")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Add address" }));
    expect(screen.queryByText("No peer allowlist")).toBeNull();
    expect(screen.getByRole("textbox", { name: /^IP address or range/ })).toBeTruthy();
  });

  it("suggests an id from the label until the id is typed", async () => {
    const { user } = await openNewEndpoint(stubApi());
    await user.type(screen.getByRole("textbox", { name: /^Label/ }), "LIS host");
    // "lis-host" is not taken, "lis-out" is another endpoint.
    expect((screen.getByRole("textbox", { name: /^Id\b/ }) as HTMLInputElement).value).toBe(
      "lis-host",
    );
    await user.clear(screen.getByRole("textbox", { name: /^Id\b/ }));
    await user.type(screen.getByRole("textbox", { name: /^Id\b/ }), "mine");
    await user.type(screen.getByRole("textbox", { name: /^Label/ }), " two");
    expect((screen.getByRole("textbox", { name: /^Id\b/ }) as HTMLInputElement).value).toBe("mine");
  });

  it("checks presence and format before calling the engine", async () => {
    const { api, user } = await openNewEndpoint(stubApi());
    await pick(user, document.body, "Kind", "TCP client");
    await user.type(screen.getByRole("textbox", { name: /^Port/ }), "70000");
    await user.click(screen.getByRole("button", { name: "Save and apply" }));
    expect(await screen.findByText("Enter label.")).toBeTruthy();
    expect(screen.getByText("Enter host.")).toBeTruthy();
    expect(screen.getByText("Enter a whole number from 1 to 65535.")).toBeTruthy();
    expect(api.validateConfig).not.toHaveBeenCalled();
    expect(api.applyConfig).not.toHaveBeenCalled();
    // The input is kept.
    expect((screen.getByRole("textbox", { name: /^Port/ }) as HTMLInputElement).value).toBe(
      "70000",
    );
  });

  it("puts an engine finding on its field and keeps the input", async () => {
    const api = stubApi(
      validationFails(
        { severity: "error", path: "endpoints[3].port", message: "Port 3002 is already in use." },
        { severity: "warning", path: "endpoints[3].host", message: "Host does not resolve." },
      ),
    );
    const { user } = await openNewEndpoint(api);
    await fillTcpClient(user);
    await user.click(screen.getByRole("button", { name: "Validate" }));
    const results = await screen.findByRole("region", { name: "Validation results" });
    expect(within(results).getByText("Port 3002 is already in use.")).toBeTruthy();
    expect(within(results).getByText("Warnings do not block applying.")).toBeTruthy();
    const port = screen.getByRole("textbox", { name: /^Port/ });
    await waitFor(() => expect(port.getAttribute("aria-invalid")).toBe("true"));
    expect(screen.getAllByText("Port 3002 is already in use.").length).toBeGreaterThan(1);
    expect(api.applyConfig).not.toHaveBeenCalled();
    expect((screen.getByRole("textbox", { name: /^Host/ }) as HTMLInputElement).value).toBe(
      "lis.example.test",
    );
  });

  it("validates, explains the apply, applies the edited document and opens the record", async () => {
    const { api, user } = await openNewEndpoint(stubApi());
    await fillTcpClient(user);
    await user.click(screen.getByRole("button", { name: "Validate" }));
    expect(await screen.findByText("No problems found")).toBeTruthy();
    expect(api.validateConfig).toHaveBeenCalledTimes(1);
    await user.click(screen.getByRole("button", { name: "Save and apply" }));
    const dialog = await screen.findByRole("alertdialog", {
      name: "Apply configuration version 8?",
    });
    expect(within(dialog).getByText(/Only channels that changed are restarted/)).toBeTruthy();
    expect(within(dialog).getByText("No channel is restarted.")).toBeTruthy();
    expect(api.applyConfig).not.toHaveBeenCalled();
    // Nothing is applied without a reason: the dialog refuses and says so.
    await user.click(within(dialog).getByRole("button", { name: "Apply version 8" }));
    expect(within(dialog).getByText("Enter why you are making this change.")).toBeTruthy();
    expect(api.applyConfig).not.toHaveBeenCalled();
    await typeNote(user, "Add LIS link");
    await user.click(within(dialog).getByRole("button", { name: "Apply version 8" }));
    expect(await screen.findByText("Navigated to /endpoints/chem-lis")).toBeTruthy();
    expect(api.applyConfig).toHaveBeenCalledTimes(1);
    const [document, note] = api.applyConfig.mock.calls[0] ?? [];
    expect(note).toBe("Add LIS link");
    expect(document?.version).toBe(7);
    expect(document?.endpoints).toHaveLength(4);
    expect(document?.endpoints[3]).toMatchObject({
      kind: "tcp_client",
      id: "chem-lis",
      label: "Chem LIS",
      host: "lis.example.test",
      port: 3002,
      connectTimeoutMs: 5000,
    });
    expect(await screen.findByText("Configuration version 8 applied")).toBeTruthy();
  });

  it("shows a refusal from the engine, leaves the running configuration alone and offers a reload on conflict", async () => {
    const api = stubApi({
      applyConfig: async () => {
        throw engineFailure("conflict", "The configuration changed since it was shown.");
      },
    });
    const { user } = await openNewEndpoint(api);
    await fillTcpClient(user);
    await user.click(screen.getByRole("button", { name: "Save and apply" }));
    await typeNote(user);
    await user.click(await screen.findByRole("button", { name: "Apply version 8" }));
    const alert = await screen.findByText("Not applied: the running configuration is unchanged");
    expect(alert).toBeTruthy();
    expect(screen.getByText("The configuration changed since it was shown.")).toBeTruthy();
    expect(screen.queryByRole("alertdialog")).toBeNull();
    await user.click(screen.getByRole("button", { name: "Reload configuration" }));
    await waitFor(() => expect(api.getConfig).toHaveBeenCalledTimes(2));
    expect(await screen.findByText("Configuration reloaded")).toBeTruthy();
    expect((screen.getByRole("textbox", { name: /^Host/ }) as HTMLInputElement).value).toBe(
      "lis.example.test",
    );
  });

  it("edits an existing endpoint with its id fixed, and asks before discarding changes", async () => {
    const api = stubApi();
    const { user } = await openEndpoint("lis-out", "LIS host", api);
    expect(screen.queryByRole("textbox", { name: /^Id\b/ })).toBeNull();
    expect(screen.getByText("lis-out")).toBeTruthy();
    await user.clear(screen.getByRole("textbox", { name: /^Host/ }));
    await user.type(screen.getByRole("textbox", { name: /^Host/ }), "new.example.test");
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    const dialog = await screen.findByRole("alertdialog", { name: "Leave without saving?" });
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect((screen.getByRole("textbox", { name: /^Host/ }) as HTMLInputElement).value).toBe(
      "new.example.test",
    );
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await user.click(await screen.findByRole("button", { name: "Leave without saving" }));
    expect(await screen.findByText("Navigated to /endpoints/lis-out")).toBeTruthy();
    expect(api.applyConfig).not.toHaveBeenCalled();
  });

  it("leaves a pristine form without asking", async () => {
    const { user } = await openEndpoint("lis-out", "LIS host", stubApi());
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(await screen.findByText("Navigated to /endpoints/lis-out")).toBeTruthy();
  });

  it("explains which channels use an endpoint that cannot be deleted", async () => {
    const api = stubApi();
    const { user } = await openEndpoint("analyzer-in", "Analyzer link", api);
    await user.click(screen.getByRole("button", { name: "Delete endpoint…" }));
    const dialog = await screen.findByRole("dialog", { name: "Analyzer link is in use" });
    expect(within(dialog).getByRole("link", { name: "Chemistry bridge" })).toBeTruthy();
    expect(within(dialog).getByRole("link", { name: "Results to LIS" })).toBeTruthy();
    expect(api.applyConfig).not.toHaveBeenCalled();
  });

  it("deletes an unused endpoint after confirming", async () => {
    const config = sampleConfig();
    config.channels = [];
    const api = stubApi({ getConfig: async () => config });
    const { user } = await openEndpoint("lis-out", "LIS host", api);
    await user.click(screen.getByRole("button", { name: "Delete endpoint…" }));
    const dialog = await screen.findByRole("alertdialog", { name: "Delete endpoint LIS host?" });
    expect(within(dialog).getByText(/No channel uses it/)).toBeTruthy();
    await typeNote(user);
    await user.click(within(dialog).getByRole("button", { name: "Delete endpoint" }));
    expect(await screen.findByText("Navigated to /endpoints")).toBeTruthy();
    const [document] = api.applyConfig.mock.calls[0] ?? [];
    expect(document?.endpoints.map((endpoint) => endpoint.id)).toEqual([
      "analyzer-in",
      "chem-serial",
    ]);
  });

  it("says so when the endpoint does not exist, and when the configuration cannot be read", async () => {
    const { unmount } = renderRoutes(endpointRoutes, "/endpoints/nope/edit", { api: stubApi() });
    expect(await screen.findByText("This endpoint was not found")).toBeTruthy();
    expect(screen.getByRole("link", { name: "Back to endpoints" })).toBeTruthy();
    unmount();
    renderRoutes(endpointRoutes, "/endpoints/lis-out/edit", {
      api: stubApi({
        getConfig: async () => {
          throw engineFailure("engine_unavailable", "The engine is not running.");
        },
      }),
    });
    expect(await screen.findByText("The configuration could not be loaded")).toBeTruthy();
    expect(screen.getByRole("heading", { level: 1, name: "Edit endpoint" })).toBeTruthy();
  });

  it("has no accessibility violations", async () => {
    const { container, user } = await openEndpoint("analyzer-in", "Analyzer link", stubApi());
    await expectNoA11yViolations(container, { wholePage: false });
    await user.click(screen.getByRole("button", { name: "Add address" }));
    await expectNoA11yViolations(container);
  });
});
