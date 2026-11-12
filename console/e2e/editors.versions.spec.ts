import { expect, type Page, test } from "@playwright/test";
import {
  fill,
  openEditor,
  openVersions,
  pick,
  reviewDialog,
  saveAndApply,
  saveAndConfirm,
  text,
  versionRow,
} from "./editors.support";
import { dataRows, heading, open } from "./support";

const history = (page: Page) => page.getByRole("grid", { name: "Configuration versions" });

test("H-07 version history: the list, a read-only view of a version and its comparison with the active one", async ({
  page,
}) => {
  await open(page, "/settings/versions");
  await expect(heading(page, "Settings")).toBeVisible();
  await expect(history(page)).toBeVisible();
  for (const column of [
    "Version",
    "Applied",
    "Applied by",
    "Note",
    "Channel outcomes",
    "Verified",
    "State",
  ]) {
    await expect(
      history(page).getByRole("columnheader", { name: column, exact: true }),
    ).toBeVisible();
  }
  const rows = dataRows(page, "Configuration versions");
  await expect(rows.first()).toContainText("Version 47");
  await expect(rows.first()).toContainText("Current");
  await expect(rows.nth(1)).toContainText("Superseded");
  await expect(history(page).getByText("rollback of 42")).toBeVisible();
  await expect(page.getByRole("heading", { name: /^Version \d+$/, level: 2 })).toHaveCount(0);

  // Choosing a version puts it in the address and shows what it held.
  await versionRow(page, 44).getByRole("gridcell").first().click();
  await expect(page).toHaveURL(/#\/settings\/versions\?version=44$/);
  await expect(page.getByRole("heading", { name: "Version 44" })).toBeVisible();
  await expect(page.getByText("Configuration in this version (read-only)")).toBeVisible();
  const detail = page.locator("section", {
    has: page.getByRole("heading", { name: "Version 44", exact: true }),
  });
  await expect(detail.getByRole("textbox")).toHaveCount(0);
  await expect(detail.getByText("Endpoints (56)")).toBeVisible();
  await expect(detail.getByText("Channels (36)")).toBeVisible();

  // The comparison with the active version is field by field.
  await expect(page.getByText("Compared with the active version (47)")).toBeVisible();
  await expect(
    page.getByText(
      /^Each line reads: field, the active value \(version 47\), then the value in version 44\./,
    ),
  ).toBeVisible();
  await expect(page.getByText("Channels (3 changed)")).toBeVisible();
  await expect(page.getByText("Retention › Keep for (days): 30 → 7")).toBeVisible();
  await expect(page.getByText("Filters: 1 filter → 0 filters")).toBeVisible();
  await expect(page.getByRole("button", { name: "Roll back to this version…" })).toBeVisible();

  // The active version has nothing to compare or roll back to.
  await page.evaluate(() => {
    window.location.hash = "/settings/versions?version=47";
  });
  await expect(page.getByText("This is the configuration the engine is running.")).toBeVisible();
  await expect(page.getByRole("button", { name: "Roll back to this version…" })).toHaveCount(0);
});

test("H-07 opening ?version=N in the address shows that version", async ({ page }) => {
  await open(page, "/settings/versions?version=40");
  await expect(page.getByRole("heading", { name: "Version 40" })).toBeVisible();
  await expect(page.getByText("Compared with the active version (47)")).toBeVisible();
  await expect(versionRow(page, 40)).toBeVisible();
});

test("H-07 rollback confirms, says what restarts and saves the old configuration as a new version", async ({
  page,
}) => {
  await open(page, "/settings/versions?version=44");
  await page.getByRole("button", { name: "Roll back to this version…" }).click();
  const dialog = page.getByRole("alertdialog", { name: "Roll back to version 44?" });
  await expect(dialog).toContainText(
    "Creates a new version, 48, with the configuration of version 44.",
  );
  await expect(dialog).toContainText("History is kept: version 47 stays available.");
  await expect(dialog).toContainText("Channels: 3 changed");
  // The plan is the engine's: all three differences are taken in place, nothing restarts.
  await expect(dialog.getByText("What the engine will do")).toBeVisible();
  await expect(dialog).toContainText(
    "Updated in place: Reference lab results, Blood gas results, Dashboard feed",
  );
  await expect(dialog.getByText(/Restarted/)).toHaveCount(0);

  // Cancel changes nothing.
  await dialog.getByRole("button", { name: "Cancel" }).click();
  await expect(dialog).toHaveCount(0);
  await expect(versionRow(page, 48)).toHaveCount(0);

  await page.getByRole("button", { name: "Roll back to this version…" }).click();
  const confirming = page.getByRole("alertdialog", { name: "Roll back to version 44?" });
  // A rollback says why, like any change.
  await confirming.getByRole("button", { name: "Roll back to version 44" }).click();
  await expect(confirming.getByText("Enter why you are making this change.")).toBeVisible();
  await confirming
    .getByRole("textbox", { name: /^Reason for the rollback/ })
    .fill("The new mappings dropped a field");
  await confirming.getByRole("button", { name: "Roll back to version 44" }).click();
  await expect(page.getByText("Rolled back to version 44")).toBeVisible();
  await expect(
    page.getByText(
      "Saved as version 48. Updated: Reference lab results, Blood gas results, Dashboard feed. 33 unchanged.",
    ),
  ).toBeVisible();
  await expect(versionRow(page, 48)).toContainText("Current");
  await expect(versionRow(page, 48)).toContainText(
    "rollback of 44: The new mappings dropped a field",
  );
  await expect(versionRow(page, 47)).toContainText("Superseded");
});

test("H-07 an applied change appears as a version, differs from the one before and can be rolled back", async ({
  page,
}) => {
  await openEditor(page, "/endpoints/new", "New endpoint");
  await pick(page, "Kind", "Folder");
  await fill(page, { Label: "Bench inbox", "Folder path": "/srv/lab/bench" }, undefined);
  const version = await saveAndConfirm(page, "Add the bench inbox");
  expect(version).toBe(48);

  await openVersions(page);
  await expect(versionRow(page, 48)).toContainText("Current");
  await expect(versionRow(page, 48)).toContainText("LAB\\jdoe");
  await expect(versionRow(page, 48)).toContainText("Add the bench inbox");

  // Version 47 lacks the endpoint: the comparison says so, and rolling back removes it.
  await versionRow(page, 47).getByRole("gridcell").first().click();
  await expect(page.getByText("Compared with the active version (48)")).toBeVisible();
  await expect(page.getByText("Endpoints (1 removed)")).toBeVisible();
  await expect(
    page.getByText("Removed: Bench inbox bench-inbox (not in version 47)"),
  ).toBeVisible();
  await page.getByRole("button", { name: "Roll back to this version…" }).click();
  const dialog = page.getByRole("alertdialog", { name: "Roll back to version 47?" });
  await expect(dialog).toContainText("Creates a new version, 49");
  await expect(dialog).toContainText("Endpoints: 1 removed");
  await dialog
    .getByRole("textbox", { name: /^Reason for the rollback/ })
    .fill("Back out the inbox");
  await dialog.getByRole("button", { name: "Roll back to version 47" }).click();
  await expect(page.getByText("Rolled back to version 47")).toBeVisible();
  await expect(versionRow(page, 49)).toContainText("rollback of 47");
  await expect(versionRow(page, 49)).toContainText("Current");
});

// Another client applies a configuration while this editor is open: applying from the stale
// document is refused with a reload offer, and after reloading it applies.
test("H-07 a stale version is rejected with a reload offer", async ({ page }) => {
  await openEditor(page, "/endpoints/new", "New endpoint");
  await pick(page, "Kind", "Folder");
  await fill(page, { Label: "Bench inbox", "Folder path": "/srv/lab/bench" }, undefined);
  await page.evaluate(() => window.__packieMock?.applyElsewhere());
  await saveAndApply(page).click();
  const dialog = reviewDialog(page);
  await expect(dialog).toBeVisible();
  await dialog.getByRole("textbox", { name: /^Change note/ }).fill("Journey change");
  await dialog.getByRole("button", { name: /^Apply version \d+$/ }).click();
  await expect(
    page.getByRole("alert").filter({ hasText: /configuration changed since it was loaded/ }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Reload configuration" }).click();
  // The entries are kept; nothing was applied by the refused attempt, so after the reload
  // (the toast says so) the same edit goes through, on top of the version applied elsewhere.
  await expect(page.getByText("Configuration reloaded")).toBeVisible();
  await expect(page.getByRole("button", { name: "Reload configuration" })).toHaveCount(0);
  await expect(text(page, "Label")).toHaveValue("Bench inbox");
  expect(await saveAndConfirm(page)).toBe(49);
});
