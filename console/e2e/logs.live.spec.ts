import { expect, test } from "@playwright/test";
import {
  COLUMN,
  clockMs,
  column,
  countText,
  followState,
  GRID,
  gridElement,
  heldCount,
  loadedCount,
  looksLikeBody,
  newestTime,
  openLogs,
  publishEvents,
  readRows,
  scrollAway,
} from "./logs.support";
import { dataRows, eventPanel, heading, mockHandle, param } from "./support";

test.use({ timezoneId: "UTC" });

test("H-06 follows live: new events appear at the top without any action", async ({ page }) => {
  await openLogs(page);
  await expect(followState(page)).toHaveText(/^Following/);
  await expect(page.getByRole("button", { name: "Follow" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  const before = await loadedCount(page);
  const newest = await newestTime(page);
  await publishEvents(page, 40);
  await expect.poll(() => loadedCount(page)).toBeGreaterThanOrEqual(before + 30);
  await expect.poll(() => newestTime(page)).not.toBe(newest);
  expect(clockMs(await newestTime(page))).toBeGreaterThan(clockMs(newest));
  await expect(followState(page)).toHaveText(/^Following/);
});

test("H-06 scrolling away pauses following, counts new events and Jump to live resumes", async ({
  page,
}) => {
  await openLogs(page);
  await expect(page.getByRole("button", { name: "Jump to live" })).toHaveCount(0);
  await scrollAway(page);
  await expect(followState(page)).toHaveText(/^Paused — \d+ new events?/);
  await expect(page.getByRole("button", { name: "Jump to live" })).toBeVisible();

  const frozen = await loadedCount(page);
  await publishEvents(page, 40);
  await expect.poll(() => heldCount(page)).toBeGreaterThanOrEqual(30);
  expect(await loadedCount(page)).toBe(frozen);
  const held = await heldCount(page);

  await page.getByRole("button", { name: "Jump to live" }).click();
  await expect(followState(page)).toHaveText(/^Following/);
  await expect(page.getByRole("button", { name: "Pause" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Jump to live" })).toHaveCount(0);
  expect(await loadedCount(page)).toBeGreaterThanOrEqual(frozen + held);
  await expect.poll(() => gridElement(page).evaluate((grid) => grid.scrollTop)).toBe(0);
});

test("H-06 the Pause button holds the list and Jump to live resumes it", async ({ page }) => {
  await openLogs(page);
  await page.getByRole("button", { name: "Pause" }).click();
  await expect(followState(page)).toHaveText(/^Paused/);
  const frozen = await loadedCount(page);
  const newest = await newestTime(page);
  await publishEvents(page, 30);
  await expect.poll(() => heldCount(page)).toBeGreaterThanOrEqual(20);
  expect(await loadedCount(page)).toBe(frozen);
  expect(await newestTime(page)).toBe(newest);

  await page.getByRole("button", { name: "Jump to live" }).click();
  await expect(followState(page)).toHaveText(/^Following/);
  await expect.poll(() => loadedCount(page)).toBeGreaterThanOrEqual(frozen + 20);
  expect(clockMs(await newestTime(page))).toBeGreaterThan(clockMs(newest));

  // Following can also be switched off and on.
  await page.getByRole("button", { name: "Follow" }).click();
  await expect(followState(page)).toHaveText("Not following");
  await page.getByRole("button", { name: "Follow" }).click();
  await expect(followState(page)).toHaveText(/^Following/);
});

test("H-06 opening a row shows its fields and links to its channel and endpoint", async ({
  page,
}) => {
  await openLogs(page, "?range=1h&event=delivery.state_changed");
  const first = dataRows(page, GRID).first();
  const channel = (await first.getByRole("gridcell").nth(COLUMN.channel).textContent()) ?? "";
  const endpoint = (await first.getByRole("gridcell").nth(COLUMN.endpoint).textContent()) ?? "";
  await first.click();

  const panel = eventPanel(page);
  await expect(panel).toBeVisible();
  const seq = /^Event (\d+)$/.exec(
    (await panel.getByRole("heading", { level: 2 }).textContent()) ?? "",
  );
  expect(seq?.[1]).toBeDefined();
  for (const label of ["Time", "Event", "Target", "Sequence", "Channel", "Endpoint", "Message"]) {
    await expect(panel.locator("dt", { hasText: new RegExp(`^${label}$`) })).toBeVisible();
  }
  await expect(panel.locator("dd", { hasText: /^delivery\.state_changed$/ })).toBeVisible();
  await expect(panel.locator("dd", { hasText: /^packie_routing::message$/ })).toBeVisible();
  await expect(panel.locator("dd", { hasText: new RegExp(`^${seq?.[1]}$`) })).toBeVisible();
  await expect(panel.getByRole("heading", { level: 3, name: "Fields" })).toBeVisible();
  await expect(panel.locator("dt", { hasText: /^channel_id$/ })).toBeVisible();

  const channelLink = panel.getByRole("link", { name: channel });
  const endpointLink = panel.getByRole("link", { name: endpoint });
  const messageLink = panel.getByRole("link", { name: /^msg-\d+$/ });
  await expect(channelLink).toHaveAttribute("href", /#\/channels\/.+/);
  await expect(endpointLink).toHaveAttribute("href", /#\/endpoints\/.+/);
  await expect(messageLink).toHaveAttribute("href", /#\/messages\/msg-\d+/);

  await channelLink.click();
  await expect(page).toHaveURL(/#\/channels\//);
  await expect(heading(page, channel)).toBeVisible();
  await page.goBack();
  await expect(eventPanel(page).or(dataRows(page, GRID).first())).toBeVisible();
  await openLogs(page, "?range=1h&event=delivery.state_changed");
  await dataRows(page, GRID).first().click();
  await eventPanel(page).getByRole("link", { name: /.+/ }).filter({ hasText: /./ }).nth(1).click();
  await expect(page).toHaveURL(/#\/endpoints\//);
});

test("H-06 opening a row while following pauses, closing resumes", async ({ page }) => {
  await openLogs(page);
  await dataRows(page, GRID).nth(2).click();
  await expect(eventPanel(page)).toBeVisible();
  await expect(followState(page)).toHaveText(/^Paused/);
  await expect(page.getByRole("button", { name: "Next" })).toBeEnabled();
  await eventPanel(page).getByRole("button", { name: "Close" }).click();
  await expect(eventPanel(page)).toHaveCount(0);
  await expect(followState(page)).toHaveText(/^Following/);
});

test("H-06 Load older events pages back without repeating events", async ({ page }) => {
  await openLogs(page, "?range=1h");
  await expect(countText(page)).toHaveText("200 loaded · older events available");
  await expect(gridElement(page)).toHaveAttribute("aria-rowcount", "201");

  await page.getByRole("button", { name: "Load older events" }).click();
  await expect(countText(page)).toHaveText("400 loaded · older events available");
  await expect(gridElement(page)).toHaveAttribute("aria-rowcount", "401");
  const times = column(await readRows(page), "time").map(clockMs);
  expect(times).toHaveLength(400);
  expect(new Set(times).size).toBe(400);
  expect(times.every((time, index) => index === 0 || time < (times[index - 1] ?? 0))).toBe(true);

  await page.getByRole("button", { name: "Load older events" }).click();
  await expect(countText(page)).toHaveText("600 loaded · older events available");
});

test("H-06 no message body appears in the logs, and the help says bodies are never logged", async ({
  page,
}) => {
  await openLogs(page, "?level=trace&range=1h");
  const rows = await readRows(page);
  expect(rows.length).toBeGreaterThan(100);
  for (const row of rows) expect(looksLikeBody(row.join(" "))).toBe(false);

  await dataRows(page, GRID).first().click();
  expect(looksLikeBody((await eventPanel(page).textContent()) ?? "")).toBe(false);

  // Searching for a body marker finds no event: nothing the engine logs holds one.
  for (const marker of ["MSH|", "<STX>", "OBX|"]) {
    await page.getByRole("searchbox", { name: "Search log messages" }).fill(marker);
    await expect.poll(() => param(page, "text")).toBe(marker);
    await expect(dataRows(page, GRID)).toHaveCount(0);
    await expect(page.getByText(/No events/i)).toBeVisible();
  }

  await page.getByRole("button", { name: "Help" }).click();
  const help = page.getByRole("region", { name: "Page help" });
  await expect(help.getByText(/Message bodies are never logged\./)).toBeVisible();
});

test("H-06 during an outage the list waits for the engine and following resumes by itself", async ({
  page,
}) => {
  await openLogs(page);
  await expect(followState(page)).toHaveText(/^Following/);
  await mockHandle(page, (mock) => mock.simulateOutage());
  await expect(followState(page)).toHaveText(
    "Waiting for the engine… following resumes automatically",
  );
  await expect(dataRows(page, GRID).first()).toBeVisible();
  await publishEvents(page, 25);

  await mockHandle(page, (mock) => mock.endOutage());
  await expect(followState(page)).toHaveText(/^Following/);
  await expect(dataRows(page, GRID).first()).toBeVisible();
  const before = await loadedCount(page);
  await publishEvents(page, 30);
  await expect.poll(() => loadedCount(page)).toBeGreaterThanOrEqual(before + 20);
});

test("H-06 after an outage a divider says what reconnecting filled in", async ({ page }) => {
  await openLogs(page);
  await mockHandle(page, (mock) => mock.simulateOutage());
  await expect(followState(page)).toHaveText(/^Waiting for the engine…/);
  await publishEvents(page, 25);
  await mockHandle(page, (mock) => mock.endOutage());
  await expect(followState(page)).toHaveText(/^Following/);
  const divider = dataRows(page, GRID).filter({ hasText: /Connection lost/ });
  await expect(divider).toHaveCount(1, { timeout: 3_000 });
  const text = (await divider.textContent()) ?? "";
  expect(text).toMatch(/Connection lost .+ — reconnected .+, \d+ events? filled in\./);
  expect(Number(/(\d+) events? filled in/.exec(text)?.[1])).toBeGreaterThanOrEqual(15);
});

test("H-06 after an outage with an engine restart the divider says earlier events are gone", async ({
  page,
}) => {
  await openLogs(page);
  await mockHandle(page, (mock) => mock.simulateOutage());
  await expect(followState(page)).toHaveText(/^Waiting for the engine…/);
  await mockHandle(page, (mock) => mock.endOutage({ restarted: true }));
  const divider = dataRows(page, GRID).filter({ hasText: /Connection lost/ });
  await expect(divider).toHaveCount(1, { timeout: 3_000 });
  await expect(divider).toContainText("Engine restarted — earlier events are no longer available.");
  await expect(followState(page)).toHaveText(/^Following/);
});
