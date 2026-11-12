import { screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { openRow } from "@/lib/test/grid";
import { mockGridLayout } from "@/lib/test/layout";
import { renderRoutes } from "@/lib/test/router";
import { engineFailure } from "./testFixtures";
import { history, versionRoutes as routes } from "./testVersions";

describe("VersionHistory", () => {
  beforeEach(mockGridLayout);

  it("lists who applied each version, when, why and whether it was verified", async () => {
    const { container } = renderRoutes(routes, "/settings/versions", { api: history() });
    const grid = await screen.findByRole("grid", { name: "Configuration versions" });
    expect(within(grid).getByRole("gridcell", { name: "Version 3" })).toBeTruthy();
    expect(within(grid).getByText("Current")).toBeTruthy();
    expect(within(grid).getAllByText("Superseded")).toHaveLength(2);
    expect(within(grid).getByText("Added channel Results to LIS")).toBeTruthy();
    expect(within(grid).getByText("LAB\\mlopez")).toBeTruthy();
    expect(within(grid).getByText("Verified (2)")).toBeTruthy();
    // Without a sign-off the words say so; a version saved before the record shows neither.
    expect(within(grid).getAllByText("Not verified since change")).toHaveLength(2);
    expect(within(grid).getByText("Not recorded")).toBeTruthy();
    // Each version's outcomes in a line; a failure is flagged in its own tone.
    expect(within(grid).getByText("1 restarted, 1 failed")).toBeTruthy();
    expect(within(grid).getByText("1 started")).toBeTruthy();
    expect(within(grid).getByText("No outcomes recorded")).toBeTruthy();
    expect(within(grid).getByText("No note")).toBeTruthy();
    await expectNoA11yViolations(container);
  });

  it("shows each sign-off of a version with who, when and the reference", async () => {
    const { user } = renderRoutes(routes, "/settings/versions", { api: history() });
    await openRow(user, "Version 2");
    const detail = await screen.findByRole("region", { name: "Version 2" });
    const list = within(detail).getByRole("list", { name: "Verifications" });
    expect(within(list).getAllByRole("listitem")).toHaveLength(2);
    expect(list.textContent).toContain("VR-2026-018");
    expect(list.textContent).toContain("LAB\\jdoe");
    expect(within(detail).getByText("Added channel Results to LIS")).toBeTruthy();
  });

  it("lists what applying a version did to each channel, a failure with its reason", async () => {
    const { user } = renderRoutes(routes, "/settings/versions", { api: history() });
    await openRow(user, "Version 3");
    const detail = await screen.findByRole("region", { name: "Version 3" });
    const outcomes = within(detail).getByRole("list", { name: "Channel outcomes" });
    const items = within(outcomes).getAllByRole("listitem");
    // Failures first, then by the words' order; labels come from the documents.
    expect(items[0]?.textContent).toContain("Failed");
    expect(items[0]?.textContent).toContain("Results to LIS");
    expect(items[0]?.textContent).toContain("Cannot bind COM3: access denied");
    expect(items[1]?.textContent).toContain("Restarted");
    expect(items[1]?.textContent).toContain("Chemistry bridge");
    expect(
      within(detail).getByText(/A channel that failed kept its previous definition/),
    ).toBeTruthy();
  });

  it("says when a version has no recorded outcomes", async () => {
    const { user } = renderRoutes(routes, "/settings/versions", { api: history() });
    await openRow(user, "Version 1");
    const detail = await screen.findByRole("region", { name: "Version 1" });
    expect(within(detail).getByText(/No per-channel outcomes are recorded/)).toBeTruthy();
  });

  it("marks a version as verified only with a reference", async () => {
    const api = history();
    const { user } = renderRoutes(routes, "/settings/versions", { api });
    await openRow(user, "Version 3");
    expect(await screen.findByText("Not verified since the change.")).toBeTruthy();
    await user.click(await screen.findByRole("button", { name: "Mark as verified…" }));
    const dialog = await screen.findByRole("alertdialog", { name: "Mark version 3 as verified?" });
    await user.click(within(dialog).getByRole("button", { name: "Mark version 3 as verified" }));
    expect(
      within(dialog).getByText(
        "Enter a verification reference, such as a validation record number.",
      ),
    ).toBeTruthy();
    expect(api.verifyConfigVersion).not.toHaveBeenCalled();
    await user.type(
      within(dialog).getByRole("textbox", { name: /Verification reference/ }),
      "  VR-2026-020 ",
    );
    await user.click(within(dialog).getByRole("button", { name: "Mark version 3 as verified" }));
    await waitFor(() => expect(api.verifyConfigVersion).toHaveBeenCalledWith(3, "VR-2026-020"));
    expect(await screen.findByText("Version 3 marked as verified")).toBeTruthy();
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });

  it("says why a sign-off was refused, such as an audit log that cannot be written", async () => {
    const api = history({
      verifyConfigVersion: async () => {
        throw engineFailure("unavailable", "The audit log cannot be written.");
      },
    });
    const { user } = renderRoutes(routes, "/settings/versions", { api });
    await openRow(user, "Version 3");
    await user.click(await screen.findByRole("button", { name: "Mark as verified…" }));
    const dialog = await screen.findByRole("alertdialog");
    await user.type(within(dialog).getByRole("textbox"), "VR-1");
    await user.click(within(dialog).getByRole("button", { name: /Mark version 3 as verified$/ }));
    expect(await within(dialog).findByText("The audit log cannot be written.")).toBeTruthy();
    expect(within(dialog).getByText("The version was not marked as verified")).toBeTruthy();
  });

  it("shows what a chosen version holds and how it differs from the current one", async () => {
    const { user, router } = renderRoutes(routes, "/settings/versions", { api: history() });
    await openRow(user, "Version 2");
    const detail = await screen.findByRole("region", { name: "Version 2" });
    expect(await within(detail).findByText("Compared with the active version (3)")).toBeTruthy();
    // The version's own configuration, read-only, in words.
    expect(within(detail).getByText("Configuration in this version (read-only)")).toBeTruthy();
    expect(within(detail).getByText("Endpoints (3)")).toBeTruthy();
    expect(within(detail).getByText("Channels (1)")).toBeTruthy();
    const channels = within(detail).getByRole("list", { name: "Channels" });
    expect(within(channels).getByText("Chemistry bridge")).toBeTruthy();
    // What rolling back changes: the channel only the active configuration has, and the
    // threshold as `field: active → version`.
    const channelDiff = within(detail).getByRole("region", { name: "Channels that differ" });
    expect(channelDiff.textContent).toContain("Removed: Results to LIS results (not in version 2)");
    expect(within(detail).getByText("Delivery queue warning: 500 → 100")).toBeTruthy();
    expect(router.state.location.search).toBe("?version=2");
    expect(within(detail).getByRole("button", { name: "Roll back to this version…" })).toBeTruthy();
  });

  it("offers no rollback for the current version", async () => {
    const { user } = renderRoutes(routes, "/settings/versions", { api: history() });
    await openRow(user, "Version 3");
    const detail = await screen.findByRole("region", { name: "Version 3" });
    expect(
      await within(detail).findByText(/This is the configuration the engine is running/),
    ).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Roll back to this version…" })).toBeNull();
  });

  it("opens the version named in the address", async () => {
    renderRoutes(routes, "/settings/versions?version=1", { api: history() });
    expect(await screen.findByRole("region", { name: "Version 1" })).toBeTruthy();
  });

  it("shows empty and error states", async () => {
    const { unmount } = renderRoutes(routes, "/settings/versions", {
      api: history({ listConfigVersions: async () => [] }),
    });
    expect(await screen.findByText("No versions yet")).toBeTruthy();
    unmount();
    renderRoutes(routes, "/settings/versions", {
      api: history({
        listConfigVersions: async () => {
          throw engineFailure("engine_unavailable", "The engine is not running.");
        },
      }),
    });
    expect(await screen.findByText("The version history could not be loaded")).toBeTruthy();
  });
});
