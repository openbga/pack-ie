import { screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ValidationResult } from "@/api/types";
import { openRow } from "@/lib/test/grid";
import { mockGridLayout } from "@/lib/test/layout";
import { renderRoutes } from "@/lib/test/router";
import { engineFailure } from "./testFixtures";
import { history, versionRoutes as routes } from "./testVersions";

describe("VersionHistory: rollback", () => {
  beforeEach(mockGridLayout);

  it("rolls back after a confirmation that says what restarts", async () => {
    const api = history();
    const { user } = renderRoutes(routes, "/settings/versions", { api });
    await openRow(user, "Version 2");
    await user.click(await screen.findByRole("button", { name: "Roll back to this version…" }));
    const dialog = await screen.findByRole("alertdialog", { name: "Roll back to version 2?" });
    expect(
      within(dialog).getByText(/Creates a new version, 4, with the configuration of version 2/),
    ).toBeTruthy();
    // The plan is the engine's, not a guess of the console's.
    expect(await within(dialog).findByText("Stopped and removed: Results to LIS")).toBeTruthy();
    expect(api.previewRollback).toHaveBeenCalledWith(2);
    // It says what the comparison holds, and what removing a channel does to its deliveries.
    expect(within(dialog).getByText("Channels: 1 removed")).toBeTruthy();
    expect(within(dialog).getByText("Alert thresholds differ")).toBeTruthy();
    expect(
      within(dialog).getByText(/Unresolved deliveries of a channel or destination/),
    ).toBeTruthy();
    expect(api.rollbackConfig).not.toHaveBeenCalled();
    // A rollback says why, like any change: nothing happens without a reason.
    await user.click(within(dialog).getByRole("button", { name: "Roll back to version 2" }));
    expect(within(dialog).getByText("Enter why you are making this change.")).toBeTruthy();
    expect(api.rollbackConfig).not.toHaveBeenCalled();
    await user.type(
      within(dialog).getByRole("textbox", { name: /Reason for the rollback/ }),
      "The new mapping dropped a field",
    );
    await user.click(within(dialog).getByRole("button", { name: "Roll back to version 2" }));
    await waitFor(() =>
      expect(api.rollbackConfig).toHaveBeenCalledWith(2, "The new mapping dropped a field"),
    );
    expect(await screen.findByText("Rolled back to version 2")).toBeTruthy();
    expect(
      screen.getByText(
        "Saved as version 4. Restarted: Chemistry bridge. Removed: Results to LIS. 0 unchanged.",
      ),
    ).toBeTruthy();
    expect(screen.queryByRole("alertdialog")).toBeNull();
    // The cache is refreshed: the lists and the configuration are read again.
    await waitFor(() => expect(vi.mocked(api.getConfig).mock.calls.length).toBeGreaterThan(1));
  });

  it("does not let the rollback be confirmed before the engine's plan is shown", async () => {
    let answer: (result: ValidationResult) => void = () => {};
    const api = history({
      previewRollback: vi.fn(() => new Promise<ValidationResult>((resolve) => (answer = resolve))),
    });
    const { user } = renderRoutes(routes, "/settings/versions", { api });
    await openRow(user, "Version 2");
    await user.click(await screen.findByRole("button", { name: "Roll back to this version…" }));
    const dialog = await screen.findByRole("alertdialog", { name: "Roll back to version 2?" });
    expect(
      await within(dialog).findByText(/Asking the engine what rolling back would do/),
    ).toBeTruthy();
    const confirm = within(dialog).getByRole("button", { name: "Roll back to version 2" });
    expect(confirm.hasAttribute("disabled")).toBe(true);
    answer({
      ok: true,
      issues: [],
      plan: {
        channels: [{ id: "chem-bridge", label: "Chemistry bridge", change: "updated" }],
        endpoints: [],
        alertsChanged: false,
      },
    });
    expect(await within(dialog).findByText("Updated in place: Chemistry bridge")).toBeTruthy();
    expect(within(dialog).getByText("Untouched: 0 channels, which keep running.")).toBeTruthy();
    expect(confirm.hasAttribute("disabled")).toBe(false);
  });

  it("says when the plan cannot be loaded, keeps Roll back off and retries", async () => {
    const previewRollback = vi
      .fn()
      .mockRejectedValueOnce(engineFailure("engine_unavailable", "The engine is not running."))
      .mockResolvedValue({
        ok: true,
        issues: [],
        plan: { channels: [], endpoints: [], alertsChanged: false },
      });
    const { user } = renderRoutes(routes, "/settings/versions", {
      api: history({ previewRollback }),
    });
    await openRow(user, "Version 2");
    await user.click(await screen.findByRole("button", { name: "Roll back to this version…" }));
    const dialog = await screen.findByRole("alertdialog", { name: "Roll back to version 2?" });
    expect(
      await within(dialog).findByText(/could not be loaded, so Roll back is off/),
    ).toBeTruthy();
    const confirm = within(dialog).getByRole("button", { name: "Roll back to version 2" });
    expect(confirm.hasAttribute("disabled")).toBe(true);
    await user.click(within(dialog).getByRole("button", { name: "Try again" }));
    await waitFor(() => expect(confirm.hasAttribute("disabled")).toBe(false));
  });

  it("keeps Roll back off when the engine would refuse the rollback", async () => {
    const api = history({
      previewRollback: async () => ({
        ok: false,
        issues: [{ severity: "error", path: "channels[0]", message: "Secret lis-key is missing." }],
        plan: { channels: [], endpoints: [], alertsChanged: false },
      }),
    });
    const { user } = renderRoutes(routes, "/settings/versions", { api });
    await openRow(user, "Version 2");
    await user.click(await screen.findByRole("button", { name: "Roll back to this version…" }));
    const dialog = await screen.findByRole("alertdialog", { name: "Roll back to version 2?" });
    expect(await within(dialog).findByText("Secret lis-key is missing.")).toBeTruthy();
    expect(
      within(dialog)
        .getByRole("button", { name: "Roll back to version 2" })
        .hasAttribute("disabled"),
    ).toBe(true);
  });

  it("calls out a partial rollback with the failed channel's reason, until dismissed", async () => {
    const api = history({
      rollbackConfig: async () => ({
        version: 4,
        outcomes: [
          {
            channelId: "chem-bridge",
            outcome: "failed" as const,
            reason: "Cannot bind COM3: access denied",
          },
          { channelId: "results", outcome: "removed" as const },
        ],
        restartedChannelIds: [],
        unchangedChannelCount: 0,
        failures: [{ channelId: "chem-bridge", reason: "Cannot bind COM3: access denied" }],
      }),
    });
    const { user } = renderRoutes(routes, "/settings/versions", { api });
    await openRow(user, "Version 2");
    await user.click(await screen.findByRole("button", { name: "Roll back to this version…" }));
    const dialog = await screen.findByRole("alertdialog");
    await within(dialog).findByText("Stopped and removed: Results to LIS");
    await user.type(within(dialog).getByRole("textbox"), "Back out");
    await user.click(within(dialog).getByRole("button", { name: "Roll back to version 2" }));
    expect(await screen.findByText("Rolled back to version 2, but not everywhere")).toBeTruthy();
    expect(
      screen.getByText(
        /Version 4 is active, but this channel was not changed and keeps the previous definition/,
      ),
    ).toBeTruthy();
    expect(screen.getByText(/Chemistry bridge: Cannot bind COM3: access denied/)).toBeTruthy();
  });

  it("keeps the dialog open and says why when the engine refuses", async () => {
    const api = history({
      rollbackConfig: async () => {
        throw engineFailure("validation", "Version 2 no longer passes validation.");
      },
    });
    const { user } = renderRoutes(routes, "/settings/versions", { api });
    await openRow(user, "Version 1");
    await user.click(await screen.findByRole("button", { name: "Roll back to this version…" }));
    const dialog = await screen.findByRole("alertdialog");
    await user.type(within(dialog).getByRole("textbox"), "Testing the refusal");
    await user.click(within(dialog).getByRole("button", { name: "Roll back to version 1" }));
    expect(await within(dialog).findByText("Version 2 no longer passes validation.")).toBeTruthy();
    expect(within(dialog).getByText("The rollback did not happen")).toBeTruthy();
  });
});
