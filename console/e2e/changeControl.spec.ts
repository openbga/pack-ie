import { expect, test } from "@playwright/test";
import {
  fill,
  openEditor,
  openVersions,
  reviewDialog,
  saveAndApply,
  saveAndConfirm,
  versionRow,
} from "./editors.support";
import { heading, open } from "./support";

// Change control (decision #16, docs/spec/configuration.md section 10): who changed what, when,
// why, and who checked it afterwards. The mock engine's account is LAB\jdoe.
const ME = "LAB\\jdoe";

test("CC-01 the top bar names the account the engine records the console's actions under", async ({
  page,
}) => {
  await open(page, "/dashboard");
  await expect(heading(page, "Dashboard")).toBeVisible();
  await expect(page.getByText(`Signed in as ${ME}`)).toBeVisible();
});

test("CC-02 an apply is refused without a note, then recorded with who, when and why", async ({
  page,
}) => {
  await openEditor(page, "/endpoints/new", "New endpoint");
  await page.getByRole("button", { name: /Kind$/ }).click();
  await page.getByRole("option", { name: "Folder", exact: true }).click();
  await fill(page, { Label: "Bench inbox", "Folder path": "/srv/lab/bench" });
  await saveAndApply(page).click();

  const dialog = reviewDialog(page);
  const note = dialog.getByRole("textbox", { name: /^Change note/ });
  await expect(note).toBeVisible();
  await expect(dialog.getByText("1 to 500 characters")).toBeVisible();
  // Nothing is applied: the dialog stays, says why, and focus goes to the field.
  await dialog.getByRole("button", { name: /^Apply version \d+$/ }).click();
  await expect(dialog.getByText("Enter why you are making this change.")).toBeVisible();
  await expect(note).toBeFocused();
  await expect(page.getByText(/^Configuration version \d+ applied$/)).toHaveCount(0);
  // Too long is refused as well, with the count.
  await note.fill("x".repeat(501));
  await expect(
    dialog.getByText(/Shorten the note to 500 characters or fewer \(now 501\)/),
  ).toBeVisible();
  await dialog.getByRole("button", { name: /^Apply version \d+$/ }).click();
  await expect(page.getByText(/^Configuration version \d+ applied$/)).toHaveCount(0);
  await note.fill("Add the bench inbox for the ward 3 export");
  await dialog.getByRole("button", { name: /^Apply version \d+$/ }).click();
  await expect(page.getByText("Configuration version 48 applied")).toBeVisible();

  await openVersions(page);
  const row = versionRow(page, 48);
  await expect(row).toContainText("Current");
  await expect(row).toContainText(ME);
  await expect(row).toContainText("Add the bench inbox for the ward 3 export");
  await expect(row).toContainText("Not verified since change");
  // The detail says the same with the time to the second.
  await row.getByRole("gridcell").first().click();
  const detail = page.getByRole("region", { name: "Version 48" });
  await expect(detail.getByText(ME, { exact: true })).toBeVisible();
  await expect(detail.getByText("Not verified since the change.")).toBeVisible();
});

test("CC-03 a version is marked as verified with a reference, and the history shows who and when", async ({
  page,
}) => {
  await open(page, "/settings/versions?version=47");
  const detail = page.getByRole("region", { name: "Version 47" });
  await expect(detail.getByText("Not verified since the change.")).toBeVisible();
  await expect(versionRow(page, 47)).toContainText("Not verified since change");
  // An older version was signed off by a colleague.
  await expect(versionRow(page, 40)).toContainText("Verified");

  await detail.getByRole("button", { name: "Mark as verified…" }).click();
  const dialog = page.getByRole("alertdialog", { name: "Mark version 47 as verified?" });
  await expect(dialog).toContainText("Signs this version off as working as intended");
  await dialog.getByRole("button", { name: "Mark version 47 as verified" }).click();
  await expect(
    dialog.getByText("Enter a verification reference, such as a validation record number."),
  ).toBeVisible();
  await dialog.getByRole("textbox", { name: /^Verification reference/ }).fill("VR-2026-031");
  await dialog.getByRole("button", { name: "Mark version 47 as verified" }).click();
  await expect(page.getByText("Version 47 marked as verified")).toBeVisible();
  await expect(dialog).toHaveCount(0);

  await expect(versionRow(page, 47)).toContainText("Verified");
  await expect(versionRow(page, 47)).not.toContainText("Not verified");
  const sign = detail.getByRole("list", { name: "Verifications" });
  await expect(sign).toContainText(ME);
  await expect(sign).toContainText("VR-2026-031");
  // It can be signed off again.
  await detail.getByRole("button", { name: "Mark as verified…" }).click();
  await dialog.getByRole("textbox", { name: /^Verification reference/ }).fill("VR-2026-032");
  await dialog.getByRole("button", { name: "Mark version 47 as verified" }).click();
  await expect(versionRow(page, 47)).toContainText("Verified (2)");
  await expect(sign.getByRole("listitem")).toHaveCount(2);
});

test("CC-04 the version detail compares a version with the active configuration, read-only", async ({
  page,
}) => {
  await openEditor(page, "/endpoints/new", "New endpoint");
  await page.getByRole("button", { name: /Kind$/ }).click();
  await page.getByRole("option", { name: "Folder", exact: true }).click();
  await fill(page, { Label: "Bench inbox", "Folder path": "/srv/lab/bench" });
  await saveAndConfirm(page, "Add the bench inbox");

  await openVersions(page, 47);
  const detail = page.getByRole("region", { name: "Version 47" });
  await expect(detail.getByText("Compared with the active version (48)")).toBeVisible();
  await expect(detail.getByText("Each line reads: field, the active value")).toBeVisible();
  await expect(detail.getByRole("region", { name: "Endpoints that differ" })).toContainText(
    "Removed: Bench inbox bench-inbox (not in version 47)",
  );
  // Nothing in the detail can be edited.
  await expect(detail.getByRole("textbox")).toHaveCount(0);
  await expect(detail.getByRole("checkbox")).toHaveCount(0);
});

test("CC-05 the settings that apply a configuration ask why too", async ({ page }) => {
  await open(page, "/settings/engine");
  const queue = page.getByRole("textbox", { name: /^Delivery queue warning/ });
  await queue.fill("750");
  await page.getByRole("button", { name: "Save thresholds" }).click();
  await expect(page.getByText("Enter change note.")).toBeVisible();
  await page
    .getByRole("textbox", { name: /^Change note/ })
    .fill("Raise the warning after the backlog");
  await page.getByRole("button", { name: "Save thresholds" }).click();
  await expect(page.getByText("Configuration version 48 applied")).toBeVisible();
  await openVersions(page);
  await expect(versionRow(page, 48)).toContainText("Raise the warning after the backlog");
});
