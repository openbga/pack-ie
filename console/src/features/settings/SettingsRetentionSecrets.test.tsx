import { screen, waitFor, within } from "@testing-library/react";
import type { UserEvent } from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import { engineFailure, sampleConfig, sampleSnapshot } from "@/features/config/testFixtures";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { openRow, openRowActions } from "@/lib/test/grid";
import { mockGridLayout } from "@/lib/test/layout";
import { renderRoutes } from "@/lib/test/router";
import { settingsApi, settingsRoutes } from "./testFixtures";

describe("SettingsPage", () => {
  beforeEach(mockGridLayout);

  it("leaves out the store figures the engine does not report", async () => {
    const snapshot = sampleSnapshot();
    snapshot.node.store = null;
    renderRoutes(settingsRoutes, "/settings/retention", {
      api: settingsApi({ getSnapshot: async () => snapshot }),
    });
    // State, message count, database size and disk free: none reported.
    expect(await screen.findAllByText("Not reported")).toHaveLength(4);
    expect(screen.queryByText("1,234")).toBeNull();
    expect(await screen.findByRole("grid", { name: "Retention by channel" })).toBeTruthy();
  });

  it("shows unknown storage and value as such", async () => {
    const api = settingsApi({
      listSecrets: async () => [
        { name: "mystery", storage: null, hasValue: null, usedByEndpointIds: [] },
      ],
    });
    renderRoutes(settingsRoutes, "/settings/secrets", { api });
    const grid = await screen.findByRole("grid", { name: "Secrets" });
    expect(within(grid).getByText("Not reported")).toBeTruthy();
    expect(within(grid).getByText("Unknown")).toBeTruthy();
    expect(screen.queryByText(/cannot store secrets/)).toBeNull();
  });

  it("is read-only when the engine cannot store secrets", async () => {
    const api = settingsApi({
      capabilities: { messages: "engine", secretsWritable: false, alertHistory: true },
    });
    const { user } = renderRoutes(settingsRoutes, "/settings/secrets", { api });
    const grid = await screen.findByRole("grid", { name: "Secrets" });
    expect(
      screen.getByText(
        "This engine build cannot store secrets through the console yet; set them with the OS key store or the secrets file.",
      ),
    ).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Add secret" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Actions for lis-sftp" })).toBeNull();
    await openRow(user, "lis-sftp");
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(within(grid).getByText("Stored")).toBeTruthy();
    expect(api.setSecret).not.toHaveBeenCalled();
    expect(api.deleteSecret).not.toHaveBeenCalled();
  });

  it("shows the store status and retention per message channel", async () => {
    renderRoutes(settingsRoutes, "/settings/retention", {
      api: settingsApi({ getSnapshot: async () => sampleSnapshot() }),
    });
    expect(await screen.findByText("1,234")).toBeTruthy();
    expect(screen.getByText("5.0 MB")).toBeTruthy();
    const grid = await screen.findByRole("grid", { name: "Retention by channel" });
    expect(within(grid).getByRole("gridcell", { name: "Results to LIS" })).toBeTruthy();
    expect(within(grid).getByText("30 days")).toBeTruthy();
    expect(within(grid).getByText("No count limit")).toBeTruthy();
    // Bridges keep no messages.
    expect(within(grid).queryByText("Chemistry bridge")).toBeNull();
  });

  it("edits one channel's retention in a dialog and applies it", async () => {
    const api = settingsApi();
    const { user } = renderRoutes(settingsRoutes, "/settings/retention", { api });
    await screen.findByRole("grid", { name: "Retention by channel" });
    await openRow(user, "Results to LIS");
    const dialog = await screen.findByRole("dialog", { name: "Edit retention of Results to LIS" });
    expect(
      within(dialog).getByText(/Undelivered and unresolved messages are never purged/),
    ).toBeTruthy();
    const days = within(dialog).getByRole("textbox", { name: /Keep for/ });
    await user.clear(days);
    await user.type(within(dialog).getByRole("textbox", { name: /Keep at most/ }), "2000");
    await user.type(within(dialog).getByRole("textbox", { name: /^Change note/ }), "Shorter keep");
    await user.click(within(dialog).getByRole("button", { name: "Save retention" }));
    expect(await screen.findByText("Configuration version 8 applied")).toBeTruthy();
    const [document, note] = api.applyConfig.mock.calls[0] ?? [];
    expect(note).toBe("Shorter keep");
    const channel = document?.channels.find((each) => each.id === "results");
    expect(channel).toMatchObject({ retention: { days: null, maxMessages: 2000 } });
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("shows an empty state when there are no message channels", async () => {
    const config = { ...sampleConfig(), channels: [] };
    renderRoutes(settingsRoutes, "/settings/retention", {
      api: settingsApi({ getConfig: async () => config }),
    });
    expect(await screen.findByText("No message channels yet")).toBeTruthy();
  });

  it("lists secrets without their values, with the endpoints that use them", async () => {
    const { container } = renderRoutes(settingsRoutes, "/settings/secrets", { api: settingsApi() });
    const grid = await screen.findByRole("grid", { name: "Secrets" });
    expect(within(grid).getByText("OS key store")).toBeTruthy();
    expect(within(grid).getByText("Stored")).toBeTruthy();
    expect(within(grid).getByText("Not set")).toBeTruthy();
    expect(within(grid).getByRole("link", { name: "lis-out" }).getAttribute("href")).toBe(
      "/endpoints/lis-out",
    );
    expect(within(grid).getByText("Not used")).toBeTruthy();
    await expectNoA11yViolations(container);
  });

  async function setValue(user: UserEvent, value: string) {
    const dialog = await screen.findByRole("dialog");
    const field = within(dialog).getByLabelText(/^Value/);
    expect(field.getAttribute("type")).toBe("password");
    await user.type(field, value);
    return dialog;
  }

  it("sets a value in a password field and never shows it again", async () => {
    const api = settingsApi();
    const { user } = renderRoutes(settingsRoutes, "/settings/secrets", { api });
    await screen.findByRole("grid", { name: "Secrets" });
    await openRow(user, "backup-sftp");
    const dialog = await setValue(user, "s3cret-value");
    await user.click(within(dialog).getByRole("button", { name: "Save value" }));
    expect(await screen.findByText("Secret backup-sftp saved")).toBeTruthy();
    expect(api.setSecret).toHaveBeenCalledWith("backup-sftp", "s3cret-value");
    expect(document.body.textContent).not.toContain("s3cret-value");
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("requires a value, and keeps what was typed when the engine refuses", async () => {
    const api = settingsApi({
      setSecret: async () => {
        throw engineFailure("unavailable", "The OS key store is locked.");
      },
    });
    const { user } = renderRoutes(settingsRoutes, "/settings/secrets", { api });
    await screen.findByRole("grid", { name: "Secrets" });
    await user.click(await screen.findByRole("button", { name: "Add secret" }));
    const dialog = await screen.findByRole("dialog", { name: "Add secret" });
    await user.click(within(dialog).getByRole("button", { name: "Add secret" }));
    expect(await within(dialog).findByText("Enter name.")).toBeTruthy();
    expect(within(dialog).getByText("Enter the value.")).toBeTruthy();
    await user.type(within(dialog).getByRole("textbox", { name: /^Name/ }), "new-secret");
    await user.type(within(dialog).getByLabelText(/^Value/), "abc");
    await user.click(within(dialog).getByRole("button", { name: "Add secret" }));
    expect(await within(dialog).findByText("The OS key store is locked.")).toBeTruthy();
    expect((within(dialog).getByRole("textbox", { name: /^Name/ }) as HTMLInputElement).value).toBe(
      "new-secret",
    );
  });

  it("warns before deleting a secret that endpoints still use", async () => {
    const api = settingsApi();
    const { user } = renderRoutes(settingsRoutes, "/settings/secrets", { api });
    await screen.findByRole("grid", { name: "Secrets" });
    const menu = await openRowActions(user, "lis-sftp");
    await user.click(within(menu).getByRole("menuitem", { name: "Delete secret…" }));
    const dialog = await screen.findByRole("alertdialog", { name: "Delete secret lis-sftp?" });
    expect(
      within(dialog).getByText(/cannot sign in until a new value is set: lis-out/),
    ).toBeTruthy();
    expect(api.deleteSecret).not.toHaveBeenCalled();
    await user.click(within(dialog).getByRole("button", { name: "Delete secret" }));
    await waitFor(() => expect(api.deleteSecret).toHaveBeenCalledWith("lis-sftp"));
    expect(await screen.findByText("Secret lis-sftp deleted")).toBeTruthy();
  });

  it("has empty and error states for secrets", async () => {
    const { unmount } = renderRoutes(settingsRoutes, "/settings/secrets", {
      api: settingsApi({ listSecrets: async () => [] }),
    });
    expect(await screen.findByText("No secrets yet")).toBeTruthy();
    unmount();
    renderRoutes(settingsRoutes, "/settings/secrets", {
      api: settingsApi({
        listSecrets: async () => {
          throw engineFailure("engine_unavailable", "The engine is not running.");
        },
      }),
    });
    expect(await screen.findByText("The secrets could not be loaded")).toBeTruthy();
  });
});
