import { expect, type Locator, type Page, test } from "@playwright/test";
import { ACCEPTED, factValue, notice, openDetail, RECEIVED, WRITTEN } from "./messages.support";

// ── D-08: what an acknowledgement proved ─────────────────────────────────────────────────

/** Words that would claim more than a lower acknowledgement level proved. */
const OVERCLAIM = /delivered|success/i;

async function expectCardSaysOnly(card: Locator, sentence: string): Promise<void> {
  await expect(card.getByText(sentence, { exact: true })).toBeVisible();
  await expect(card.getByText("Acknowledged", { exact: true }).first()).toBeVisible();
  expect(await card.innerText()).not.toMatch(OVERCLAIM);
}

test("D-08: a file destination is 'Written to the destination. Not confirmed by it.'", async ({
  page,
}) => {
  await openDetail(page, "?channel=ch-msg-billing", /Written to the destination/);
  const card = page.getByRole("article").filter({ hasText: WRITTEN }).first();
  await expectCardSaysOnly(card, WRITTEN);
  await expect(card.getByText(RECEIVED)).toHaveCount(0);
  await expect(card.getByText(ACCEPTED)).toHaveCount(0);
  // Nowhere on this page does the delivery claim more.
  expect(await page.getByRole("region", { name: "Deliveries" }).innerText()).not.toMatch(
    /success/i,
  );
});

test("D-08: an ASTM or AU destination is 'Received by the destination. Processing not confirmed.'", async ({
  page,
}) => {
  await openDetail(page, "", /Received by the destination/);
  const card = page.getByRole("article").filter({ hasText: RECEIVED }).first();
  await expectCardSaysOnly(card, RECEIVED);
  await expect(card.getByText(WRITTEN)).toHaveCount(0);
  await expect(card.getByText(ACCEPTED)).toHaveCount(0);
});

test("D-08: an HL7 application answer is 'Accepted by the destination's application.'", async ({
  page,
}) => {
  await openDetail(page, "", /Accepted by the destination/);
  const card = page.getByRole("article").filter({ hasText: ACCEPTED }).first();
  await expectCardSaysOnly(card, ACCEPTED);
  // The attempt it rests on shows the code the destination answered.
  await expect(card.getByRole("row").nth(1)).toContainText("AA");
});

test("D-08: the list and its chips never call a lower level a success", async ({ page }) => {
  await openDetail(page, "");
  await page
    .getByRole("list", { name: "Breadcrumbs" })
    .getByRole("link", { name: "Messages" })
    .click();
  const chips = page.getByRole("grid", { name: "Messages" }).getByRole("listitem");
  await expect(chips.first()).toBeVisible();
  const titles = await chips.evaluateAll((items) =>
    items.map((item) => item.getAttribute("title") ?? ""),
  );
  expect(titles.some((title) => title.includes(`Acknowledged (written). ${WRITTEN}`))).toBe(true);
  expect(titles.some((title) => title.includes(`Acknowledged (received). ${RECEIVED}`))).toBe(true);
  expect(titles.some((title) => title.includes(`Acknowledged (accepted). ${ACCEPTED}`))).toBe(true);
  for (const title of titles.filter((t) => /Acknowledged \((written|received)\)/.test(t))) {
    expect(title).not.toMatch(OVERCLAIM);
  }
});

test("D-08: an uncertain outcome says the message may have been delivered", async ({ page }) => {
  await openDetail(page, "?delivery=timed_out", /: No acknowledgement/);
  const silent = page
    .getByRole("article")
    .filter({ hasText: "No acknowledgement in time, may have been delivered" })
    .first();
  await expect(
    silent.getByText("No acknowledgement in time, may have been delivered"),
  ).toBeVisible();
  await openDetail(page, "?delivery=outcome_unknown", /: Outcome unknown/);
  const lost = page
    .getByRole("article")
    .filter({ hasText: "Outcome unknown, may have been delivered" })
    .first();
  await expect(lost.getByText("Outcome unknown, may have been delivered")).toBeVisible();
  // And a failure is the one state that says it was not.
  await openDetail(page, "?delivery=failed", /: Failed/);
  await expect(page.getByText("Failed, not delivered").first()).toBeVisible();
});

// ── Close with a reason ──────────────────────────────────────────────────────────────────

const closeButton = (card: Locator): Locator =>
  card.getByRole("button", { name: /^Close delivery to / });

/** The card of the first delivery in `state` (its badge text) on the record. */
const cardIn = (page: Page, state: string): Locator =>
  page.getByRole("article").filter({ hasText: state }).first();

async function destinationOf(card: Locator): Promise<string> {
  return ((await card.getByRole("heading", { level: 3 }).textContent()) ?? "").trim();
}

test("D-12: closing needs a reason of 1 to 500 characters and says it does not contact the destination", async ({
  page,
}) => {
  await openDetail(page, "?delivery=failed", /: Failed/);
  const card = cardIn(page, "Failed, not delivered");
  const destination = await destinationOf(card);
  await closeButton(card).click();
  const dialog = page.getByRole("dialog", { name: `Close delivery to ${destination}` });
  await expect(dialog.getByText("Closing does not contact the destination.")).toBeVisible();
  // A failed delivery was provably not delivered, and the dialog says so.
  await expect(dialog).toContainText(`The message is not delivered to ${destination}.`);
  await expect(dialog.getByText("0 of 500 characters")).toBeVisible();
  const reason = dialog.getByRole("textbox", { name: "Reason" });
  const submit = dialog.getByRole("button", { name: "Close delivery", exact: true });
  // No reason: refused, and nothing changed.
  await submit.click();
  await expect(dialog).toBeVisible();
  await expect(reason).toHaveAttribute("aria-invalid", "true");
  // Too long: the counter runs past 500 and the form refuses.
  await reason.fill("y".repeat(501));
  await expect(dialog.getByText("501 of 500 characters")).toBeVisible();
  await submit.click();
  await expect(dialog.getByText("Use 500 characters or fewer.")).toBeVisible();
  await expect(dialog).toBeVisible();
  // The most it takes: 500.
  await reason.fill("z".repeat(500));
  await expect(dialog.getByText("500 of 500 characters")).toBeVisible();
  const written = "Result confirmed by phone with the ward";
  await reason.fill(written);
  await expect(dialog.getByText(`${written.length} of 500 characters`)).toBeVisible();
  await submit.click();
  await expect(dialog).toHaveCount(0);
  // It reads cancelled, with the reason; nothing more can be closed, and nothing needs attention.
  await expect(page.getByText(`Delivery to ${destination} closed`)).toBeVisible();
  await expect(page.getByText("The destination was not contacted.")).toBeVisible();
  const closed = page
    .getByRole("article")
    .filter({ has: page.getByRole("heading", { level: 3, name: destination }) });
  await expect(closed.getByText("Closed", { exact: true }).first()).toBeVisible();
  expect(await factValue(closed, "Closed with reason")).toBe(written);
  await expect(closeButton(closed)).toHaveCount(0);
  await expect(closed.getByRole("button", { name: /^Resend to / })).toBeVisible();
  await expect(notice(page, "Needs attention")).toHaveCount(0);
});

test("D-12: closing an uncertain outcome says the earlier attempt may have reached the destination", async ({
  page,
}) => {
  for (const [query, chip, state] of [
    ["?delivery=timed_out", /: No acknowledgement/, "No acknowledgement in time"],
    ["?delivery=outcome_unknown", /: Outcome unknown/, "Outcome unknown"],
  ] as const) {
    await openDetail(page, query, chip);
    const card = cardIn(page, state);
    const destination = await destinationOf(card);
    await closeButton(card).click();
    const dialog = page.getByRole("dialog", { name: `Close delivery to ${destination}` });
    await expect(dialog.getByText("Closing does not contact the destination.")).toBeVisible();
    await expect(dialog).toContainText(
      `does not contact ${destination}. The earlier attempt may have reached it: `,
    );
    await expect(dialog).toContainText(
      /may have reached it: (no acknowledgement was received|the last acknowledgement code was \w+|it was written|the destination received it)/,
    );
    // It never says the message was not delivered: nobody knows.
    expect(await dialog.innerText()).not.toMatch(/not delivered/i);
    await dialog
      .getByRole("textbox", { name: "Reason" })
      .fill("Checked with the destination's operator");
    await dialog.getByRole("button", { name: "Close delivery", exact: true }).click();
    await expect(dialog).toHaveCount(0);
    const closed = page
      .getByRole("article")
      .filter({ has: page.getByRole("heading", { level: 3, name: destination }) });
    expect(await factValue(closed, "Closed with reason")).toBe(
      "Checked with the destination's operator",
    );
    await expect(closeButton(closed)).toHaveCount(0);
  }
});

test("D-12: a delivery that is sending offers no close; a queued one does", async ({ page }) => {
  await openDetail(page, "?delivery=sending", /: Sending/);
  const sending = cardIn(page, "Sending");
  await expect(sending.getByRole("heading", { level: 3 })).toBeVisible();
  await expect(closeButton(sending)).toHaveCount(0);
  // The attempt in flight is shown as such.
  await expect(sending.getByRole("row").nth(1)).toContainText("In progress");
  await expect(sending.getByRole("row").nth(1)).toContainText("Not yet");
  await openDetail(page, "?delivery=queued", /: Queued/);
  const queued = cardIn(page, "Queued");
  const destination = await destinationOf(queued);
  await closeButton(queued).click();
  const dialog = page.getByRole("dialog", { name: `Close delivery to ${destination}` });
  await dialog.getByRole("textbox", { name: "Reason" }).fill("Order was cancelled by the ward");
  await dialog.getByRole("button", { name: "Close delivery", exact: true }).click();
  await expect(dialog).toHaveCount(0);
  const closed = page
    .getByRole("article")
    .filter({ has: page.getByRole("heading", { level: 3, name: destination }) });
  expect(await factValue(closed, "Closed with reason")).toBe("Order was cancelled by the ward");
});

// ── Resend from the record ───────────────────────────────────────────────────────────────

test("D-12: Resend… on the record works and shows the delivery queued, then acknowledged again", async ({
  page,
}) => {
  await openDetail(page, "?delivery=outcome_unknown", /: Outcome unknown/);
  const card = cardIn(page, "Outcome unknown, may have been delivered");
  const destination = await destinationOf(card);
  const attempts = card.getByRole("table", { name: /^Attempts to / }).getByRole("row");
  const before = await attempts.count();
  await page.getByRole("button", { name: "Resend…", exact: true }).first().click();
  const dialog = page.getByRole("dialog", { name: /^Resend / });
  await expect(dialog.getByRole("checkbox", { name: destination })).toBeChecked();
  // The delivery may have been processed: the duplicate-risk confirmation first.
  await dialog.getByRole("button", { name: "Resend", exact: true }).click();
  await expect(
    dialog.getByText("Confirm the duplicate risk to resend these deliveries."),
  ).toBeVisible();
  await dialog
    .locator("label")
    .filter({ hasText: /I have checked the destination/ })
    .click();
  await dialog
    .getByRole("textbox", { name: /Reason \(optional\)/ })
    .fill("Destination restarted, sending again");
  await dialog.getByRole("button", { name: "Resend", exact: true }).click();
  await expect(dialog.getByText("1 of 1 destination accepted.")).toBeVisible();
  await expect(
    dialog.getByText("Queued again. A new attempt cycle starts with the stored output."),
  ).toBeVisible();
  await dialog.getByRole("button", { name: "Done" }).click();
  await expect(dialog).toHaveCount(0);
  // The record shows the new state at once, then the destination answers.
  const again = page
    .getByRole("article")
    .filter({ has: page.getByRole("heading", { level: 3, name: destination }) });
  await expect(again.getByText("Queued", { exact: true }).first()).toBeVisible();
  await expect(again.getByText("Acknowledged", { exact: true }).first()).toBeVisible({
    timeout: 15_000,
  });
  expect(await factValue(again, "Resent")).toBe("Destination restarted, sending again");
  expect(await factValue(again, "Attempts")).toMatch(/^1 in this cycle, \d+ in all$/);
  await expect(again.getByRole("table", { name: /^Attempts to / }).getByRole("row")).toHaveCount(
    before + 1,
  );
  await expect(notice(page, "Needs attention")).toHaveCount(0);
});

test("D-12: Resend on a delivery card starts with that destination", async ({ page }) => {
  await openDetail(page, "?channel=ch-msg-au680&delivery=acknowledged");
  const both = page.getByRole("article");
  await expect(both).toHaveCount(2);
  const names = await both.getByRole("heading", { level: 3 }).allTextContents();
  const [chosen = "", other = ""] = names;
  await both
    .filter({ has: page.getByRole("heading", { level: 3, name: chosen }) })
    .getByRole("button", { name: `Resend to ${chosen}` })
    .click();
  const dialog = page.getByRole("dialog", { name: /^Resend / });
  await expect(dialog.getByRole("checkbox", { name: chosen, exact: true })).toBeChecked();
  await expect(dialog.getByRole("checkbox", { name: other, exact: true })).not.toBeChecked();
  // Acknowledged deliveries may already have been processed.
  await expect(dialog.getByText("The destination may already have this message")).toBeVisible();
});
