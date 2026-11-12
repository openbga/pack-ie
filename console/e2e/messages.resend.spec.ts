import { expect, type Locator, test } from "@playwright/test";
import {
  columnTexts,
  messageRows,
  openMessages,
  openResend,
  rowIdentifier,
  rowWithChip,
  tick,
} from "./messages.support";

// ── Resend dialog ────────────────────────────────────────────────────────────────────────

const SAME = "Try again with the same output";
const NEW = "Process the original again with the current configuration";
const RISK_TEXT = "The destination may already have this message";
const RISK_CHECKBOX =
  /I have checked the destination and accept that it may receive this message twice/;

/** Delivery states whose resend needs the duplicate-risk confirmation (message-channels.md §9). */
const RISKY = [
  "Acknowledged",
  "No acknowledgement in time, may have been delivered",
  "Outcome unknown, may have been delivered",
];

/** The destinations the dialog lists, each with the state it shows under its name. */
async function destinationsOf(dialog: Locator): Promise<{ name: string; state: string }[]> {
  // Each option is the checkbox with its label and the state under it.
  const options = dialog
    .getByRole("group", { name: "Destinations" })
    .locator("label")
    .locator("xpath=..");
  const texts = await options.allInnerTexts();
  return texts.map((text) => {
    const [name = "", ...state] = text
      .split("\n")
      .map((line) => line.trim())
      .filter(Boolean);
    return { name, state: state.join(" ") };
  });
}

test("H-05: the resend dialog offers both modes with one sentence each", async ({ page }) => {
  await openMessages(page, "?delivery=failed");
  const row = rowWithChip(page, /: Failed/);
  const dialog = await openResend(page, row);
  await expect(dialog.getByText("Mode", { exact: true })).toBeVisible();
  const same = dialog.getByRole("radio", { name: new RegExp(`^Same message ${SAME}`) });
  const fresh = dialog.getByRole("radio", { name: new RegExp(`^New message ${NEW}`) });
  await expect(same).toBeChecked();
  await expect(fresh).not.toBeChecked();
  await expect(dialog.getByText(SAME, { exact: true })).toBeVisible();
  await expect(dialog.getByText(NEW, { exact: true })).toBeVisible();
  // The button names what it does in each mode.
  await expect(dialog.getByRole("button", { name: "Resend", exact: true })).toBeVisible();
  await dialog.locator("label").filter({ hasText: "New message" }).click();
  await expect(fresh).toBeChecked();
  await expect(dialog.getByRole("button", { name: "Create new message" })).toBeVisible();
  // A failed delivery was provably not delivered: no duplicate risk to confirm.
  await expect(dialog.getByText(RISK_TEXT)).toHaveCount(0);
  await dialog.getByRole("button", { name: "Cancel" }).click();
  await expect(dialog).toHaveCount(0);
});

test("H-05: the duplicate-risk confirmation appears exactly for timed out, unknown and acknowledged deliveries", async ({
  page,
}) => {
  const seen = new Set<string>();
  for (const [filter, chip] of [
    ["delivery=failed", /: Failed/],
    ["delivery=rejected", /: Rejected/],
    ["delivery=cancelled", /: Closed/],
    ["delivery=timed_out", /: No acknowledgement/],
    ["delivery=outcome_unknown", /: Outcome unknown/],
    ["delivery=acknowledged", /: Acknowledged/],
  ] as const) {
    await openMessages(page, `?${filter}`);
    const dialog = await openResend(page, rowWithChip(page, chip));
    const destinations = await destinationsOf(dialog);
    expect(destinations.length).toBeGreaterThan(0);
    // Each destination alone: the confirmation is there if and only if its state is a risky one.
    for (const wanted of destinations) {
      for (const other of destinations) {
        await tick(dialog, other.name, other.name === wanted.name);
      }
      const risky = RISKY.includes(wanted.state);
      await expect(dialog.getByText(RISK_TEXT)).toHaveCount(risky ? 1 : 0);
      await expect(dialog.getByRole("checkbox", { name: RISK_CHECKBOX })).toHaveCount(
        risky ? 1 : 0,
      );
      seen.add(wanted.state);
    }
    await dialog.getByRole("button", { name: "Cancel" }).click();
  }
  // Every state of the table was met, so the rule was checked in both directions.
  for (const state of RISKY) expect(seen, state).toContain(state);
  expect([...seen].some((state) => !RISKY.includes(state))).toBe(true);
});

test("H-05: the duplicate-risk confirmation is required before Resend works", async ({ page }) => {
  await openMessages(page, "?delivery=outcome_unknown");
  const dialog = await openResend(page, rowWithChip(page, /: Outcome unknown/));
  // Preselected: the delivery that needs attention.
  await expect(dialog.getByText(RISK_TEXT)).toBeVisible();
  await expect(dialog.getByRole("checkbox", { name: RISK_CHECKBOX })).not.toBeChecked();
  await dialog.getByRole("button", { name: "Resend", exact: true }).click();
  await expect(
    dialog.getByText("Confirm the duplicate risk to resend these deliveries."),
  ).toBeVisible();
  // Nothing was sent: the choices are still there and no result is.
  await expect(dialog.getByRole("list", { name: "Result per destination" })).toHaveCount(0);
  await dialog.locator("label").filter({ hasText: RISK_CHECKBOX }).click();
  await expect(dialog.getByRole("checkbox", { name: RISK_CHECKBOX })).toBeChecked();
  await dialog.getByRole("button", { name: "Resend", exact: true }).click();
  await expect(dialog.getByRole("list", { name: "Result per destination" })).toBeVisible();
  await expect(dialog.getByText("1 of 1 destination accepted.")).toBeVisible();
});

test("H-05: a queued or sending delivery is resent only as a new message", async ({ page }) => {
  await openMessages(page, "?delivery=sending");
  const row = rowWithChip(page, /: Sending/);
  const dialog = await openResend(page, row);
  // Nothing is preselected (it needs no attention); pick the delivery in progress.
  const [first] = await destinationsOf(dialog);
  await tick(dialog, first?.name ?? "");
  await dialog.getByRole("button", { name: "Resend", exact: true }).click();
  await expect(dialog.getByText(/cannot be resent as the same message/)).toBeVisible();
  await dialog.locator("label").filter({ hasText: "New message" }).click();
  await dialog.getByRole("button", { name: "Create new message" }).click();
  await expect(dialog.getByRole("list", { name: "Result per destination" })).toBeVisible();
  await expect(dialog.getByRole("link", { name: "Open the new message" })).toBeVisible();
});

test("H-05: an invalid message offers only a new message", async ({ page }) => {
  for (const state of ["invalid", "aborted"]) {
    await openMessages(page, `?state=${state}`);
    const dialog = await openResend(page, messageRows(page).first());
    await expect(dialog.getByRole("radio")).toHaveCount(0);
    await expect(dialog.getByText("New message.", { exact: false }).first()).toBeVisible();
    await expect(dialog.getByText(NEW, { exact: false }).first()).toBeVisible();
    await expect(dialog.getByText(SAME)).toHaveCount(0);
    await expect(
      dialog.getByText(
        /can only be processed again as a new message|processed again from its original/,
      ),
    ).toBeVisible();
    await expect(dialog.getByRole("button", { name: "Create new message" })).toBeVisible();
    await expect(dialog.getByRole("button", { name: "Resend", exact: true })).toHaveCount(0);
    await dialog.getByRole("button", { name: "Cancel" }).click();
  }
});

test("H-05: the result lists every destination and what happened", async ({ page }) => {
  await openMessages(page, "?channel=ch-msg-au680&delivery=acknowledged");
  // A message with two destinations, both resent as a new message.
  const row = messageRows(page)
    .filter({ has: page.getByRole("listitem", { name: /: Acknowledged/ }) })
    .filter({ hasText: /Results archive folder/ })
    .first();
  const dialog = await openResend(page, row);
  await tick(dialog, "LIS HL7 out");
  await tick(dialog, "Results archive folder");
  await dialog.locator("label").filter({ hasText: "New message" }).click();
  // Acknowledged deliveries may already have been processed: confirm, then go.
  await dialog.locator("label").filter({ hasText: RISK_CHECKBOX }).click();
  await dialog.getByRole("button", { name: "Create new message" }).click();
  const results = dialog.getByRole("list", { name: "Result per destination" });
  await expect(results).toBeVisible();
  await expect(dialog.getByText("2 of 2 destinations accepted.")).toBeVisible();
  const items = results.getByRole("listitem");
  await expect(items).toHaveCount(2);
  await expect(items.filter({ hasText: "LIS HL7 out" })).toContainText("Accepted");
  await expect(items.filter({ hasText: "Results archive folder" })).toContainText(
    "The new message is processed with the current configuration",
  );
  // A toast words the outcome too, and the new message can be opened.
  await expect(page.getByText("Resend accepted for 2 destinations")).toBeVisible();
  await dialog.getByRole("link", { name: "Open the new message" }).click();
  await expect(page).toHaveURL(/#\/messages\/msg-new-1/);
});

test("H-05: the same-message result says the new attempt cycle starts", async ({ page }) => {
  await openMessages(page, "?delivery=rejected");
  const row = rowWithChip(page, /: Rejected/);
  const id = await rowIdentifier(row);
  const dialog = await openResend(page, row);
  await dialog.getByRole("button", { name: "Resend", exact: true }).click();
  const results = dialog.getByRole("list", { name: "Result per destination" });
  await expect(results.getByRole("listitem").first()).toContainText(
    "Queued again. A new attempt cycle starts with the stored output.",
  );
  await dialog.getByRole("button", { name: "Done" }).click();
  await expect(dialog).toHaveCount(0);
  // The row left the list of rejected deliveries (its delivery is queued again).
  await expect.poll(async () => (await columnTexts(page, "Identifier")).includes(id)).toBe(false);
});

test("H-05: the optional reason counts up to 500 characters", async ({ page }) => {
  await openMessages(page, "?delivery=failed");
  const dialog = await openResend(page, rowWithChip(page, /: Failed/));
  const reason = dialog.getByRole("textbox", { name: /Reason \(optional\)/ });
  await expect(dialog.getByText(/0 of 500 characters\./)).toBeVisible();
  await reason.fill("Mapping corrected, send again");
  await expect(dialog.getByText(/29 of 500 characters\./)).toBeVisible();
  // 500 is the most: 501 is refused where it is typed, and nothing is sent.
  await reason.fill("x".repeat(501));
  await expect(dialog.getByText(/501 of 500 characters\./)).toBeVisible();
  await dialog.getByRole("button", { name: "Resend", exact: true }).click();
  await expect(dialog.getByText(/Use 500 characters or fewer\./)).toBeVisible();
  await expect(dialog.getByRole("list", { name: "Result per destination" })).toHaveCount(0);
  await reason.fill("x".repeat(500));
  await expect(dialog.getByText(/500 of 500 characters\./)).toBeVisible();
  await expect(dialog.getByText(/Use 500 characters or fewer\./)).toHaveCount(0);
  await dialog.getByRole("button", { name: "Resend", exact: true }).click();
  await expect(dialog.getByRole("list", { name: "Result per destination" })).toBeVisible();
});
