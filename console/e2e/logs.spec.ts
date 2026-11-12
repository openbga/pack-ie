import { expect, test } from "@playwright/test";
import {
  clockMs,
  column,
  countText,
  distinct,
  GRID,
  loadAllOlder,
  openLogs,
  readRows,
} from "./logs.support";
import { choose, dataRows, param } from "./support";

test.use({ timezoneId: "UTC" });

test("H-06 level filter: rows below the level disappear and only chips at or above it remain", async ({
  page,
}) => {
  await openLogs(page);
  const base = distinct(column(await readRows(page), "level"));
  expect(base).toEqual(expect.arrayContaining(["Info", "Warning", "Error"]));
  expect(base).not.toContain("Debug");
  expect(base).not.toContain("Trace");

  await choose(page, "Level", "Error only");
  await expect.poll(() => param(page, "level")).toBe("error");
  await expect(page.getByRole("button", { name: /Error only/ })).toBeVisible();
  await expect.poll(async () => distinct(column(await readRows(page), "level"))).toEqual(["Error"]);

  await choose(page, "Level", "Warning and above");
  await expect.poll(() => param(page, "level")).toBe("warn");
  await expect
    .poll(async () => distinct(column(await readRows(page), "level")).sort())
    .toEqual(["Error", "Warning"]);

  await choose(page, "Level", "Everything (trace)");
  await expect.poll(() => param(page, "level")).toBe("trace");
  await expect
    .poll(async () => distinct(column(await readRows(page), "level")))
    .toEqual(expect.arrayContaining(["Trace", "Debug", "Info", "Warning", "Error"]));

  await choose(page, "Level", "Info and above");
  await expect.poll(() => param(page, "level")).toBeNull();
});

const HOUR = 3_600_000;

test("H-06 time range: the presets look back from now and a custom range ends in the past", async ({
  page,
}) => {
  await openLogs(page);
  await expect(page.getByRole("button", { name: /Live Time range/ })).toBeVisible();
  const live = await readRows(page);
  const newestLive = clockMs(column(live, "time")[0] ?? "");

  await choose(page, "Time range", "Last 15 minutes");
  await expect.poll(() => param(page, "range")).toBe("15m");
  await expect(page.getByText("Not following", { exact: true })).toBeVisible();
  await loadAllOlder(page);
  await expect(countText(page)).toContainText("no older events in the engine's log");
  const quarter = column(await readRows(page), "time").map(clockMs);
  expect(quarter.length).toBeGreaterThan(200);
  expect(newestLive - Math.min(...quarter)).toBeLessThanOrEqual(15 * 60_000 + 5_000);
  expect(newestLive - Math.min(...quarter)).toBeGreaterThan(10 * 60_000);

  await choose(page, "Time range", "Last hour");
  await expect.poll(() => param(page, "range")).toBe("1h");
  await expect(countText(page)).toContainText("older events available");

  await choose(page, "Time range", "Last 24 hours");
  await expect.poll(() => param(page, "range")).toBe("24h");

  // A custom range that ended an hour ago.
  await choose(page, "Time range", "Custom range…");
  await expect.poll(() => param(page, "range")).toBe("custom");
  const stamp = (ago: number) => new Date(Date.now() - ago).toISOString().slice(0, 16);
  await page.getByLabel("From", { exact: true }).fill(stamp(2 * HOUR));
  await page.getByLabel("To", { exact: true }).fill(stamp(HOUR));
  await page.getByLabel("To", { exact: true }).press("Enter");
  await expect.poll(() => param(page, "to")).not.toBeNull();
  expect(param(page, "from")).toMatch(/^\d{4}-\d\d-\d\dT/);
  await expect(page.getByText("Not following: the range ends in the past")).toBeVisible();
  await expect(page.getByRole("button", { name: "Follow" })).toBeDisabled();
  await expect(dataRows(page, GRID).first()).toBeVisible();
  const past = column(await readRows(page), "time").map(clockMs);
  expect(past.length).toBeGreaterThan(0);
  const gap = newestLive - (past[0] ?? 0);
  expect(gap).toBeGreaterThanOrEqual(HOUR - 30_000);
  expect(gap).toBeLessThanOrEqual(HOUR + 120_000);
  expect(past.every((time) => time >= newestLive - 2 * HOUR - 120_000)).toBe(true);
});

test("H-06 channel and endpoint filters show only events of that channel or endpoint", async ({
  page,
}) => {
  await openLogs(page, "?range=1h");
  await choose(page, "Channel", "Vitros 350 results");
  await expect.poll(() => param(page, "channel")).not.toBeNull();
  await expect
    .poll(async () => distinct(column(await readRows(page), "channel")))
    .toEqual(["Vitros 350 results"]);
  expect(column(await readRows(page), "time").length).toBeGreaterThan(5);

  await choose(page, "Channel", "All channels");
  await expect.poll(() => param(page, "channel")).toBeNull();
  await choose(page, "Endpoint", "LIS HL7 out");
  await expect.poll(() => param(page, "endpoint")).not.toBeNull();
  await expect
    .poll(async () => distinct(column(await readRows(page), "endpoint")))
    .toEqual(["LIS HL7 out"]);

  // Both together narrow further, and Clear filters restores the defaults.
  await choose(page, "Channel", "Reference lab results");
  await expect
    .poll(async () => distinct(column(await readRows(page), "channel")))
    .toEqual(["Reference lab results"]);
  await page.getByRole("button", { name: "Clear filters" }).click();
  await expect.poll(() => new URL(page.url()).hash).toBe("#/logs");
  await expect(page.getByRole("button", { name: /All channels/ })).toBeVisible();
  await expect(page.getByRole("button", { name: /All endpoints/ })).toBeVisible();
});

test("H-06 event filter: suggestions from the catalog, whole-name match and the clear control", async ({
  page,
}) => {
  await openLogs(page, "?range=1h");
  const event = page.getByRole("combobox", { name: "Event" });
  const names = await page
    .locator("datalist option")
    .evaluateAll((options) => options.map((option) => option.getAttribute("value")));
  expect(names).toEqual(
    expect.arrayContaining(["channel.started", "delivery.state_changed", "config.applied"]),
  );
  expect(names.length).toBeGreaterThan(30);

  await event.fill("delivery.state_changed");
  await expect.poll(() => param(page, "event")).toBe("delivery.state_changed");
  await expect
    .poll(async () => distinct(column(await readRows(page), "event")))
    .toEqual(["delivery.state_changed"]);

  // The engine matches the whole name: a part of it finds nothing, and the page says so.
  await event.fill("delivery");
  await expect.poll(() => param(page, "event")).toBe("delivery");
  await expect(page.getByText("The engine matches the whole event name")).toBeVisible();
  await expect(dataRows(page, GRID)).toHaveCount(0);

  await page.getByRole("button", { name: "Clear event filter" }).click();
  await expect.poll(() => param(page, "event")).toBeNull();
  await expect(event).toHaveValue("");
  await expect(dataRows(page, GRID).first()).toBeVisible();
  expect(distinct(column(await readRows(page), "event")).length).toBeGreaterThan(2);
});

test("H-06 text filter is case-insensitive over message, target and fields", async ({ page }) => {
  await openLogs(page, "?range=1h");
  const search = page.getByRole("searchbox", { name: "Search log messages" });

  // The message.
  await search.fill("ReFuSeD");
  await expect.poll(() => param(page, "text")).toBe("ReFuSeD");
  await expect
    .poll(async () => {
      const messages = column(await readRows(page), "message");
      return messages.length > 0 && messages.every((message) => /refused/i.test(message));
    })
    .toBe(true);

  // The target, which no column shows: the row's panel does.
  await search.fill("PACKIE_ROUTING::MESSAGE");
  await expect.poll(() => param(page, "text")).toBe("PACKIE_ROUTING::MESSAGE");
  await expect(dataRows(page, GRID).first()).toBeVisible();
  await dataRows(page, GRID).first().click();
  await expect(
    page.getByRole("region", { name: /^Event \d+/ }).getByText("packie_routing::message"),
  ).toBeVisible();
  await page.getByRole("button", { name: "Close" }).click();

  // A field name, which neither the message nor the target holds.
  await search.fill("ACK_LEVEL");
  await expect.poll(() => param(page, "text")).toBe("ACK_LEVEL");
  await expect(dataRows(page, GRID).first()).toBeVisible();
  await dataRows(page, GRID).first().click();
  await expect(
    page.getByRole("region", { name: /^Event \d+/ }).getByText("ack_level"),
  ).toBeVisible();
  await page.getByRole("button", { name: "Close" }).click();

  await search.fill("no such text anywhere zzz");
  await expect(dataRows(page, GRID)).toHaveCount(0);
  await expect(page.getByText(/No events/i)).toBeVisible();
});

test("H-06 every filter is in the URL and restored on reload", async ({ page }) => {
  await openLogs(page);
  await choose(page, "Level", "Warning and above");
  await choose(page, "Time range", "Last hour");
  await choose(page, "Channel", "Vitros 350 results");
  await page.getByRole("combobox", { name: "Event" }).fill("delivery.state_changed");
  await expect.poll(() => param(page, "event")).toBe("delivery.state_changed");
  await page.getByRole("searchbox", { name: "Search log messages" }).fill("timeout");
  await expect.poll(() => param(page, "text")).toBe("timeout");
  const channel = param(page, "channel");
  expect(channel).not.toBeNull();
  expect(param(page, "level")).toBe("warn");
  expect(param(page, "range")).toBe("1h");

  await choose(page, "Endpoint", "LIS HL7 out");
  await expect.poll(() => param(page, "endpoint")).not.toBeNull();
  const address = page.url();

  await page.reload();
  await expect(page.getByRole("heading", { level: 1, name: "Logs" })).toBeVisible();
  expect(page.url()).toBe(address);
  await expect(page.getByRole("button", { name: /Warning and above/ })).toBeVisible();
  await expect(page.getByRole("button", { name: /Last hour/ })).toBeVisible();
  await expect(page.getByRole("button", { name: /Vitros 350 results/ })).toBeVisible();
  await expect(page.getByRole("button", { name: /LIS HL7 out/ })).toBeVisible();
  await expect(page.getByRole("combobox", { name: "Event" })).toHaveValue("delivery.state_changed");
  await expect(page.getByRole("searchbox", { name: "Search log messages" })).toHaveValue("timeout");
  await expect(page.getByRole("button", { name: "Clear filters" })).toBeVisible();
});
