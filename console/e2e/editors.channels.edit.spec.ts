import { expect, test } from "@playwright/test";
import {
  expectIssue,
  expectRequired,
  fill,
  form,
  general,
  group,
  openEditor,
  reviewDialog,
  row,
  runValidate,
  saveAndApply,
  startMessageChannel,
  text,
  validate,
} from "./editors.support";
import { navigateTo } from "./support";

test("H-07 message channel: destination label, enabled, retry policy and retention, applied as a new channel", async ({
  page,
}) => {
  await startMessageChannel(page, "Bench relay");
  const destination = group(page, "Destination 1");
  await expectRequired(
    page,
    ["Label", "Initial backoff (ms)", "Maximum backoff (ms)", "Maximum attempts"],
    destination,
  );

  // Label: required, at most 100 characters.
  await fill(page, { Label: "x".repeat(101) }, destination);
  await validate(page).click();
  await expect(destination.getByText("Use at most 100 characters.")).toBeVisible();
  await fill(page, { Label: "Primary LIS" }, destination);

  // Enabled is a plain checkbox, on by default.
  const enabled = destination.getByRole("checkbox", { name: "Enabled", exact: true });
  await expect(enabled).toBeChecked();
  await expect(
    destination.getByText(
      "A disabled destination gets no new deliveries; the ones already queued wait.",
    ),
  ).toBeVisible();
  await destination.locator("label", { hasText: "Enabled" }).first().click();
  await expect(enabled).not.toBeChecked();
  await destination.locator("label", { hasText: "Enabled" }).first().click();
  await expect(enabled).toBeChecked();

  // Retry policy: attempts are required for a destination, and the engine checks the backoff pair.
  await expect(text(page, "Initial backoff (ms)", destination)).toHaveValue("2000");
  await expect(text(page, "Maximum attempts", destination)).toHaveValue("5");
  await fill(page, { "Maximum attempts": "" }, destination);
  await validate(page).click();
  await expect(destination.getByText("Enter maximum attempts.")).toBeVisible();
  await fill(
    page,
    { "Maximum attempts": "3", "Initial backoff (ms)": "9000", "Maximum backoff (ms)": "1000" },
    destination,
  );
  await runValidate(page);
  await expectIssue(
    page,
    "The longest retry delay must not be shorter than the first.",
    /destinations\[0\]\.retry\.maxMs · retry_invalid/,
  );
  await expect(saveAndApply(page)).toBeDisabled();
  await fill(page, { "Maximum backoff (ms)": "60000" }, destination);

  // Retention: either limit may stay empty; a limit is a whole number.
  const retention = group(page, "Retention");
  await fill(page, { "Keep for (days)": "0" }, retention);
  await validate(page).click();
  await expect(retention.getByText("Enter a whole number, 1 or more.")).toBeVisible();
  await fill(page, { "Keep for (days)": "30", "Keep at most (messages)": "" }, retention);
  await runValidate(page);
  await expect(page.getByText("No problems found")).toBeVisible();

  await saveAndApply(page).click();
  const dialog = reviewDialog(page);
  await expect(dialog.getByText("Started (new): Bench relay")).toBeVisible();
  await dialog.getByRole("textbox", { name: /^Change note/ }).fill("Journey change");
  await dialog.getByRole("button", { name: /^Apply version/ }).click();
  await expect(page.getByText(/^Configuration version \d+ applied$/)).toBeVisible();
  await expect(page).toHaveURL(/#\/channels\/bench-relay$/);
  await navigateTo(page, "Channels");
  await expect(page.getByRole("grid", { name: "Channels" }).getByText("Bench relay")).toBeVisible();
});

test("H-07 editing a channel: an edit restarts only that channel and each edit URL shows its own channel", async ({
  page,
}) => {
  await openEditor(page, "/channels/ch-msg-sta/edit", "STA Compact results");
  await expect(form(page).getByText("Message", { exact: true })).toBeVisible();
  await expect(page.getByText("ch-msg-sta", { exact: true })).toBeVisible();
  // Kind and id cannot be changed any more.
  await expect(form(page).getByRole("button", { name: /Kind$/ })).toHaveCount(0);
  await expect(general(page).getByRole("textbox", { name: "Id", exact: true })).toHaveCount(0);

  // Another channel's edit URL, opened from here, shows that channel and none of this one: the
  // editor is rebuilt per record id.
  await page.evaluate(() => {
    window.location.hash = "/channels/ch-msg-billing/edit";
  });
  await expect(page.getByRole("heading", { level: 1, name: "Billing export" })).toBeVisible();
  await expect(general(page).getByRole("textbox", { name: "Label", exact: true })).toHaveValue(
    "Billing export",
  );
  await expect(page.getByText("ch-msg-billing", { exact: true })).toBeVisible();
  await expect(page.getByRole("textbox", { name: "Label", exact: true })).toHaveCount(2);

  // Back on the first one, a label change is taken in place: that channel updates, none restarts.
  await page.evaluate(() => {
    window.location.hash = "/channels/ch-msg-sta/edit";
  });
  await expect(page.getByRole("heading", { level: 1, name: "STA Compact results" })).toBeVisible();
  await expect(general(page).getByRole("textbox", { name: "Label", exact: true })).toHaveValue(
    "STA Compact results",
  );
  await fill(page, { Label: "STA Compact results (renamed)" }, general(page));
  await saveAndApply(page).click();
  const dialog = reviewDialog(page);
  await expect(dialog.getByText("Updated in place: STA Compact results")).toBeVisible();
  await expect(dialog.getByText(/^Restarted/)).toHaveCount(0);
  await expect(dialog.getByText(/^Untouched: \d+ channels?, which keep running\.$/)).toBeVisible();
  await dialog.getByRole("textbox", { name: /^Change note/ }).fill("Journey change");
  await dialog.getByRole("button", { name: /^Apply version/ }).click();
  await expect(page.getByText(/^Configuration version \d+ applied$/)).toBeVisible();
  await expect(page.getByText("Updated: STA Compact results (renamed).")).toBeVisible();
});

test("H-07 disabling a channel is planned and reported as stopped, not restarted", async ({
  page,
}) => {
  await openEditor(page, "/channels/ch-msg-sta/edit", "STA Compact results");
  const enabled = general(page).getByRole("checkbox", { name: "Enabled", exact: true });
  await expect(enabled).toBeChecked();
  await general(page).locator("label", { hasText: "Enabled" }).first().click();
  await expect(enabled).not.toBeChecked();
  await saveAndApply(page).click();
  const dialog = reviewDialog(page);
  await expect(dialog.getByText("Stopped (disabled): STA Compact results")).toBeVisible();
  await expect(dialog.getByText(/^Restarted/)).toHaveCount(0);
  await expect(dialog.getByText(/^Started/)).toHaveCount(0);
  await dialog.getByRole("textbox", { name: /^Change note/ }).fill("Journey: pause the channel");
  await dialog.getByRole("button", { name: /^Apply version/ }).click();
  await expect(page.getByText(/^Configuration version \d+ applied$/)).toBeVisible();
  await expect(page.getByText(/Stopped: STA Compact results/)).toBeVisible();
});

test("H-07 removing a saved destination confirms first and says its unresolved deliveries are closed", async ({
  page,
}) => {
  await openEditor(page, "/channels/ch-msg-sta/edit", "STA Compact results");
  await expect(row(page, "Destination 2")).toBeVisible();
  await page.getByRole("button", { name: "Remove destination 2", exact: true }).click();
  const confirm = page.getByRole("alertdialog", { name: /^Remove destination 2/ });
  await expect(confirm).toContainText(
    "Unresolved deliveries of this destination are closed when it is removed",
  );
  await expect(confirm).toContainText("become cancelled and are not delivered or retried");
  await expect(confirm).toContainText("Nothing changes until you save and apply");
  await confirm.getByRole("button", { name: "Cancel" }).click();
  await expect(row(page, "Destination 2")).toBeVisible();

  await page.getByRole("button", { name: "Remove destination 2", exact: true }).click();
  await page
    .getByRole("alertdialog", { name: /^Remove destination 2/ })
    .getByRole("button", { name: "Remove destination" })
    .click();
  await expect(row(page, "Destination 2")).toHaveCount(0);
  await saveAndApply(page).click();
  const dialog = reviewDialog(page);
  await expect(dialog).toContainText(
    "Unresolved deliveries of a removed channel or destination are closed",
  );
  await expect(
    dialog
      .getByRole("listitem")
      .filter({ hasText: /^Destination .+ of STA Compact results is removed\.$/ }),
  ).toBeVisible();
});

test("H-07 deleting a channel confirms, explains the closed deliveries and removes it as a new version", async ({
  page,
}) => {
  await openEditor(page, "/channels/ch-msg-billing/edit", "Billing export");
  await page.getByRole("button", { name: "Delete channel…" }).click();
  const confirm = page.getByRole("alertdialog", { name: "Delete channel Billing export?" });
  await expect(confirm).toContainText(
    /Removes this channel from the configuration as version \d+\. The channel stops; other channels keep running\./,
  );
  await expect(confirm).toContainText(
    "Unresolved deliveries of this channel are closed when it is removed",
  );
  await expect(confirm).toContainText(
    "queued, rejected, failed, timed out and outcome unknown ones become cancelled",
  );
  await expect(confirm).toContainText(
    "stored messages remain readable until retention purges them",
  );
  await expect(confirm).toContainText("The current version stays in the history");
  await confirm.getByRole("button", { name: "Cancel" }).click();
  await expect(confirm).toHaveCount(0);

  await page.getByRole("button", { name: "Delete channel…" }).click();
  const deleting = page.getByRole("alertdialog", { name: "Delete channel Billing export?" });
  await deleting.getByRole("textbox", { name: /^Change note/ }).fill("Billing moved to the LIS");
  await deleting.getByRole("button", { name: "Delete channel" }).click();
  await expect(page.getByText(/^Configuration version \d+ applied$/)).toBeVisible();
  await expect(page).toHaveURL(/#\/channels$/);
  await expect(
    page.getByRole("grid", { name: "Channels" }).getByText("Billing export"),
  ).toHaveCount(0);
});
