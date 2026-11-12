import { expect, type Page, test } from "@playwright/test";
import { axeViolations } from "./a11y.support";
import { openAudit } from "./audit.support";
import { downloadFrom } from "./download.support";
import { fill, openEditor, pick, saveAndConfirm } from "./editors.support";
import { open } from "./support";

// A pending button announces "Saving…" through a live region that points at the button with
// aria-labelledby. When the dialog closes with the command, the button is gone and the
// announcement must go with it: a label that names nothing is an accessibility violation
// (axe: role-img-alt, aria-valid-attr-value) until React Aria's 7 s timer removes it.

/** How long React Aria keeps an announcement; the journeys look again after this. */
const ANNOUNCEMENT_MS = 12_000;

/** The axe violations of the rules a dangling label breaks, and every dangling reference itself. */
async function danglingLabels(page: Page): Promise<string[]> {
  const found = await axeViolations(page, { rules: ["role-img-alt", "aria-valid-attr-value"] });
  const dangling = await page.evaluate(() => {
    const ids: string[] = [];
    for (const node of document.querySelectorAll("[aria-labelledby]")) {
      for (const id of (node.getAttribute("aria-labelledby") ?? "").split(/\s+/)) {
        if (id !== "" && document.getElementById(id) === null) {
          ids.push(
            `dangling aria-labelledby ${id} on ${node.tagName}[role=${node.getAttribute("role")}]`,
          );
        }
      }
    }
    return ids;
  });
  return [...found, ...dangling];
}

/** Checks right after a dialog closed, a moment later, and once the announcements have expired. */
async function expectCleanAfterClose(page: Page): Promise<void> {
  expect(await danglingLabels(page)).toEqual([]);
  await page.waitForTimeout(400);
  expect(await danglingLabels(page)).toEqual([]);
  await page.waitForTimeout(ANNOUNCEMENT_MS);
  expect(await danglingLabels(page)).toEqual([]);
}

test("A11-01 a verification sign-off leaves no dangling live-region label", async ({ page }) => {
  await open(page, "/settings/versions?version=47");
  const detail = page.getByRole("region", { name: "Version 47" });
  await detail.getByRole("button", { name: "Mark as verified…" }).click();
  const dialog = page.getByRole("alertdialog", { name: "Mark version 47 as verified?" });
  await dialog.getByRole("textbox", { name: /^Verification reference/ }).fill("VR-2026-031");
  await dialog.getByRole("button", { name: "Mark version 47 as verified" }).click();
  await expect(dialog).toHaveCount(0);
  await expectCleanAfterClose(page);
});

test("A11-01 a rollback confirmation leaves no dangling live-region label", async ({ page }) => {
  await open(page, "/settings/versions?version=44");
  await page.getByRole("button", { name: "Roll back to this version…" }).click();
  const dialog = page.getByRole("alertdialog", { name: "Roll back to version 44?" });
  await dialog.getByRole("textbox", { name: /^Reason for the rollback/ }).fill("Dropped a field");
  await dialog.getByRole("button", { name: "Roll back to version 44" }).click();
  await expect(dialog).toHaveCount(0);
  await expectCleanAfterClose(page);
});

test("A11-01 an apply review leaves no dangling live-region label", async ({ page }) => {
  await openEditor(page, "/endpoints/new", "New endpoint");
  await pick(page, "Kind", "Folder");
  await fill(page, { Label: "Bench inbox", "Folder path": "/srv/lab/bench" });
  await saveAndConfirm(page, "Add the bench inbox");
  await expect(page.getByRole("alertdialog")).toHaveCount(0);
  await expectCleanAfterClose(page);
});

test("A11-01 an export leaves no dangling live-region label", async ({ page }) => {
  await openAudit(page, "?action=config_apply");
  await page.getByRole("button", { name: "Export…" }).click();
  const dialog = page.getByRole("dialog", { name: "Export audit events" });
  await downloadFrom(page, dialog.getByRole("button", { name: "Export" }));
  await expect(dialog).toHaveCount(0);
  await expectCleanAfterClose(page);
});
