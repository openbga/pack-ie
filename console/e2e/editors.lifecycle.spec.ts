import { expect, type Page, test } from "@playwright/test";
import { fill, form, openEditor, pick, saveAndConfirm, text } from "./editors.support";
import { dataRows, heading, open } from "./support";

const SECRET_VALUE = "correct-horse-battery-staple";
const secrets = (page: Page) => page.getByRole("grid", { name: "Secrets" });

async function openSecrets(page: Page): Promise<void> {
  await open(page, "/settings/secrets");
  await expect(heading(page, "Settings")).toBeVisible();
  await expect(secrets(page)).toBeVisible();
}

test("H-07 secrets are write-only: set through a password field, never shown, deleted with a warning when in use", async ({
  page,
}) => {
  await openSecrets(page);
  for (const column of ["Name", "Storage", "Value", "Used by endpoints"]) {
    await expect(secrets(page).getByRole("columnheader", { name: column })).toBeVisible();
  }
  // The grid says whether a value exists and who uses it; never the value.
  const clinic = dataRows(page, "Secrets").filter({ hasText: "clinic-sftp-key" });
  await expect(clinic).toContainText("Not set");
  await expect(dataRows(page, "Secrets").filter({ hasText: "reflab-sftp-password" })).toContainText(
    "Stored",
  );
  await expect(dataRows(page, "Secrets").filter({ hasText: "dashboard-api-token" })).toContainText(
    "Not used",
  );

  // Add a secret: the value goes in a password field and is checked for presence only.
  await page.getByRole("button", { name: "Add secret" }).click();
  const dialog = page.getByRole("dialog", { name: "Add secret" });
  await expect(dialog.getByText("Stored securely and never shown again.")).toBeVisible();
  const value = dialog.locator('input[name="value"]');
  await expect(value).toHaveAttribute("type", "password");
  await expect(value).toHaveAttribute("autocomplete", "new-password");
  await dialog.getByRole("button", { name: "Add secret" }).click();
  await expect(dialog.getByText("Enter name.")).toBeVisible();
  await expect(dialog.getByText("Enter the value.")).toBeVisible();
  await dialog.getByRole("textbox", { name: "Name" }).fill("bad name!");
  await value.fill(SECRET_VALUE);
  await dialog.getByRole("button", { name: "Add secret" }).click();
  await expect(
    dialog.getByText("Use letters, digits, dots, hyphens and underscores."),
  ).toBeVisible();
  await dialog.getByRole("textbox", { name: "Name" }).fill("bench-sftp-password");
  await dialog.getByRole("button", { name: "Add secret" }).click();
  await expect(page.getByText("Secret bench-sftp-password saved")).toBeVisible();
  await expect(dialog).toHaveCount(0);

  // It is stored, and its value is nowhere on the page, not even in a field.
  const added = dataRows(page, "Secrets").filter({ hasText: "bench-sftp-password" });
  await expect(added).toContainText("Stored");
  await expect(added).toContainText("Not used");
  expect(await page.locator("body").innerText()).not.toContain(SECRET_VALUE);
  expect(await page.locator("body").innerHTML()).not.toContain(SECRET_VALUE);

  // Setting it again starts from an empty password field: there is nothing to reveal.
  await page.getByRole("button", { name: "Actions for bench-sftp-password" }).click();
  await page.getByRole("menuitem", { name: "Replace value…" }).click();
  const replace = page.getByRole("dialog", { name: "Set value of bench-sftp-password" });
  await expect(replace.locator('input[name="value"]')).toHaveValue("");
  await expect(replace.locator('input[name="value"]')).toHaveAttribute("type", "password");
  await expect(replace.getByRole("textbox", { name: "Name" })).toHaveCount(0);
  await replace.getByRole("button", { name: "Cancel" }).click();

  // Deleting asks first; a secret in use says which endpoints lose their sign-in.
  await page.getByRole("button", { name: "Actions for reflab-sftp-password" }).click();
  await page.getByRole("menuitem", { name: "Delete secret…" }).click();
  const used = page.getByRole("alertdialog", { name: "Delete secret reflab-sftp-password?" });
  await expect(used).toContainText("The stored value is removed and cannot be recovered.");
  await expect(used.getByText("Still in use")).toBeVisible();
  await expect(used).toContainText(
    "Endpoints that use this secret cannot sign in until a new value is set: ep-reflab-sftp.",
  );
  await used.getByRole("button", { name: "Cancel" }).click();
  await expect(dataRows(page, "Secrets").filter({ hasText: "reflab-sftp-password" })).toHaveCount(
    1,
  );

  await page.getByRole("button", { name: "Actions for bench-sftp-password" }).click();
  await page.getByRole("menuitem", { name: "Delete secret…" }).click();
  const unused = page.getByRole("alertdialog", { name: "Delete secret bench-sftp-password?" });
  await expect(unused.getByText("Still in use")).toHaveCount(0);
  await unused.getByRole("button", { name: "Delete secret" }).click();
  await expect(page.getByText("Secret bench-sftp-password deleted")).toBeVisible();
  await expect(dataRows(page, "Secrets").filter({ hasText: "bench-sftp-password" })).toHaveCount(0);
});

test("H-07 the endpoint editor shows secret names only: a name chosen from Settings > Secrets, no value field", async ({
  page,
}) => {
  await openEditor(page, "/endpoints/ep-reflab-sftp/edit", "Reference lab SFTP");
  await expect(text(page, "Password secret name")).toHaveValue("reflab-sftp-password");
  await expect(form(page).getByText("never in the configuration")).toBeVisible();
  await expect(form(page).locator('input[type="password"]')).toHaveCount(0);
  await expect(form(page).getByRole("textbox", { name: /value/i })).toHaveCount(0);
});

test("H-07 leaving an editor with unsaved changes asks first, and keeps the entries on Stay", async ({
  page,
}) => {
  await openEditor(page, "/endpoints/ep-lis-out/edit", "LIS HL7 out");
  // Nothing changed: navigation is free.
  const nav = page.getByRole("navigation", { name: "Main" });
  await fill(page, { Host: "lis2.lab.local" });
  await nav.getByRole("link", { name: "Endpoints" }).click();
  const guard = page.getByRole("alertdialog", { name: "Leave without saving?" });
  await expect(guard).toContainText(
    "You changed this form and have not applied the changes. If you leave, they are lost.",
  );
  await expect(page).toHaveURL(/#\/endpoints\/ep-lis-out\/edit$/);

  // Cancel keeps the form as it was.
  await guard.getByRole("button", { name: "Cancel" }).click();
  await expect(guard).toHaveCount(0);
  await expect(text(page, "Host")).toHaveValue("lis2.lab.local");
  await expect(page).toHaveURL(/#\/endpoints\/ep-lis-out\/edit$/);

  // The form's own Cancel button is a navigation too.
  await form(page).getByRole("button", { name: "Cancel" }).click();
  await expect(guard).toBeVisible();
  await guard.getByRole("button", { name: "Cancel" }).click();

  // Leaving goes on to the page, and the entries are gone when the editor is opened again.
  await nav.getByRole("link", { name: "Endpoints" }).click();
  await guard.getByRole("button", { name: "Leave without saving" }).click();
  await expect(page).toHaveURL(/#\/endpoints$/);
  await expect(heading(page, "Endpoints")).toBeVisible();
  await page.goBack();
  await expect(heading(page, "LIS HL7 out")).toBeVisible();
  await expect(text(page, "Host")).toHaveValue("lis.lab.local");

  // A clean form, and a form that was applied, are left without a question.
  await nav.getByRole("link", { name: "Endpoints" }).click();
  await expect(heading(page, "Endpoints")).toBeVisible();
  await expect(guard).toHaveCount(0);
});

test("H-07 a clean editor closes without the browser asking", async ({ page }) => {
  await openEditor(page, "/endpoints/ep-lis-out/edit", "LIS HL7 out");
  const kinds: string[] = [];
  page.on("dialog", (dialog) => {
    kinds.push(dialog.type());
    void dialog.dismiss();
  });
  await page.close({ runBeforeUnload: true });
  // Nothing was edited, so the page closes without asking.
  expect(kinds).toEqual([]);
});

test("H-07 a form with unsaved changes holds a reload or close with the browser's own prompt", async ({
  page,
}) => {
  await openEditor(page, "/endpoints/ep-lis-out/edit", "LIS HL7 out");
  await fill(page, { Host: "lis2.lab.local" });
  const asked = new Promise<string>((resolve) => {
    page.once("dialog", (dialog) => {
      resolve(dialog.type());
      void dialog.dismiss();
    });
  });
  await page.close({ runBeforeUnload: true });
  expect(await asked).toBe("beforeunload");
});

test("H-07 deleting an endpoint that channels use is blocked, and lists those channels", async ({
  page,
}) => {
  await openEditor(page, "/endpoints/ep-lis-out/edit", "LIS HL7 out");
  await page.getByRole("button", { name: "Delete endpoint…" }).click();
  const blocked = page.getByRole("dialog", { name: "LIS HL7 out is in use" });
  await expect(blocked).toContainText("This endpoint cannot be deleted while channels use it.");
  await expect(blocked).toContainText(
    "Remove it from these channels first, or delete those channels:",
  );
  const channels = blocked.getByRole("listitem");
  await expect(channels.filter({ hasText: "Vitros 350 results" })).toContainText("(Destination 1)");
  await expect(channels.filter({ hasText: "Reference lab results" })).toBeVisible();
  expect(await channels.count()).toBeGreaterThan(10);
  await expect(blocked.getByRole("button", { name: "Delete endpoint" })).toHaveCount(0);
  const link = blocked.getByRole("link", { name: "Vitros 350 results" });
  await expect(link).toHaveAttribute("href", "#/channels/ch-msg-vitros350/edit");

  // The link leads to that channel's editor.
  await link.click();
  await expect(heading(page, "Vitros 350 results")).toBeVisible();
  await expect(page).toHaveURL(/#\/channels\/ch-msg-vitros350\/edit$/);
});

test("H-07 deleting an endpoint nothing uses confirms and applies it as a new version", async ({
  page,
}) => {
  // A new endpoint, applied in this session, is used by no channel.
  await openEditor(page, "/endpoints/new", "New endpoint");
  await pick(page, "Kind", "Folder");
  await fill(page, { Label: "Bench scratch", "Folder path": "/srv/lab/scratch" });
  await saveAndConfirm(page);
  await page.evaluate(() => {
    window.location.hash = "/endpoints/bench-scratch/edit";
  });
  await expect(heading(page, "Bench scratch")).toBeVisible();

  await page.getByRole("button", { name: "Delete endpoint…" }).click();
  const confirm = page.getByRole("alertdialog", { name: "Delete endpoint Bench scratch?" });
  await expect(confirm).toContainText(
    /Removes this endpoint from the configuration as version 49\. No channel uses it, so no channel is restarted\./,
  );
  await expect(confirm).toContainText(
    "The current version stays in the history; Settings › Versions can restore it.",
  );
  await confirm.getByRole("button", { name: "Cancel" }).click();

  await page.getByRole("button", { name: "Delete endpoint…" }).click();
  const deleting = page.getByRole("alertdialog", { name: "Delete endpoint Bench scratch?" });
  await deleting.getByRole("textbox", { name: /^Change note/ }).fill("Scratch endpoint retired");
  await deleting.getByRole("button", { name: "Delete endpoint" }).click();
  await expect(page.getByText("Configuration version 49 applied")).toBeVisible();
  await expect(page).toHaveURL(/#\/endpoints$/);
  await expect(dataRows(page, "Endpoints").filter({ hasText: "Bench scratch" })).toHaveCount(0);
});
