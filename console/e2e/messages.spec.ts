import { expect, type Page, test } from "@playwright/test";
import {
  chipTitles,
  chooseMore,
  columnHeaders,
  columnTexts,
  expectEveryRow,
  find,
  messageGrid,
  messageRows,
  openMessages,
  openMoreFilters,
  quick,
  quickCount,
  rangeStatus,
  receivedTimes,
  settled,
  tick,
  totalCount,
} from "./messages.support";
import { choose } from "./support";

const DEFAULT_HEADERS = [
  "Received",
  "Channel",
  "Direction",
  "Message state",
  "Delivery summary",
  "Message type",
  "Identifier",
];

const clearFilters = (page: Page) => page.getByRole("button", { name: "Clear filters" }).first();

test("H-05: default columns that fit 1280 px, newest first, last 24 hours", async ({ page }) => {
  await openMessages(page);
  // The default set drops Patient ID and Size at this width (columnFit); the button says so.
  expect(await columnHeaders(page)).toEqual(DEFAULT_HEADERS);
  await expect(
    page.getByRole("button", { name: "Columns: Default (2 hidden to fit)" }),
  ).toBeVisible();
  // Newest first: the Received header is sorted descending and the rows really are.
  await expect(messageGrid(page).getByRole("columnheader", { name: "Received" })).toHaveAttribute(
    "aria-sort",
    "descending",
  );
  const times = await receivedTimes(page);
  expect(times.length).toBe(100);
  expect(times).toEqual([...times].sort((a, b) => b - a));
  // The default range, and a plain address.
  await expect(
    page.getByRole("button", { name: /Last 24 hours.*Time range|Time range/ }),
  ).toContainText("Last 24 hours");
  expect(page.url()).not.toMatch(/from=|range=|sort=|page=/);
  await expect(rangeStatus(page)).toHaveText(/^Rows 1–100 of [\d,]+$/);
  // Everything shown was received within the last 24 hours.
  expect(Date.now() - (times.at(-1) ?? 0)).toBeLessThan(24 * 3_600_000);
  await expect(clearFilters(page)).toHaveCount(0);
});

test("H-05: the Columns chooser brings back the hidden columns and adds optional ones", async ({
  page,
}) => {
  await openMessages(page);
  await page.getByRole("button", { name: /^Columns:/ }).click();
  const chooser = page.getByRole("dialog", { name: "Columns" });
  await tick(chooser, "Patient ID");
  await tick(chooser, "Size");
  await tick(chooser, "Message ID");
  await tick(chooser, "Source peer");
  await tick(chooser, "Configuration version");
  await tick(chooser, "Resent from");
  await expect(page.getByRole("button", { name: "Columns: Custom" })).toBeVisible();
  expect(await columnHeaders(page)).toEqual([
    ...DEFAULT_HEADERS.slice(0, 7),
    "Patient ID",
    "Size",
    "Message ID",
    "Source peer",
    "Configuration version",
    "Resent from",
  ]);
  // The new columns carry data: patient ids, sizes and the `msg-<n>` ids.
  expect((await columnTexts(page, "Patient ID")).some((text) => /^PT-\d{6}$/.test(text))).toBe(
    true,
  );
  expect((await columnTexts(page, "Size")).some((text) => /\d+ B$/.test(text))).toBe(true);
  expect((await columnTexts(page, "Message ID")).every((text) => /^msg-\d+$/.test(text))).toBe(
    true,
  );
  // A column can also be taken away again, and "Default columns" returns to the set that fits.
  await tick(chooser, "Direction", false);
  expect(await columnHeaders(page)).not.toContain("Direction");
  await chooser.getByRole("button", { name: "Default columns" }).click();
  expect(await columnHeaders(page)).toEqual(DEFAULT_HEADERS);
  await expect(
    page.getByRole("button", { name: "Columns: Default (2 hidden to fit)" }),
  ).toBeVisible();
});

test("H-05: the time range presets narrow the list and its count", async ({ page }) => {
  await openMessages(page);
  const totals: Record<string, number> = { "Last 24 hours": await totalCount(page) };
  const ranges: [string, string][] = [
    ["Last hour", "from=-1h"],
    ["Last 7 days", "from=-7d"],
    ["Last 30 days", "from=-30d"],
    ["Any time", "range=any"],
  ];
  for (const [label, param] of ranges) {
    await choose(page, "Time range", label);
    await expect(page).toHaveURL(new RegExp(param));
    await settled(page);
    await expect(rangeStatus(page)).toHaveText(/^Rows 1–100 of [\d,]+$/);
    totals[label] = await totalCount(page);
  }
  const hour = totals["Last hour"] ?? 0;
  const day = totals["Last 24 hours"] ?? 0;
  const week = totals["Last 7 days"] ?? 0;
  const month = totals["Last 30 days"] ?? 0;
  const any = totals["Any time"] ?? 0;
  expect(hour).toBeGreaterThan(100);
  expect(hour).toBeLessThan(day);
  expect(day).toBeLessThan(week);
  expect(week).toBeLessThan(month);
  expect(month).toBeLessThanOrEqual(any);
  // The seeded history is about 412,000 messages and "Any time" holds all of it.
  expect(any).toBeGreaterThan(400_000);
  // Back to the default range: a plain address again.
  await choose(page, "Time range", "Last 24 hours");
  await settled(page);
  expect(page.url()).not.toMatch(/from=|range=/);
});

test("H-05: a custom range takes explicit times", async ({ page }) => {
  await openMessages(page);
  const day = await totalCount(page);
  await choose(page, "Time range", "Custom range…");
  const from = page.getByRole("textbox", { name: "From" });
  await expect(from).toBeVisible();
  await expect(page.getByRole("textbox", { name: "To" })).toBeVisible();
  // Two hours ago in this computer's time zone, which is what the field reads.
  const start = new Date(Date.now() - 2 * 3_600_000);
  const pad = (n: number) => String(n).padStart(2, "0");
  const local = `${start.getFullYear()}-${pad(start.getMonth() + 1)}-${pad(start.getDate())}T${pad(start.getHours())}:${pad(start.getMinutes())}`;
  await from.fill(local);
  await from.press("Enter");
  await expect(page).toHaveURL(/from=20\d\d-/);
  await settled(page);
  const two = await totalCount(page);
  expect(two).toBeGreaterThan(500);
  expect(two).toBeLessThan(day);
  const times = await receivedTimes(page);
  expect(Math.min(...times)).toBeGreaterThanOrEqual(start.getTime() - 60_000);
});

test("H-05: channels (several) narrow the list", async ({ page }) => {
  await openMessages(page);
  const all = await totalCount(page);
  await page.getByRole("button", { name: "Channels: All" }).click();
  const chooser = page.getByRole("dialog", { name: "Channels" });
  await tick(chooser, "AU680 results");
  await expectEveryRow(page, "Channel", (text) => text === "AU680 results", "one channel");
  await expect(page.getByRole("button", { name: "Channels: AU680 results" })).toBeVisible();
  const one = await totalCount(page);
  expect(one).toBeLessThan(all);
  await tick(chooser, "Vitros 350 results");
  await expectEveryRow(
    page,
    "Channel",
    (text) => text === "AU680 results" || text === "Vitros 350 results",
    "two channels",
  );
  await expect(page.getByRole("button", { name: "Channels: 2 selected" })).toBeVisible();
  expect(new Set(await columnTexts(page, "Channel"))).toEqual(
    new Set(["AU680 results", "Vitros 350 results"]),
  );
  expect(await totalCount(page)).toBeGreaterThan(one);
  expect(page.url()).toMatch(/channel=.*channel=/);
  // "Show all channels" puts every channel back.
  await chooser.getByRole("button", { name: "Show all channels" }).click();
  await expect(page.getByRole("button", { name: "Channels: All" })).toBeVisible();
  await expect.poll(() => totalCount(page)).toBeGreaterThanOrEqual(all - 5);
});

test("H-05: direction, message state and delivery state narrow the list", async ({ page }) => {
  await openMessages(page);
  const all = await totalCount(page);
  // Direction: the tap-observed messages travel between the sides of a bridge.
  await chooseMore(page, "Direction", "Side A to side B");
  await expectEveryRow(page, "Direction", (text) => text === "A to B", "direction A to B");
  const aToB = await totalCount(page);
  expect(aToB).toBeLessThan(all);
  await expect(page.getByText("Direction: Side A to side B")).toBeVisible();
  await choose(page, "Direction", "Inbound");
  await expectEveryRow(page, "Direction", (text) => text === "Inbound", "direction inbound");
  await choose(page, "Direction", "Any direction");
  await expect.poll(() => totalCount(page)).toBeGreaterThanOrEqual(all - 5);
  // Message state.
  await choose(page, "Message state", "Invalid");
  await expectEveryRow(page, "Message state", (text) => text === "Invalid", "state invalid");
  const invalid = await totalCount(page);
  expect(invalid).toBeLessThan(all);
  expect(page.url()).toContain("state=invalid");
  await choose(page, "Message state", "Any message state");
  await settled(page);
  // Delivery state: every row has a delivery in that state.
  await choose(page, "Delivery state", "Outcome unknown, may have been delivered");
  await expect
    .poll(async () => {
      const chips = await chipTitles(page);
      return chips.length > 0 && chips.every((row) => row.some((c) => /: Outcome unknown/.test(c)));
    })
    .toBe(true);
  await settled(page);
  expect(await totalCount(page)).toBeLessThan(all);
  expect(page.url()).toContain("delivery=outcome_unknown");
  await expect(page.getByText("Delivery: Outcome unknown, may have been delivered")).toBeVisible();
});

test("H-05: the identifier matches exactly or by prefix", async ({ page }) => {
  await openMessages(page);
  const identifiers = await columnTexts(page, "Identifier");
  const control = identifiers.find((text) => /^MC\d{8}$/.test(text));
  expect(control, "an HL7 control id on the first page").toBeDefined();
  const full = control ?? "";
  const all = await totalCount(page);
  // Prefix (the default): "MC" and the first digits find that message and any like it.
  await find(page, "Identifier", full.slice(0, 7));
  await expectEveryRow(
    page,
    "Identifier",
    (text) => text.startsWith(full.slice(0, 7)),
    "identifier prefix",
  );
  expect(await totalCount(page)).toBeLessThan(all);
  expect(await columnTexts(page, "Identifier")).toContain(full);
  // Exactly: what was typed must be a whole id, so the prefix finds nothing...
  await chooseMore(page, "Identifier match", "Exactly");
  await expect(page.getByText("No messages match")).toBeVisible();
  await expect(page.getByText("Identifier: exactly")).toBeVisible();
  // ...and the whole id finds that message (and anything repeating it) only.
  await find(page, "Identifier", full);
  await expectEveryRow(page, "Identifier", (text) => text === full, "identifier exactly");
  const exact = await totalCount(page);
  expect(exact).toBeGreaterThanOrEqual(1);
  expect(exact).toBeLessThan(10);
  // The patient and sample ids are found by the same box.
  await chooseMore(page, "Identifier match", "Starts with");
  await find(page, "Identifier", "SMP-0146");
  await expect.poll(() => totalCount(page)).toBeGreaterThan(exact);
  await find(page, "Identifier", "PT-0");
  await expect.poll(() => totalCount(page)).toBeGreaterThan(exact);
});

test("H-05: message text needs three characters, then searches stored messages", async ({
  page,
}) => {
  await openMessages(page);
  const all = await totalCount(page);
  // Two characters are not a search: the hint says so and the list is not narrowed.
  await find(page, "Message text", "ab");
  await expect(page.getByText("Type at least 3 characters to search text.")).toBeVisible();
  await expect.poll(() => totalCount(page)).toBeGreaterThanOrEqual(all - 5);
  // Three or more is: the hint now names the limit of the search.
  await find(page, "Message text", "SMP-0146");
  await expect(page.getByText("Text search covers the selected time range only.")).toBeVisible();
  await expect(page.getByText("Type at least 3 characters to search text.")).toHaveCount(0);
  await expect.poll(() => totalCount(page)).toBeLessThan(all);
  const narrowed = await totalCount(page);
  expect(narrowed).toBeGreaterThan(0);
  // Text that is in no message.
  await find(page, "Message text", "no-such-text-anywhere");
  await expect(page.getByText("No messages match")).toBeVisible();
  expect(narrowed).toBeGreaterThan(0);
});

test("H-05: a message ID under More filters finds that message", async ({ page }) => {
  await openMessages(page);
  await openMoreFilters(page);
  const field = page.getByRole("searchbox", { name: "Message ID" });
  // Fewer than 8 hex digits are not a search yet.
  await field.fill("0000");
  await expect(
    page.getByText("A message ID needs 8 to 32 hex digits, dashes optional."),
  ).toBeVisible();
  // The mock's ids are msg-<n>, stored as UUIDs ending in n in hex: msg-5 is ...-000000000005.
  await field.fill("00000000-0000-4000-8000-000000000005");
  await expect(messageRows(page)).toHaveCount(1);
  await settled(page);
  await expect(rangeStatus(page)).toHaveText("Rows 1–1 of 1");
  await expect(
    page.getByText("Message ID: starts with 00000000-0000-4000-8000-000000000005"),
  ).toBeVisible();
  // A shorter prefix finds the sixteen messages msg-0 to msg-15.
  await field.fill("0000000000004000800000000000");
  await expect.poll(() => totalCount(page)).toBeGreaterThan(1);
  await field.fill("0000000000004000800000000000000");
  await expect(rangeStatus(page)).toHaveText("Rows 1–16 of 16");
});

test("H-05: quick filters show counts and replace the state choices", async ({ page }) => {
  await openMessages(page);
  const all = await totalCount(page);
  const counts: Record<string, number> = {};
  for (const label of ["Needs attention", "Queued", "Duplicates", "Invalid or aborted"]) {
    counts[label] = await quickCount(page, label);
  }
  expect(counts["Needs attention"]).toBeGreaterThan(100);
  expect(counts.Duplicates).toBeGreaterThan(100);
  expect(counts["Invalid or aborted"]).toBeGreaterThan(100);
  // The header total of each filter is the count its chip showed.
  await quick(page, "Needs attention").click();
  await expect(quick(page, "Needs attention")).toHaveAttribute("aria-pressed", "true");
  await expect(rangeStatus(page)).toContainText(
    `of ${counts["Needs attention"]?.toLocaleString("en-US")}`,
  );
  await expect
    .poll(async () => {
      const chips = await chipTitles(page);
      return chips.every((row) =>
        row.some((c) => /: (Failed|Rejected|No acknowledgement|Outcome unknown)/.test(c)),
      );
    })
    .toBe(true);
  expect(page.url()).toContain("quick=attention");
  // Only one at a time.
  await quick(page, "Duplicates").click();
  await expect(quick(page, "Duplicates")).toHaveAttribute("aria-pressed", "true");
  await expect(quick(page, "Needs attention")).toHaveAttribute("aria-pressed", "false");
  await expectEveryRow(page, "Message state", (text) => text === "Duplicate", "duplicates");
  await expect(rangeStatus(page)).toContainText(`of ${counts.Duplicates?.toLocaleString("en-US")}`);
  await quick(page, "Invalid or aborted").click();
  await expectEveryRow(
    page,
    "Message state",
    (text) => text === "Invalid" || text === "Aborted",
    "invalid or aborted",
  );
  await quick(page, "Queued").click();
  await expect(rangeStatus(page)).toContainText(`of ${counts.Queued?.toLocaleString("en-US")}`);
  await expect
    .poll(async () => (await chipTitles(page)).every((row) => row.some((c) => /: Queued/.test(c))))
    .toBe(true);
  // Choosing a state replaces the quick filter...
  await chooseMore(page, "Message state", "Invalid");
  await expect(quick(page, "Queued")).toHaveAttribute("aria-pressed", "false");
  await expectEveryRow(page, "Message state", (text) => text === "Invalid", "state after quick");
  expect(page.url()).not.toContain("quick=");
  // ...and choosing a quick filter replaces the state.
  await quick(page, "Duplicates").click();
  await expect(page.getByText("State: Invalid")).toHaveCount(0);
  await expectEveryRow(page, "Message state", (text) => text === "Duplicate", "quick after state");
  expect(page.url()).not.toContain("state=");
  // Turning it off again shows everything.
  await quick(page, "Duplicates").click();
  await expect.poll(() => totalCount(page)).toBeGreaterThanOrEqual(all - 5);
});

test("H-05: Clear filters resets every criterion", async ({ page }) => {
  await openMessages(page);
  const all = await totalCount(page);
  await expect(clearFilters(page)).toHaveCount(0);
  await choose(page, "Time range", "Last 7 days");
  await page.getByRole("button", { name: "Channels: All" }).click();
  await tick(page.getByRole("dialog", { name: "Channels" }), "AU680 results");
  await page.keyboard.press("Escape");
  await chooseMore(page, "Direction", "Inbound");
  await find(page, "Identifier", "SMP-0");
  await find(page, "Message text", "SMP");
  await expectEveryRow(page, "Channel", (text) => text === "AU680 results", "narrowed");
  await expect(clearFilters(page)).toBeVisible();
  await clearFilters(page).click();
  await expect(page.getByRole("button", { name: "Channels: All" })).toBeVisible();
  await expect(page.getByRole("button", { name: /Time range$/ })).toContainText("Last 24 hours");
  await expect(page.getByRole("searchbox", { name: "Identifier" })).toHaveValue("");
  await expect(page.getByRole("searchbox", { name: "Message text" })).toHaveValue("");
  await expect(page.getByRole("button", { name: /Direction$/ })).toContainText("Any direction");
  await expect(clearFilters(page)).toHaveCount(0);
  await expect.poll(() => totalCount(page)).toBeGreaterThanOrEqual(all - 5);
  expect(page.url()).not.toMatch(/channel=|direction=|from=|range=/);
});
