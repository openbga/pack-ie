import { expect, type Locator, type Page, test } from "@playwright/test";
import { CHANNELS, channelRow, openMock } from "./operate.support";
import { dataRows, heading, loaded, mockHandle } from "./support";

const NODE = "Main Lab Interface PC";

const lostBanner = (page: Page) =>
  page.getByRole("alert").filter({ hasText: "Lost the connection to the engine" });

/** The smallest opacity an element or any of its ancestors has: below 1 means it is dimmed. */
async function effectiveOpacity(locator: Locator): Promise<number> {
  return locator.evaluate((element) => {
    let opacity = 1;
    for (let node: Element | null = element; node !== null; node = node.parentElement) {
      opacity *= Number.parseFloat(getComputedStyle(node).opacity);
    }
    return opacity;
  });
}

/** The traffic text of one running bridge row: it grows while the engine is live. */
const trafficOf = (page: Page) =>
  channelRow(page, "Channels", CHANNELS.au5800.label)
    .getByRole("gridcell")
    .filter({ hasText: /^A → B/ });

test("H-01: with no engine the console says so, says where it looked and offers to start the service", async ({
  page,
}) => {
  await openMock(page, "/dashboard", { offline: true });

  await expect(page.getByText("The engine is not running")).toBeVisible();
  await expect(page.getByText("No engine answered at mock://engine.")).toBeVisible();
  await expect(page.getByText(/The console keeps trying and continues as soon as/)).toBeVisible();
  await expect(page.getByText(/run this in an administrator terminal/)).toBeVisible();
  await expect(page.locator("code").filter({ hasText: /packie.*service start/ })).toBeVisible();
  await expect(page.getByRole("button", { name: "Start the service" })).toBeEnabled();
  // Nothing of the dashboard is shown while no engine answers.
  await expect(heading(page, "Dashboard")).toHaveCount(0);
  await expect(page.getByRole("grid", { name: "Channels" })).toHaveCount(0);
});

test("H-01: Start the service brings the engine up and the console continues by itself", async ({
  page,
}) => {
  await openMock(page, "/dashboard", { offline: true });
  await page.getByRole("button", { name: "Start the service" }).click();

  await expect(page.getByText("Started the demo engine.")).toBeVisible();
  // No reload and no further click: the dashboard appears and attaches to the node.
  await loaded(page, "Dashboard");
  await expect(page.getByText("The engine is not running")).toHaveCount(0);
  await expect(page.getByRole("banner").getByText(NODE)).toBeVisible();
  await expect(dataRows(page, "Channels").first()).toBeVisible();
  await expect(
    page.getByRole("region", { name: "Engine" }).getByText("Connected", { exact: true }),
  ).toBeVisible();
});

test("H-01: with an engine the console attaches and shows the node", async ({ page }) => {
  await openMock(page, "/dashboard");
  await loaded(page, "Dashboard");

  await expect(page.getByText("The engine is not running")).toHaveCount(0);
  await expect(lostBanner(page)).toHaveCount(0);
  await expect(page.getByRole("banner").getByText(NODE)).toBeVisible();
  const strip = page.getByRole("region", { name: "Engine" });
  await expect(strip.getByText(NODE)).toBeVisible();
  await expect(strip.getByText("Connected", { exact: true })).toBeVisible();
  await expect(dataRows(page, "Channels").first()).toBeVisible();
});

test("H-01: a lost connection keeps the pages with dimmed last data and a banner, and reconnects", async ({
  page,
}) => {
  await openMock(page, "/dashboard");
  await loaded(page, "Dashboard");
  const grid = page.getByRole("grid", { name: "Channels" });
  await expect(dataRows(page, "Channels").first()).toBeVisible();
  const rowsBefore = await dataRows(page, "Channels").count();
  expect(await effectiveOpacity(grid)).toBe(1);

  await mockHandle(page, (mock) => mock.simulateOutage());

  const banner = lostBanner(page);
  await expect(banner).toBeVisible();
  await expect(banner.getByText(/Reconnecting…/)).toBeVisible();
  await expect(banner.getByText(/What is shown may be out of date\./)).toBeVisible();
  // The page stays, with its last data, dimmed and labelled as such.
  await expect(heading(page, "Dashboard")).toBeVisible();
  await expect(dataRows(page, "Channels")).toHaveCount(rowsBefore);
  const strip = page.getByRole("region", { name: "Engine" });
  await expect(strip.getByText("Reconnecting", { exact: true })).toBeVisible();
  await expect(strip.getByText("The last known state is shown dimmed.")).toBeVisible();
  await expect.poll(() => effectiveOpacity(grid)).toBeLessThan(1);

  await mockHandle(page, (mock) => mock.endOutage());

  await expect(banner).toHaveCount(0);
  await expect(strip.getByText("Connected", { exact: true })).toBeVisible();
  await expect(strip.getByText("The last known state is shown dimmed.")).toHaveCount(0);
  await expect.poll(() => effectiveOpacity(grid)).toBe(1);
  // Live again: a running bridge's traffic counter moves on.
  const before = await trafficOf(page).innerText();
  await expect.poll(() => trafficOf(page).innerText()).not.toBe(before);
  await expect(dataRows(page, "Channels")).toHaveCount(rowsBefore);
});

test("H-01: a lost connection shows the banner on every page, not only the dashboard", async ({
  page,
}) => {
  await openMock(page, `/channels/${CHANNELS.c503.id}`);
  await loaded(page, CHANNELS.c503.label);

  await mockHandle(page, (mock) => mock.simulateOutage());

  await expect(lostBanner(page)).toBeVisible();
  await expect(heading(page, CHANNELS.c503.label)).toBeVisible();
  await expect(page.getByRole("region", { name: "Endpoints" })).toBeVisible();

  await mockHandle(page, (mock) => mock.endOutage());
  await expect(lostBanner(page)).toHaveCount(0);
  await expect(heading(page, CHANNELS.c503.label)).toBeVisible();
});

test("H-01: it reconnects after an engine restart and the app is usable", async ({ page }) => {
  await openMock(page, "/dashboard");
  await loaded(page, "Dashboard");
  await expect(dataRows(page, "Channels").first()).toBeVisible();

  await mockHandle(page, (mock) => mock.simulateOutage());
  await expect(lostBanner(page)).toBeVisible();
  await mockHandle(page, (mock) => mock.endOutage({ restarted: true }));

  await expect(lostBanner(page)).toHaveCount(0);
  const strip = page.getByRole("region", { name: "Engine" });
  await expect(strip.getByText("Connected", { exact: true })).toBeVisible();
  await expect(page.getByText("The engine is not running")).toHaveCount(0);

  // Usable: navigate, open a channel, and read its live data.
  const nav = page.getByRole("navigation", { name: "Main" });
  await nav.getByRole("link", { name: "Channels" }).click();
  await loaded(page, "Channels");
  await channelRow(page, "Channels", CHANNELS.sta.label).click();
  await loaded(page, CHANNELS.sta.label);
  await expect(page.getByRole("region", { name: "Destinations and deliveries" })).toBeVisible();

  await nav.getByRole("link", { name: "Logs" }).click();
  await loaded(page, "Logs");
  await expect(lostBanner(page)).toHaveCount(0);
});
