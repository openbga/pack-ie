import { expect, type Locator, type Page, test } from "@playwright/test";
import { rowOfAction } from "./audit.support";
import {
  factValue,
  find,
  messageRows,
  openMessages,
  openMoreFilters,
  openResend,
  SUPERSEDED,
  tick,
} from "./messages.support";
import { eventPanel, loaded, mockHandle, navigateTo, open } from "./support";

test.use({ timezoneId: "UTC" });

/** A history big enough for the mock's revision chains: msg-40 is superseded by msg-15. */
const HISTORY = { messages: 2000 };
const CONFIRM =
  "I understand this may reissue a corrected result; resend this older version anyway";

/** msg-69 is not superseded, and a newer revision can arrive for it (the mock's `reviseMessage`). */
const LATE = "msg-69";

const resendDialog = (page: Page): Locator => page.getByRole("dialog", { name: /^Resend / });

/** The Resend button of the dialog (not its "Resend…" openers). */
const resendButton = (dialog: Locator): Locator =>
  dialog.getByRole("button", { name: "Resend", exact: true });

/** Opens the record of a message and waits for it. */
async function openRecord(page: Page, id: string): Promise<void> {
  await open(page, `/messages/${id}`, HISTORY);
  await expect(page.getByRole("button", { name: "Show original message" })).toBeVisible();
}

/** The label around a checkbox: the mark covers the input, so a click goes to the label. */
const labelOf = (box: Locator): Locator => box.locator("xpath=ancestor::label[1]");

/** Selects the delivery to resend if the dialog did not, and accepts the duplicate risk if asked. */
async function chooseDelivery(dialog: Locator): Promise<void> {
  const destinations = dialog.getByRole("group", { name: "Destinations" }).getByRole("checkbox");
  const chosen = await destinations.evaluateAll(
    (boxes) => boxes.filter((box) => box instanceof HTMLInputElement && box.checked).length,
  );
  if (chosen === 0) await labelOf(destinations.first()).click();
  const risk = dialog.getByRole("checkbox", { name: /twice/ });
  if ((await risk.count()) > 0) await labelOf(risk).click();
}

/** Closes the resend result and opens the Audit page, where the dialog's work is recorded. */
async function openAuditAfterResend(page: Page, dialog: Locator): Promise<void> {
  await dialog.getByRole("button", { name: "Done" }).click();
  await navigateTo(page, "Audit");
  await loaded(page, "Audit");
}

test("SS-01: resending from the detail page of a superseded message warns with the newer message, holds Resend until confirmed, and the audit log records it", async ({
  page,
}) => {
  await openRecord(page, SUPERSEDED.old);
  // The record says it is superseded, and links the newer message.
  const notice = page
    .locator("main")
    .getByRole("alert")
    .filter({ hasText: "Superseded by a newer revision" });
  await expect(notice).toBeVisible();
  await expect(notice.getByRole("link", { name: "Open the newer message" })).toHaveAttribute(
    "href",
    new RegExp(`${SUPERSEDED.newer}$`),
  );
  await expect(
    page.locator("main").getByRole("link", { name: "The newest revision" }),
  ).toBeVisible();

  await page.getByRole("main").getByRole("button", { name: "Resend…" }).first().click();
  const dialog = resendDialog(page);
  await expect(dialog).toBeVisible();
  const warning = dialog
    .getByRole("alert")
    .filter({ hasText: "A newer revision of this message exists" });
  await expect(warning).toContainText(
    "Resending this older version may reissue a corrected result.",
  );
  const link = warning.getByRole("link", { name: SUPERSEDED.newer });
  await expect(link).toHaveAttribute("href", new RegExp(`/messages/${SUPERSEDED.newer}$`));
  // Its received time is the newer message's own.
  const received = (await warning.locator("time").first().textContent()) ?? "";
  expect(received).toMatch(/^\d{4}-\d\d-\d\d \d\d:\d\d:\d\d UTC$/);

  // Whatever else is chosen, Resend waits for the confirmation of the older version.
  await chooseDelivery(dialog);
  await expect(resendButton(dialog)).toBeDisabled();
  await tick(dialog, CONFIRM);
  await expect(resendButton(dialog)).toBeEnabled();
  await resendButton(dialog).click();
  await expect(dialog.getByText("1 of 1 destination accepted.")).toBeVisible();

  // The audit log records the confirmation with the resend.
  await openAuditAfterResend(page, dialog);
  await rowOfAction(page, "Message resent").click();
  const panel = eventPanel(page);
  await expect(panel).toContainText("Succeeded");
  const detail = (key: string) =>
    panel
      .locator("dl > div")
      .filter({ has: page.locator("code", { hasText: new RegExp(`^${key}$`) }) });
  await expect(detail("confirm_superseded")).toContainText("true");
  await expect(panel.getByRole("link", { name: SUPERSEDED.old })).toBeVisible();
});

test("SS-01: the newer message is the one the dialog names: its record shows the same received time", async ({
  page,
}) => {
  await openRecord(page, SUPERSEDED.old);
  await page.getByRole("main").getByRole("button", { name: "Resend…" }).first().click();
  const dialog = resendDialog(page);
  const warning = dialog
    .getByRole("alert")
    .filter({ hasText: "A newer revision of this message exists" });
  await expect(warning.getByRole("link", { name: SUPERSEDED.newer })).toBeVisible();
  const received = (await warning.locator("time").first().textContent()) ?? "";
  // Leave through the link: the newer message is stored, later, and revises this one.
  await warning.getByRole("link", { name: SUPERSEDED.newer }).click();
  await expect(page).toHaveURL(new RegExp(`#/messages/${SUPERSEDED.newer}$`));
  // The record of the old message stays until the newer one has been read.
  await expect.poll(() => factValue(page.locator("main"), "Message ID")).toBe(SUPERSEDED.newer);
  expect(await factValue(page.locator("main"), "Received")).toBe(received);
  await expect(
    page.locator("main").getByRole("link", { name: "The earlier message" }),
  ).toBeVisible();
  await expect(page.getByText("Superseded by a newer revision")).toHaveCount(0);
});

test("SS-01: a message nothing supersedes is resent without the confirmation", async ({ page }) => {
  await openRecord(page, SUPERSEDED.newer);
  await page.getByRole("main").getByRole("button", { name: "Resend…" }).first().click();
  const dialog = resendDialog(page);
  await expect(dialog).toBeVisible();
  await expect(dialog.getByText("A newer revision of this message exists")).toHaveCount(0);
  await expect(dialog.getByRole("checkbox", { name: CONFIRM })).toHaveCount(0);
  await chooseDelivery(dialog);
  await expect(resendButton(dialog)).toBeEnabled();
});

test("SS-02: from a grid row, the dialog learns on opening that the message is superseded", async ({
  page,
}) => {
  await openMessages(page, "", HISTORY);
  // The message ID filter finds msg-40 alone.
  await openMoreFilters(page);
  await find(page, "Message ID", SUPERSEDED.prefix);
  await expect(messageRows(page)).toHaveCount(1);
  const dialog = await openResend(page, messageRows(page).first());
  const warning = dialog
    .getByRole("alert")
    .filter({ hasText: "A newer revision of this message exists" });
  await expect(warning).toBeVisible();
  await expect(warning.getByRole("link", { name: SUPERSEDED.newer })).toBeVisible();
  await expect(warning.locator("time")).toHaveCount(1);
  await chooseDelivery(dialog);
  await expect(resendButton(dialog)).toBeDisabled();
  await tick(dialog, CONFIRM);
  await expect(resendButton(dialog)).toBeEnabled();
});

test("SS-03: a revision that arrives while the dialog is open is refused by the engine, shown with the newer message linked, and resent once confirmed", async ({
  page,
}) => {
  await openRecord(page, LATE);
  await page.getByRole("main").getByRole("button", { name: "Resend…" }).first().click();
  const dialog = resendDialog(page);
  await expect(dialog).toBeVisible();
  // Nothing supersedes it yet: no warning, Resend is open.
  await expect(dialog.getByText("A newer revision of this message exists")).toHaveCount(0);
  await chooseDelivery(dialog);
  await expect(resendButton(dialog)).toBeEnabled();

  // Another client stores a newer revision now.
  const newer = await mockHandle(page, (handle) => handle.reviseMessage("msg-69"));
  expect(newer).toMatch(/^msg-\d+$/);

  await resendButton(dialog).click();
  // The engine refused the whole request, in its words, naming the newer message and its time.
  const refusal = dialog.getByRole("alert").filter({ hasText: "The message was not resent" });
  await expect(refusal).toContainText(
    `A newer revision of this message exists (${newer}, received `,
  );
  await expect(refusal).toContainText("confirm that it is intended to resend it anyway");
  // The dialog re-read the record: the newer message is linked and the confirmation is asked.
  const warning = dialog
    .getByRole("alert")
    .filter({ hasText: "A newer revision of this message exists" })
    .filter({ has: page.getByRole("link") });
  await expect(warning.getByRole("link", { name: newer ?? "" })).toHaveAttribute(
    "href",
    new RegExp(`/messages/${newer}$`),
  );
  await expect(
    dialog.getByRole("alert").filter({ hasText: "The message was not resent" }),
  ).toBeVisible();
  await expect(resendButton(dialog)).toBeDisabled();
  await tick(dialog, CONFIRM);
  await resendButton(dialog).click();
  await expect(dialog.getByText("1 of 1 destination accepted.")).toBeVisible();

  // The audit log has the refused request and the confirmed one.
  await openAuditAfterResend(page, dialog);
  const resent = page
    .getByRole("grid", { name: "Audit events" })
    .getByRole("row")
    .filter({ hasText: "Message resent" });
  await expect(resent.nth(0)).toContainText("Succeeded");
  await expect(resent.nth(1)).toContainText("Requested");
  await expect(resent.nth(2)).toContainText("Rejected");
});
