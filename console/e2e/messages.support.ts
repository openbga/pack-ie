import { expect, type Locator, type Page } from "@playwright/test";
import { choose, dataRows, loaded, type MockSwitches, open } from "./support";

const GRID = "Messages";

/** The grid of the Messages list. */
export const messageGrid = (page: Page): Locator => page.getByRole("grid", { name: GRID });

/** The data rows of the Messages list (without the header row). */
export const messageRows = (page: Page): Locator => dataRows(page, GRID);

/** The paging bar below the grid. */
export const paging = (page: Page): Locator =>
  page.getByRole("navigation", { name: "Message pages" });

/** The paging bar's count line: "Rows 1–100 of 13,734", with " · Updating…" while it loads. */
export const rangeStatus = (page: Page): Locator => paging(page).getByRole("status");

const COUNT = /^(Rows|No rows)/;

/** Two frames, so a transition an action started has rendered (and marked the grid busy). */
async function flush(page: Page): Promise<void> {
  await page.evaluate(
    () =>
      new Promise<void>((done) => requestAnimationFrame(() => requestAnimationFrame(() => done()))),
  );
}

/** Lets React commit what an action asked for, then waits until the list is no longer loading. */
export async function settled(page: Page): Promise<void> {
  await flush(page);
  await expect(rangeStatus(page)).toHaveText(COUNT);
  await expect(rangeStatus(page)).not.toContainText("Updating");
  await expect(messageGrid(page)).not.toHaveAttribute("aria-busy", "true");
}

/** Opens Messages (with `query` such as `?delivery=timed_out`) and waits for its first page. */
export async function openMessages(
  page: Page,
  query = "",
  switches: MockSwitches = {},
): Promise<void> {
  await open(page, `/messages${query}`, switches);
  await loaded(page, "Messages");
  await expect(messageRows(page).first()).toBeVisible();
  await settled(page);
}

/** The number a count line ends with ("Rows 1–100 of 13,734"), or the number in `text`. */
function numberIn(text: string, after = /of /): number {
  const match = new RegExp(`${after.source}([\\d,]+)`).exec(text);
  if (!match?.[1]) throw new Error(`no number in "${text}"`);
  return Number(match[1].replaceAll(",", ""));
}

/** How many messages the list says match. */
export async function totalCount(page: Page): Promise<number> {
  return numberIn((await rangeStatus(page).textContent()) ?? "");
}

/** The rows of the grid, as the headers name them: one text per cell, header text first. */
export async function columnTexts(page: Page, header: string): Promise<string[]> {
  return messageGrid(page).evaluate((grid, name) => {
    const heads = [...grid.querySelectorAll('[role="columnheader"]')].map(
      (head) => head.textContent?.trim() ?? "",
    );
    const column = heads.indexOf(name);
    if (column < 0) return [];
    return [...grid.querySelectorAll('[role="row"]')]
      .filter((row) => row.querySelector('[role="gridcell"]') !== null)
      .map((row) => row.querySelectorAll('[role="gridcell"]')[column]?.textContent?.trim() ?? "");
  }, header);
}

/** The names of the grid's column headers, without the invisible "Actions" one. */
export async function columnHeaders(page: Page): Promise<string[]> {
  const names = await messageGrid(page).getByRole("columnheader").allTextContents();
  return names.map((name) => name.trim()).filter((name) => name !== "Actions");
}

/** The delivery chips of every row, one list of `title`s ("Destination: State. Meaning.") per row. */
export async function chipTitles(page: Page): Promise<string[][]> {
  return messageGrid(page).evaluate((grid) =>
    [...grid.querySelectorAll('[role="row"]')]
      .filter((row) => row.querySelector('[role="gridcell"]') !== null)
      .map((row) =>
        [...row.querySelectorAll('ul[aria-label="Deliveries"] > li')].map(
          (chip) => chip.getAttribute("title") ?? "",
        ),
      ),
  );
}

/** The `datetime` of every row's Received cell, in grid order. */
export async function receivedTimes(page: Page): Promise<number[]> {
  const times = await messageGrid(page)
    .locator("time")
    .evaluateAll((items) => items.map((item) => item.getAttribute("datetime") ?? ""));
  return times.map((time) => Date.parse(time));
}

/**
 * Waits until the grid shows rows and every row satisfies `accepts` for its cell under `header`.
 * Polls, so it also waits out the previous answer the grid keeps while the next one loads.
 */
export async function expectEveryRow(
  page: Page,
  header: string,
  accepts: (text: string) => boolean,
  message: string,
): Promise<void> {
  await expect
    .poll(
      async () => {
        const texts = await columnTexts(page, header);
        return texts.length > 0 && texts.every(accepts);
      },
      { message },
    )
    .toBe(true);
  await settled(page);
}

/** Opens "More filters" if it is closed. */
export async function openMoreFilters(page: Page): Promise<void> {
  const toggle = page.getByRole("button", { name: "More filters" });
  if ((await toggle.getAttribute("aria-expanded")) !== "true") await toggle.click();
  await expect(page.getByRole("group", { name: "More filters" })).toBeVisible();
}

/** Picks an option of a select inside "More filters". */
export async function chooseMore(page: Page, label: string, option: string): Promise<void> {
  await openMoreFilters(page);
  await choose(page, label, option);
}

/**
 * Types into a find box of the toolbar and presses Enter, which applies it at once instead of
 * after the pause in typing: two boxes typed into within that pause would overwrite each other.
 */
export async function find(page: Page, label: string, text: string): Promise<void> {
  const box = page.getByRole("searchbox", { name: label });
  await box.fill(text);
  await box.press("Enter");
  await flush(page);
}

/** Ticks or unticks a checkbox of a popover or dialog by its label (the mark covers the input). */
export async function tick(scope: Locator, name: string, on = true): Promise<void> {
  const box = scope.getByRole("checkbox", { name, exact: true });
  if ((await box.isChecked()) !== on) await box.locator("xpath=ancestor::label[1]").click();
  await expect(box).toBeChecked({ checked: on });
}

/** A quick filter's button. */
export const quick = (page: Page, label: string): Locator =>
  page
    .getByRole("group", { name: "Quick filters" })
    .getByRole("button", { name: new RegExp(`^${label}`) });

/** The count a quick filter shows (its name is "Needs attention 669"). */
export async function quickCount(page: Page, label: string): Promise<number> {
  await expect(quick(page, label)).toHaveText(/\d/);
  return numberIn((await quick(page, label).textContent()) ?? "", /\s/);
}

/** Opens the row menu of `row` and chooses "Resend…". */
export async function openResend(page: Page, row: Locator): Promise<Locator> {
  await row.getByRole("button", { name: /^Actions for / }).click();
  await page.getByRole("menuitem", { name: "Resend…" }).click();
  const dialog = page.getByRole("dialog", { name: /^Resend / });
  await expect(dialog).toBeVisible();
  return dialog;
}

/** The first row whose delivery chips include one titled like `chip`. */
export function rowWithChip(page: Page, chip: RegExp): Locator {
  return messageRows(page)
    .filter({ has: page.getByRole("listitem", { name: chip }) })
    .first();
}

/** The label under which a row is announced: "Actions for <identifier>" without the prefix. */
export async function rowIdentifier(row: Locator): Promise<string> {
  const label = await row.getByRole("button", { name: /^Actions for / }).getAttribute("aria-label");
  return (label ?? "").replace(/^Actions for /, "");
}

/**
 * Opens the list with `query`, then the first row (or the first with a delivery chip titled
 * like `chip`), and waits for the record. Returns the record's heading: the message's identifier.
 */
export async function openDetail(
  page: Page,
  query: string,
  chip?: RegExp,
  beforeOpen?: (page: Page) => Promise<void>,
): Promise<string> {
  await openMessages(page, query);
  if (beforeOpen) await beforeOpen(page);
  const row = chip ? rowWithChip(page, chip) : messageRows(page).first();
  await row.getByRole("gridcell").nth(1).click();
  await expect(page).toHaveURL(/#\/messages\/msg-/);
  await expect(page.getByRole("button", { name: "Show original message" })).toBeVisible();
  return (await page.getByRole("heading", { level: 1 }).textContent()) ?? "";
}

/** The delivery card (article) of a destination on the record. */
export const deliveryCard = (page: Page, destination: string | RegExp): Locator =>
  page
    .getByRole("article")
    .filter({ has: page.getByRole("heading", { level: 3, name: destination }) });

/** The value shown beside a fact's name in `scope` (a `dl` of terms and definitions). */
export async function factValue(scope: Locator, name: string): Promise<string> {
  const term = scope
    .locator("dt")
    .filter({ hasText: new RegExp(`^${name}$`) })
    .first();
  return ((await term.locator("xpath=following-sibling::dd[1]").textContent()) ?? "").trim();
}

/** The sentences that say what an acknowledgement proved (operator-console.md section 6). */
export const WRITTEN = "Written to the destination. Not confirmed by it.";
export const RECEIVED = "Received by the destination. Processing not confirmed.";
export const ACCEPTED = "Accepted by the destination's application.";

/** A notice (alert or status) of the record whose text includes `title`. */
export const notice = (page: Page, title: string): Locator =>
  page.locator("main").locator('[role="alert"], [role="status"]').filter({ hasText: title });

/**
 * Messages of the default mock seed that a newer revision supersedes, whatever the size of the
 * history (src/api/mock/messageRevisions.ts; MockEngineApi.revisions.test.ts pins them): `old`
 * is superseded by `newer`. Their ids are the mock's (`msg-<n>`), and the message ID filter
 * finds one by the last hex digits of its stored id (`SUPERSEDED.prefix`).
 */
export const SUPERSEDED = {
  old: "msg-40",
  newer: "msg-15",
  /** The message ID filter value that finds `old` alone (msg-40 is 0x28). */
  prefix: "00000000-0000-4000-8000-000000000028",
} as const;
