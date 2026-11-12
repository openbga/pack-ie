import { expect, type Page, test } from "@playwright/test";
import {
  confirmDialog,
  ENDPOINT,
  expectDismissals,
  expectEscapeReturnsFocus,
  MESSAGE_CHANNEL,
  visit,
} from "./states.support";
import { dataRows, heading, navigateTo } from "./support";

test.describe("H-08 confirm: secrets, rollback, closing and resending", () => {
  test("H-08 confirm: Delete secret says the value cannot be recovered and what still uses it", async ({
    page,
  }) => {
    await visit(page, "/settings/secrets");
    const name = "billing-sftp-password";
    const cell = page.getByRole("gridcell", { name: `Actions for ${name}` });
    await page.getByRole("button", { name: `Actions for ${name}` }).click();
    await page.getByRole("menuitem", { name: "Delete secret…" }).click();
    const dialog = confirmDialog(page, `Delete secret ${name}?`);
    await expect(dialog).toContainText("The stored value is removed and cannot be recovered.");
    await expect(dialog).toContainText(
      /Endpoints that use this secret cannot sign in until a new value is set/,
    );
    await expectEscapeReturnsFocus(page, dialog, cell);

    await page.getByRole("button", { name: `Actions for ${name}` }).click();
    await page.getByRole("menuitem", { name: "Delete secret…" }).click();
    await dialog.getByRole("button", { name: "Cancel" }).click();
    await expect(dialog).toHaveCount(0);
    await expect(dataRows(page, "Secrets").filter({ hasText: name })).toHaveCount(1);

    await page.getByRole("button", { name: `Actions for ${name}` }).click();
    await page.getByRole("menuitem", { name: "Delete secret…" }).click();
    await dialog.getByRole("button", { name: "Delete secret" }).click();
    await expect(page.getByText(`Secret ${name} deleted`)).toBeVisible();
    await expect(dataRows(page, "Secrets").filter({ hasText: name })).toHaveCount(0);
  });

  test("H-08 confirm: Roll back says what version it creates and what restarts", async ({
    page,
  }) => {
    await visit(page, "/settings/versions?version=46");
    const trigger = page.getByRole("button", { name: "Roll back to this version…" });
    await trigger.click();
    const dialog = confirmDialog(page, "Roll back to version 46?");
    await expect(dialog).toContainText(
      /Creates a new version, 48, with the configuration of version 46/,
    );
    await expect(dialog).toContainText("History is kept: version 47 stays available.");
    await expect(dialog).toContainText(/Updated in place: Dashboard feed/);
    await expectEscapeReturnsFocus(page, dialog, trigger);

    await trigger.click();
    await dialog.getByRole("button", { name: "Cancel" }).click();
    await expect(dialog).toHaveCount(0);
    await expect(page.getByText("Rolled back to version 46")).toHaveCount(0);
    await expect(
      dataRows(page, "Configuration versions").filter({ hasText: "Version 48" }),
    ).toHaveCount(0);

    await trigger.click();
    await dialog
      .getByRole("textbox", { name: /^Reason for the rollback/ })
      .fill("The mapping dropped a field");
    await dialog.getByRole("button", { name: "Roll back to version 46" }).click();
    await expect(page.getByText("Rolled back to version 46")).toBeVisible();
    await expect(
      dataRows(page, "Configuration versions").filter({ hasText: "Version 48" }),
    ).toHaveCount(1);
  });

  test("H-08 confirm: Close delivery says it does not contact the destination", async ({
    page,
  }) => {
    await visit(page, "/messages?quick=attention&range=any");
    await page.getByRole("grid", { name: "Messages" }).getByRole("row").nth(1).click();
    const trigger = page.getByRole("button", { name: /^Close delivery to / }).first();
    await trigger.click();
    const dialog = page.getByRole("dialog", { name: /^Close delivery to / });
    await expect(dialog).toContainText("Closing does not contact the destination.");
    await expect(dialog).toContainText(/Closing stops further attempts|No more attempts are made/);
    await expect(dialog.getByRole("textbox", { name: "Reason" })).toBeVisible();
    await expectDismissals(page, dialog, trigger);
    await expect(page.getByText(/^Delivery to .* closed$/)).toHaveCount(0);

    // A reason is required; with it the outcome is told in words.
    await trigger.click();
    await dialog.getByRole("button", { name: "Close delivery" }).click();
    await expect(dialog).toBeVisible();
    await dialog.getByRole("textbox", { name: "Reason" }).fill("Checked with the lab: not needed");
    await dialog.getByRole("button", { name: "Close delivery" }).click();
    await expect(page.getByText(/^Delivery to .* closed$/)).toBeVisible();
    await expect(page.getByText("The destination was not contacted.")).toBeVisible();
  });

  test("H-08 confirm: Resend with duplicate risk asks for the risk to be accepted", async ({
    page,
  }) => {
    await visit(page, "/messages?quick=attention&range=any");
    await page.getByRole("grid", { name: "Messages" }).getByRole("row").nth(1).click();
    const trigger = page.getByRole("button", { name: /^Resend to / }).first();
    await trigger.click();
    const dialog = page.getByRole("dialog", { name: /^Resend / });
    await expect(dialog).toContainText("The destination may already have this message");
    await expect(dialog).toContainText(/would receive it a second time/);
    await expect(dialog.getByRole("radio", { name: /^Same message/ })).toBeChecked();
    await expectDismissals(page, dialog, trigger);
    await expect(page.getByText(/^Resend accepted/)).toHaveCount(0);

    // Without accepting the risk nothing is resent; accepting it resends.
    await trigger.click();
    await dialog.getByRole("button", { name: "Resend", exact: true }).click();
    await expect(dialog).toContainText("Confirm the duplicate risk to resend these deliveries.");
    await expect(page.getByText(/^Resend accepted/)).toHaveCount(0);
    // NOTE: the label is clicked, not the input: once the validation message shows, it covers
    // the centre of the (visually hidden) input and Playwright's own click is intercepted.
    await dialog
      .getByText("I have checked the destination and accept that it may receive this message twice")
      .click();
    await expect(
      dialog.getByRole("checkbox", { name: /I have checked the destination/ }),
    ).toBeChecked();
    await dialog.getByRole("button", { name: "Resend", exact: true }).click();
    await expect(page.getByText(/^Resend accepted for 1 destination/)).toBeVisible();
  });
});

interface EditorCase {
  name: string;
  route: string;
  /** Types something into the form so it holds unsaved input. */
  dirty: (page: Page) => Promise<void>;
  /** The page left on a deliberate exit. */
  leaves: { link: string; heading: string };
}

const EDITORS: readonly EditorCase[] = [
  {
    name: "edit channel",
    route: `/channels/${MESSAGE_CHANNEL.id}/edit`,
    dirty: (page) => page.getByRole("textbox", { name: "Label" }).first().fill("Renamed channel"),
    leaves: { link: "Endpoints", heading: "Endpoints" },
  },
  {
    name: "edit endpoint",
    route: `/endpoints/${ENDPOINT.id}/edit`,
    dirty: (page) => page.getByRole("textbox", { name: "Label" }).first().fill("Renamed endpoint"),
    leaves: { link: "Channels", heading: "Channels" },
  },
  {
    name: "new channel",
    route: "/channels/new",
    dirty: (page) => page.getByRole("textbox", { name: /^Label/ }).fill("Draft channel"),
    leaves: { link: "Messages", heading: "Messages" },
  },
  {
    name: "new endpoint",
    route: "/endpoints/new",
    dirty: (page) => page.getByRole("textbox", { name: /^Label/ }).fill("Draft endpoint"),
    leaves: { link: "Logs", heading: "Logs" },
  },
];

test.describe("H-08 confirm: leaving an editor with unsaved changes asks first", () => {
  for (const editor of EDITORS) {
    test(`H-08 confirm: discarding unsaved changes of ${editor.name}`, async ({ page }) => {
      await visit(page, editor.route);
      await expect(page.getByRole("form")).toBeVisible();
      await editor.dirty(page);
      const nav = page.getByRole("navigation", { name: "Main" });
      const link = nav.getByRole("link", { name: editor.leaves.link });
      await link.click();
      const dialog = confirmDialog(page, "Leave without saving?");
      await expect(dialog).toContainText(
        "You changed this form and have not applied the changes. If you leave, they are lost.",
      );
      await expect(dialog.getByRole("button", { name: "Leave without saving" })).toBeVisible();
      await expectEscapeReturnsFocus(page, dialog, link);
      await expect(page).toHaveURL(new RegExp(`#${editor.route}$`));

      // Cancel keeps the page and the typed input.
      await link.click();
      await dialog.getByRole("button", { name: "Cancel" }).click();
      await expect(dialog).toHaveCount(0);
      await expect(page).toHaveURL(new RegExp(`#${editor.route}$`));
      await expect(page.getByRole("textbox", { name: /^Label/ }).first()).toHaveValue(
        /Renamed|Draft/,
      );

      // Leave discards.
      await link.click();
      await dialog.getByRole("button", { name: "Leave without saving" }).click();
      await expect(heading(page, editor.leaves.heading)).toBeVisible();
    });
  }

  test("H-08 confirm: the editors' Cancel button asks before discarding", async ({ page }) => {
    await visit(page, `/channels/${MESSAGE_CHANNEL.id}/edit`);
    await expect(page.getByRole("form")).toBeVisible();
    await page.getByRole("textbox", { name: "Label" }).first().fill("Renamed channel");
    const cancel = page.getByRole("button", { name: "Cancel" });
    await cancel.click();
    const dialog = confirmDialog(page, "Leave without saving?");
    await expectEscapeReturnsFocus(page, dialog, cancel);
  });

  test("H-08 confirm: an editor with no changes is left without asking", async ({ page }) => {
    await visit(page, `/channels/${MESSAGE_CHANNEL.id}/edit`);
    await expect(page.getByRole("form")).toBeVisible();
    await navigateTo(page, "Endpoints");
    await expect(heading(page, "Endpoints")).toBeVisible();
  });
});

test.describe("H-08 confirm: the engine gate", () => {
  test("H-08 confirm: Start the service explains itself, shows it is working and continues by itself", async ({
    page,
  }) => {
    // NOTE: "Start the service" has no confirmation dialog. It is not destructive (it only
    // starts the installed service), and the page already says what it does and which command
    // to run by hand. The journey proves that text and the outcome instead of a dialog.
    await visit(page, "/dashboard", { offline: true });
    const start = page.getByRole("button", { name: "Start the service" });
    await expect(
      page.getByText(/Starting the installed service needs administrator rights/),
    ).toBeVisible();
    await expect(page.locator("code")).toContainText("service start");
    await start.click();
    await expect(start).toBeDisabled();
    await expect(heading(page, "Dashboard")).toBeVisible();
    await expect(page.getByRole("region", { name: "Notifications" })).toContainText(
      "Start service",
    );
    await expect(page.getByRole("grid", { name: "Channels" })).toBeVisible();
  });
});
