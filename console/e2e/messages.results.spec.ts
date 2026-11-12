import { expect, test } from "@playwright/test";
import {
  chooseMore,
  columnTexts,
  expectEveryRow,
  find,
  messageGrid,
  messageRows,
  openMessages,
  paging,
  rangeStatus,
  receivedTimes,
  settled,
  totalCount,
} from "./messages.support";
import { choose, loaded, open } from "./support";

test("H-05: identifier and text stay out of the address and survive detail and Back", async ({
  page,
}) => {
  await openMessages(page);
  const identifiers = await columnTexts(page, "Identifier");
  const sample = identifiers.find((text) => /^MC\d{8}$/.test(text)) ?? "";
  expect(sample, "an HL7 control id on the first page").not.toBe("");
  await find(page, "Identifier", sample);
  await chooseMore(page, "Identifier match", "Exactly");
  const words = sample.slice(0, 5);
  await find(page, "Message text", words);
  await expectEveryRow(page, "Identifier", (text) => text === sample, "identifier narrowed");
  const narrowed = await totalCount(page);
  // Patient data does not belong in an address: neither the values nor a parameter for them.
  const url = decodeURIComponent(page.url());
  expect(url).not.toContain(sample);
  expect(url).not.toContain(words);
  expect(url).not.toMatch(/[?&](id|identifier|text)=/);
  // Open a message and come back: the criteria are as they were, from memory.
  await messageRows(page).first().click();
  await expect(page.getByRole("heading", { level: 1, name: sample })).toBeVisible();
  await page.goBack();
  await expect(page.getByRole("heading", { level: 1, name: "Messages" })).toBeVisible();
  await expect(page.getByRole("searchbox", { name: "Identifier" })).toHaveValue(sample);
  await expect(page.getByRole("searchbox", { name: "Message text" })).toHaveValue(words);
  await expectEveryRow(page, "Identifier", (text) => text === sample, "after Back");
  expect(await totalCount(page)).toBe(narrowed);
  // The breadcrumb leads back the same way.
  await messageRows(page).first().click();
  await page
    .getByRole("list", { name: "Breadcrumbs" })
    .getByRole("link", { name: "Messages" })
    .click();
  await expect(page.getByRole("searchbox", { name: "Identifier" })).toHaveValue(sample);
  // Reloading the console forgets them.
  await page.reload();
  await expect(page.getByRole("heading", { level: 1, name: "Messages" })).toBeVisible();
  await expect(page.getByRole("searchbox", { name: "Identifier" })).toHaveValue("");
  await expect(page.getByRole("searchbox", { name: "Message text" })).toHaveValue("");
});

test("H-05: no messages yet, no match and nothing in the range are told apart", async ({
  page,
}) => {
  // A fresh install: the store holds nothing.
  await open(page, "/messages", { scenario: "empty" });
  await loaded(page, "Messages");
  await expect(page.getByText("No messages yet")).toBeVisible();
  await expect(
    page.getByText("Messages appear here as the message channels receive them."),
  ).toBeVisible();
  await expect(messageGrid(page)).toHaveCount(0);
  await expect(page.getByText("No messages match")).toHaveCount(0);
  await expect(page.getByText("No messages in this time range")).toHaveCount(0);
  // A criterion with nothing behind it is a narrowing, so it says "match" even then.
  await find(page, "Message text", "anything");
  await expect(page.getByText("No messages match")).toBeVisible();
});

test("H-05: a filter that matches nothing offers Clear filters", async ({ page }) => {
  await openMessages(page);
  await find(page, "Message text", "no-such-text-anywhere");
  await expect(page.getByText("No messages match")).toBeVisible();
  await expect(
    page.getByText("No stored message in the selected time range matches these filters."),
  ).toBeVisible();
  await expect(page.getByText("No messages yet")).toHaveCount(0);
  await expect(messageGrid(page)).toHaveCount(0);
  // The button of the empty state clears them and the list comes back.
  await page.getByRole("button", { name: "Clear filters" }).last().click();
  await expect(messageRows(page).first()).toBeVisible();
  await expect(page.getByRole("searchbox", { name: "Message text" })).toHaveValue("");
});

test("H-05: messages that exist outside the time range offer Show any time", async ({ page }) => {
  await open(page, "/messages?from=2020-01-01T00:00:00Z&to=2020-01-02T00:00:00Z");
  await loaded(page, "Messages");
  await expect(page.getByText("No messages in this time range")).toBeVisible();
  await expect(
    page.getByText("Messages are stored, but none was received in the selected time range."),
  ).toBeVisible();
  await page.getByRole("button", { name: "Show any time" }).click();
  await expect(messageRows(page).first()).toBeVisible();
  await expect(page.getByRole("button", { name: /Time range$/ })).toContainText("Any time");
  await expect(page).toHaveURL(/range=any/);
});

test("H-05: paging with Next, Previous, First, the last page and rows per page", async ({
  page,
}) => {
  await openMessages(page);
  const first = await receivedTimes(page);
  const next = paging(page).getByRole("button", { name: "Next" });
  const previous = paging(page).getByRole("button", { name: "Previous" });
  const firstPage = paging(page).getByRole("button", { name: "First" });
  await expect(firstPage).toBeDisabled();
  await expect(previous).toBeDisabled();
  await next.click();
  await expect(rangeStatus(page)).toHaveText(/^Rows 101–200 of [\d,]+$/);
  await settled(page);
  expect(page.url()).toContain("page=2");
  const second = await receivedTimes(page);
  expect(Math.max(...second)).toBeLessThan(Math.min(...first));
  await expect(firstPage).toBeEnabled();
  await next.click();
  await expect(rangeStatus(page)).toHaveText(/^Rows 201–300 of /);
  await previous.click();
  await expect(rangeStatus(page)).toHaveText(/^Rows 101–200 of /);
  await settled(page);
  expect(await receivedTimes(page)).toEqual(second);
  await firstPage.click();
  await expect(rangeStatus(page)).toHaveText(/^Rows 1–100 of /);
  await settled(page);
  expect(await receivedTimes(page)).toEqual(first);
  expect(page.url()).not.toContain("page=");
  // Rows per page: 50, then 200 (which the grid draws as a window, not all at once).
  await choose(page, "Rows per page", "50");
  await expect(rangeStatus(page)).toHaveText(/^Rows 1–50 of /);
  await expect(messageRows(page)).toHaveCount(50);
  await choose(page, "Rows per page", "200");
  await expect(rangeStatus(page)).toHaveText(/^Rows 1–200 of /);
  await settled(page);
  const drawn = await messageRows(page).count();
  expect(drawn).toBeGreaterThan(10);
  expect(drawn).toBeLessThan(200);
  // The choice is the console's, not the address's: it stays through a visit to a message.
  expect(page.url()).not.toMatch(/size|limit|rows/i);
  await messageRows(page).first().click();
  await expect(page.getByRole("heading", { level: 1 })).not.toHaveText("Messages");
  await page.goBack();
  await expect(rangeStatus(page)).toHaveText(/^Rows 1–200 of /);
});

test("H-05: the last page is short, and a page past the end says so", async ({ page }) => {
  await openMessages(page, "?range=any");
  const total = await totalCount(page);
  const last = Math.ceil(total / 100);
  const onLast = total - (last - 1) * 100;
  await page.goto(page.url().replace(/#.*/, `#/messages?range=any&page=${last}`));
  await expect(rangeStatus(page)).toHaveText(
    new RegExp(
      `^Rows ${((last - 1) * 100 + 1).toLocaleString("en-US")}–${total.toLocaleString("en-US")} of`,
    ),
  );
  await expect(messageRows(page)).toHaveCount(onLast);
  await expect(paging(page).getByRole("button", { name: "Next" })).toBeDisabled();
  await page.goto(page.url().replace(/#.*/, `#/messages?range=any&page=${last + 50}`));
  await expect(page.getByText("This page is past the last message")).toBeVisible();
  await page.getByRole("button", { name: "Go to the first page" }).click();
  await expect(rangeStatus(page)).toHaveText(/^Rows 1–100 of /);
});

test("H-05: a column header sorts, and sorting returns to the first page", async ({ page }) => {
  await openMessages(page);
  const header = (name: string) => messageGrid(page).getByRole("columnheader", { name });
  const states = () => columnTexts(page, "Message state");
  const ascending = (texts: string[]) =>
    texts.every((t, i) => i === 0 || (texts[i - 1] ?? "") <= t);
  const descending = (texts: string[]) =>
    texts.every((t, i) => i === 0 || (texts[i - 1] ?? "") >= t);
  // Start from page 2 to see that a new sort goes back to page 1.
  await paging(page).getByRole("button", { name: "Next" }).click();
  await expect(rangeStatus(page)).toHaveText(/^Rows 101–200 of /);
  await header("Message state").click();
  await expect(header("Message state")).toHaveAttribute("aria-sort", "ascending");
  await expect(rangeStatus(page)).toHaveText(/^Rows 1–100 of /);
  await expect.poll(async () => ascending(await states())).toBe(true);
  expect(new Set(await states()).size).toBeLessThanOrEqual(2);
  expect(page.url()).toContain("sort=state");
  expect(page.url()).not.toMatch(/desc=1|page=/);
  await header("Message state").click();
  await expect(header("Message state")).toHaveAttribute("aria-sort", "descending");
  await expect.poll(async () => descending(await states())).toBe(true);
  expect(page.url()).toMatch(/sort=state/);
  expect(page.url()).toContain("desc=1");
  // A third click goes back to the default order.
  await header("Message state").click();
  await expect(header("Received")).toHaveAttribute("aria-sort", "descending");
  await expect(page).not.toHaveURL(/sort=/);
  // Channel sorts by name; Received turns around to oldest first.
  await header("Channel").click();
  await expect(header("Channel")).toHaveAttribute("aria-sort", "ascending");
  await expect.poll(async () => ascending(await columnTexts(page, "Channel"))).toBe(true);
  expect(new Set(await columnTexts(page, "Channel")).size).toBeLessThanOrEqual(3);
  await header("Received").click();
  await expect(header("Received")).toHaveAttribute("aria-sort", /ascending|descending/);
  await header("Received").click();
  await expect(header("Received")).toHaveAttribute("aria-sort", "ascending");
  await expect
    .poll(async () => {
      const times = await receivedTimes(page);
      return times.every((t, i) => i === 0 || (times[i - 1] ?? 0) <= t);
    })
    .toBe(true);
});

test("H-05: a row opens its message by click and by Enter", async ({ page }) => {
  await openMessages(page);
  const rows = messageRows(page);
  const click = rows.nth(2);
  const expected = await click.getByRole("gridcell").nth(6).textContent();
  await click.getByRole("gridcell").nth(1).click();
  await expect(page).toHaveURL(/#\/messages\/msg-\d+/);
  await expect(page.getByRole("heading", { level: 1, name: expected ?? "" })).toBeVisible();
  await expect(page.getByRole("navigation", { name: "Breadcrumb" })).toContainText("Messages");
  await page.goBack();
  await expect(rows.first()).toBeVisible();
  // The keyboard: focus a cell of the row, press Enter.
  const keyed = rows.nth(3);
  const keyedId = await keyed.getByRole("gridcell").nth(6).textContent();
  await keyed.getByRole("gridcell").nth(1).focus();
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/#\/messages\/msg-\d+/);
  await expect(page.getByRole("heading", { level: 1, name: keyedId ?? "" })).toBeVisible();
  // Nothing of the message is in the address or the window title but its id and identifier.
  expect(await page.title()).toBe(`${keyedId} · PACK-IE Console`);
});
