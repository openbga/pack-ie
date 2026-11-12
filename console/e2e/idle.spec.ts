import { expect, type Locator, type Page, test } from "@playwright/test";
import { countText, loadedCount, openAudit, rowOfAction } from "./audit.support";
import { deliveryCard, messageRows, openDetail, openMessages } from "./messages.support";
import { eventPanel, heading, loaded, mockHandle, open } from "./support";

// Idle protection (compliance P11): after the idle time the console clears the message content
// it holds, leaves the message detail, closes audit details and covers everything until
// Continue. The setting belongs to the console (this computer), not the engine; there is no
// login.

const IDLE = "15:05";
const COVER = "Content hidden after 15 minutes without activity. Choose Continue to carry on.";

const overlay = (page: Page): Locator => page.getByRole("alertdialog", { name: "Content hidden" });
const original = (page: Page): Locator =>
  page.getByRole("region", { name: "Original message", exact: true });

/** Opens the original message, as text and then as parsed fields; returns a line of each. */
async function openOriginalWithFields(page: Page): Promise<{ text: string; value: string }> {
  const viewer = original(page);
  await viewer.getByRole("button", { name: "Show original message" }).click();
  await expect(viewer.locator("pre")).toBeVisible();
  const text = ((await viewer.locator("pre").innerText()).split("\n")[0] ?? "").slice(0, 24);
  expect(text.length).toBeGreaterThan(5);
  await viewer.getByRole("radio", { name: "Fields" }).click();
  await viewer.getByRole("button", { name: "Show content" }).click();
  await expect(viewer.getByRole("table").getByRole("cell").first()).toBeVisible();
  // The longest value: the one least likely to appear on another screen by chance.
  const cells = await viewer.getByRole("table").getByRole("cell").allInnerTexts();
  const value = cells.map((cell) => cell.trim()).sort((a, b) => b.length - a.length)[0] ?? "";
  expect(value.length).toBeGreaterThan(5);
  return { text, value };
}

/** How many body views the audit log holds, read on the Audit page and left again for `back`. */
async function bodyViews(page: Page, back: string): Promise<number> {
  await page.evaluate((hash) => {
    window.location.hash = hash;
  }, "/audit?action=message_body_view");
  await loaded(page, "Audit");
  await expect(
    page.getByRole("grid", { name: "Audit events" }).or(page.getByText(/No audit events/)),
  ).toBeVisible();
  const count = (await countText(page).count()) === 0 ? 0 : await loadedCount(page);
  await page.evaluate((hash) => {
    window.location.hash = hash;
  }, back);
  return count;
}

/** The path of the page, from the address. */
const route = (page: Page): string => new URL(page.url()).hash.slice(1);

test("IP-01 Settings > Privacy sets the idle time, checks it and keeps it", async ({ page }) => {
  await open(page, "/settings/privacy");
  await expect(heading(page, "Settings")).toBeVisible();
  const minutes = page.getByRole("textbox", { name: /^Hide content after/ });
  await expect(minutes).toHaveValue("15");
  await expect(page.getByText(/kept by this console on this computer/)).toBeVisible();
  const save = page.getByRole("button", { name: "Save" });
  await expect(save).toBeDisabled();
  await minutes.fill("0");
  await expect(page.getByText("Enter a whole number from 1 to 240.")).toBeVisible();
  await expect(save).toBeDisabled();
  await minutes.fill("5");
  await save.click();
  await expect(page.getByText("Idle protection saved")).toBeVisible();
  await expect(page.getByText("Content is hidden after 5 minutes without activity.")).toBeVisible();
  // It is the console's own setting, kept on this computer.
  await page.reload();
  await expect(page.getByRole("textbox", { name: /^Hide content after/ })).toHaveValue("5");
});

test("IP-02 after the idle time the detail is left, the console is covered and no content remains", async ({
  page,
}) => {
  await page.clock.install();
  await openDetail(page, "?delivery=outcome_unknown", /: Outcome unknown/);
  const { text, value } = await openOriginalWithFields(page);

  // Fourteen minutes of nothing is not enough.
  await page.clock.fastForward("14:00");
  await expect(overlay(page)).toHaveCount(0);
  await expect(original(page).locator("pre, table")).toHaveCount(1);

  // Past fifteen the console is covered and the page is the Messages list, not the record.
  await page.clock.fastForward("01:30");
  await expect(overlay(page)).toBeVisible();
  await expect(overlay(page)).toContainText(COVER);
  await expect.poll(() => route(page)).toMatch(/^\/messages(\?.*)?$/);
  await expect(page.locator("main h1")).toHaveText("Messages");
  // The cover is modal: the app beneath is inert.
  await expect(page.locator("#root")).toHaveJSProperty("inert", true);

  // Not the body, not the parsed fields, and nothing of the record's own page.
  await expect(page.locator("pre")).toHaveCount(0);
  await expect(page.locator("table")).toHaveCount(0);
  const content = await page.locator("body").innerText();
  expect(content).not.toContain(text);
  expect(content).not.toContain(value);
  for (const detail of ["Show original message", "Source endpoint", "Deliveries", "Resend…"]) {
    expect(content).not.toContain(detail);
  }
  // Back does not return to the record: the list replaced it.
  await overlay(page).getByRole("button", { name: "Continue" }).click();
  await page.goBack();
  await expect(page).not.toHaveURL(/#\/messages\/msg-/);
});

test("IP-04 Continue uncovers the console, and a body is read again, and audited again, on request", async ({
  page,
}) => {
  await page.clock.install();
  await openDetail(page, "?delivery=outcome_unknown", /: Outcome unknown/);
  const record = route(page);
  const identifier = await page.locator("main h1").innerText();
  const before = await bodyViews(page, record);
  await expect(page.getByRole("button", { name: "Show original message" })).toBeVisible();

  await original(page).getByRole("button", { name: "Show original message" }).click();
  await expect(original(page).locator("pre")).toBeVisible();
  expect(await mockHandle(page, (mock) => mock.contentReads().body)).toBe(1);

  await page.clock.fastForward(IDLE);
  await expect(overlay(page)).toBeVisible();
  await overlay(page).getByRole("button", { name: "Continue" }).click();
  await expect(overlay(page)).toHaveCount(0);
  await expect(page.locator("#root")).toHaveJSProperty("inert", false);

  // The record again shows facts only: the body is not shown until it is asked for.
  await page.evaluate((hash) => {
    window.location.hash = hash;
  }, record);
  await expect(page.locator("main h1")).toHaveText(identifier);
  await expect(page.getByRole("button", { name: "Show original message" })).toBeVisible();
  await expect(page.locator("pre")).toHaveCount(0);
  expect(await mockHandle(page, (mock) => mock.contentReads().body)).toBe(1);

  // Asking for it reaches the engine again, and the engine records that view.
  await original(page).getByRole("button", { name: "Show original message" }).click();
  await expect(original(page).locator("pre")).toBeVisible();
  expect(await mockHandle(page, (mock) => mock.contentReads().body)).toBe(2);
  expect(await bodyViews(page, record)).toBe(before + 2);
});

test("IP-05 one body's reveal never reveals another", async ({ page }) => {
  await page.clock.install();
  await openDetail(page, "?delivery=outcome_unknown", /: Outcome unknown/);
  const record = route(page);
  const card = deliveryCard(page, "LIS HL7 out");
  const before = await bodyViews(page, record);
  await original(page).getByRole("button", { name: "Show original message" }).click();
  await expect(original(page).locator("pre")).toBeVisible();
  await card.getByRole("button", { name: /^Show transformed message to / }).click();
  await expect(card.locator("pre")).toBeVisible();

  await page.clock.fastForward(IDLE);
  await overlay(page).getByRole("button", { name: "Continue" }).click();
  await page.evaluate((hash) => {
    window.location.hash = hash;
  }, record);

  // Open only the original: the transformed message stays closed and is not requested.
  await original(page).getByRole("button", { name: "Show original message" }).click();
  await expect(original(page).locator("pre")).toBeVisible();
  await expect(card.locator("pre")).toHaveCount(0);
  await expect(card.getByRole("button", { name: /^Show transformed message to / })).toBeVisible();
  expect(await mockHandle(page, (mock) => mock.contentReads().body)).toBe(3);
  expect(await bodyViews(page, record)).toBe(before + 3);
});

test("IP-06 a body still being read when the idle time passes does not appear after Continue", async ({
  page,
}) => {
  await page.clock.install();
  await openMessages(page, "?delivery=outcome_unknown", { scenario: "slow" });
  await messageRows(page).first().getByRole("gridcell").nth(1).click();
  await expect(page).toHaveURL(/#\/messages\/msg-/);
  const record = route(page);
  await original(page).getByRole("button", { name: "Show original message" }).click();
  await expect(page.getByText("Loading original message…")).toBeVisible();

  await page.clock.fastForward(IDLE);
  await expect(overlay(page)).toBeVisible();
  await overlay(page).getByRole("button", { name: "Continue" }).click();
  await page.evaluate((hash) => {
    window.location.hash = hash;
  }, record);
  await expect(page.getByRole("button", { name: "Show original message" })).toBeVisible();
  // Long enough for the answer that was in flight to have arrived.
  await page.clock.fastForward("00:05");
  await page.waitForTimeout(2000);
  await expect(page.locator("pre")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Show original message" })).toBeVisible();
  expect(await mockHandle(page, (mock) => mock.contentReads().body)).toBe(1);
});

test("IP-07 an open audit event is closed, with its details, when the console goes idle", async ({
  page,
}) => {
  await page.clock.install();
  await openAudit(page, "?action=config_apply");
  await rowOfAction(page, "Configuration applied").click();
  const panel = eventPanel(page);
  await expect(panel.getByText("Note", { exact: true })).toBeVisible();
  expect(route(page)).toContain("seq=");

  await page.clock.fastForward(IDLE);
  await expect(overlay(page)).toBeVisible();
  expect(route(page)).not.toContain("seq=");
  const content = await page.locator("body").innerText();
  for (const detail of ["Previous hash", "Details", "Note"]) expect(content).not.toContain(detail);
  await overlay(page).getByRole("button", { name: "Continue" }).click();
  await expect(panel).toHaveCount(0);
  await expect(page.getByRole("grid", { name: "Audit events" })).toBeVisible();
});

test("IP-08 the cover holds focus on Continue, Tab stays inside it and Enter dismisses it", async ({
  page,
}) => {
  await page.clock.install();
  await openDetail(page, "?delivery=outcome_unknown", /: Outcome unknown/);
  await original(page).getByRole("button", { name: "Show original message" }).click();
  await expect(original(page).locator("pre")).toBeVisible();

  await page.clock.fastForward(IDLE);
  const carryOn = overlay(page).getByRole("button", { name: "Continue" });
  await expect(carryOn).toBeFocused();
  for (const key of ["Tab", "Tab", "Shift+Tab", "Escape"]) {
    await page.keyboard.press(key);
    await expect(overlay(page)).toBeVisible();
    await expect(carryOn).toBeFocused();
  }
  await page.keyboard.press("Enter");
  await expect(overlay(page)).toHaveCount(0);
  // The control that had focus left with the record: focus goes to the page's heading.
  await expect(page.locator("main h1")).toBeFocused();
});

test("IP-09 a dialog open when the idle time passes is covered, and is still there after Continue", async ({
  page,
}) => {
  await page.clock.install();
  await openAudit(page);
  await page.getByRole("button", { name: "Export…" }).click();
  const dialog = page.getByRole("dialog", { name: "Export audit events" });
  await expect(dialog).toBeVisible();

  await page.clock.fastForward(IDLE);
  await expect(overlay(page)).toBeVisible();
  // Only the cover is reachable: the dialog is inert beneath it.
  expect(await dialog.evaluate((element) => element.closest("[inert]") !== null)).toBe(true);
  await expect(overlay(page).getByRole("button", { name: "Continue" })).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(overlay(page).getByRole("button", { name: "Continue" })).toBeFocused();
  await overlay(page).getByRole("button", { name: "Continue" }).click();

  await expect(overlay(page)).toHaveCount(0);
  expect(await dialog.evaluate((element) => element.closest("[inert]") !== null)).toBe(false);
  await dialog.getByRole("button", { name: "Cancel" }).click();
  await expect(dialog).toHaveCount(0);
});

test("IP-03 activity counts as presence, so a busy operator is never interrupted", async ({
  page,
}) => {
  await page.clock.install();
  await openMessages(page);
  await open(page, "/messages");
  await page.clock.fastForward("10:00");
  await page.mouse.move(100, 100);
  await page.clock.fastForward("10:00");
  await expect(overlay(page)).toHaveCount(0);
  await expect(page.locator("#root")).toHaveJSProperty("inert", false);
});
