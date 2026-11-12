import { expect, type Page } from "@playwright/test";

/**
 * The mock engine's switches (src/api/index.ts `mockOptions`), as the address a journey opens.
 * `e2e=1` always, so a journey can reach `window.__packieMock`.
 */
export interface MockSwitches {
  /** `empty` (a fresh install), `errors` (every request fails), `slow` (1.5 s per request). */
  scenario?: "empty" | "errors" | "slow";
  /** Seeds this many messages (default about 412,000). */
  messages?: number;
  /** Starts with no reachable engine. */
  offline?: boolean;
  /** Lets endpoints fail by chance, as in the demo (journeys do not want that by default). */
  volatile?: boolean;
}

/** Opens `route` (a hash route such as `/dashboard`) against the mock, and waits for the page. */
export async function open(page: Page, route: string, switches: MockSwitches = {}): Promise<void> {
  const query = new URLSearchParams({
    mock: switches.offline ? "offline" : "1",
    e2e: "1",
    steady: switches.volatile ? "0" : "1",
  });
  if (switches.scenario) query.set("scenario", switches.scenario);
  if (switches.messages !== undefined) query.set("messages", String(switches.messages));
  await page.goto(`/?${query.toString()}#${route}`);
}

/** Follows the main navigation's link called `name`, without reloading the app. */
export async function navigateTo(page: Page, name: string | RegExp): Promise<void> {
  await page.getByRole("navigation", { name: "Main" }).getByRole("link", { name }).click();
}

/** The page's single `h1`, which every screen has. */
export function heading(page: Page, name: string | RegExp) {
  return page.getByRole("heading", { level: 1, name });
}

/** The page's main area. */
export const main = (page: Page) => page.getByRole("main");

/** The open event's panel of the Audit and Logs pages, named "Event 42". */
export function eventPanel(page: Page) {
  return page.getByRole("region", { name: /^Event \d+$/ });
}

/** Waits until the page has left its loading state: its `h1` is there and nothing says loading. */
export async function loaded(page: Page, name: string | RegExp): Promise<void> {
  await expect(heading(page, name)).toBeVisible();
  await expect(page.getByText(/^Loading /)).toHaveCount(0);
}

/** The rows of the grid called `name`, without its header row. */
export function dataRows(page: Page, name: string | RegExp) {
  return page
    .getByRole("grid", { name })
    .getByRole("row")
    .filter({ has: page.getByRole("gridcell") });
}

/** The mock handle `?e2e=1` exposes, for outages and log events. */
export async function mockHandle<T>(
  page: Page,
  run: (handle: NonNullable<Window["__packieMock"]>) => T,
): Promise<T> {
  return page.evaluate(`(${run.toString()})(window.__packieMock)`) as Promise<T>;
}

/** Picks `option` in the select called `label` (a React Aria select: button and listbox). */
export async function choose(page: Page, label: string, option: string | RegExp): Promise<void> {
  await page.getByRole("button", { name: new RegExp(`${label}$`) }).click();
  await page.getByRole("option", { name: option, exact: typeof option === "string" }).click();
}

/** A value of the page's address, from its hash route's query. */
export function param(page: Page, name: string): string | null {
  const hash = new URL(page.url()).hash;
  const query = hash.includes("?") ? hash.slice(hash.indexOf("?") + 1) : "";
  return new URLSearchParams(query).get(name);
}
