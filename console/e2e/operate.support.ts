import type { Locator, Page } from "@playwright/test";
import { dataRows, loaded, type MockSwitches, open } from "./support";

/** The mock's channel ids and labels the operate journeys rely on (src/api/mock/scenario.ts). */
export const CHANNELS = {
  /** Degraded, one destination in error, 14 unresolved, a critical alert. */
  sta: { id: "ch-msg-sta", label: "STA Compact results" },
  /** Retrying, a critical alert (its SFTP source is in error). */
  reflab: { id: "ch-msg-reflab", label: "Reference lab results" },
  /** Bridge retrying: side B cannot connect; no critical alert. */
  c503: { id: "ch-br-c503", label: "cobas c 503 to middleware" },
  /** Bridge degraded: a buffer overflow episode under way. */
  im1600: { id: "ch-br-im1600", label: "Atellica IM1600 to middleware" },
  /** Paused by an operator. */
  vitros350: { id: "ch-msg-vitros350", label: "Vitros 350 results" },
  /** Stopped for an invalid configuration. */
  outreach: { id: "ch-msg-outreach", label: "Outreach results relay" },
  /** Running and healthy bridge. */
  au5800: { id: "ch-br-au5800", label: "AU5800 chem analyzer to middleware" },
  /** Running and healthy message channel. */
  urisys: { id: "ch-msg-urisys", label: "Urisys results" },
} as const;

/** The row of the dashboard or Channels grid whose first cell starts with `label`. */
export function channelRow(page: Page, grid: string, label: string): Locator {
  return dataRows(page, grid).filter({
    has: page.getByRole("gridcell", { name: new RegExp(`^${escapeRegExp(label)}`) }),
  });
}

export function escapeRegExp(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/** The notice (toast) with this title, from the Notifications region. */
export function toast(page: Page, title: string | RegExp): Locator {
  return page
    .getByRole("region", { name: "Notifications" })
    .getByRole("listitem")
    .filter({ hasText: title });
}

/**
 * Opens `route` against the mock, with `window.__packieMock` bound to the engine the app uses.
 *
 * NOTE: in the dev server, React's StrictMode runs the `useState(createEngineApi)` initializer
 * twice and keeps the first engine, but `createEngineApi` points `window.__packieMock` at the
 * second, so `simulateOutage()` would act on an engine nobody looks at. Until the app exposes
 * the handle of the engine it keeps, the first assignment wins here.
 */
export async function openMock(
  page: Page,
  route: string,
  switches: MockSwitches = {},
): Promise<void> {
  await page.addInitScript(() => {
    let first: unknown;
    Object.defineProperty(window, "__packieMock", {
      configurable: true,
      get: () => first,
      set: (value: unknown) => {
        first ??= value;
      },
    });
  });
  await open(page, route, switches);
}

/** The value of a labelled fact (`dt`) inside `scope`: the element that follows its term. */
export function fact(scope: Locator, term: string): Locator {
  return scope
    .locator("dt")
    .filter({ hasText: new RegExp(`^${escapeRegExp(term)}$`) })
    .locator("xpath=following-sibling::*[1]");
}

/** The page's own last-error note (the first, before any destination's). */
export function lastErrorNote(page: Page): Locator {
  return page
    .getByRole("main")
    .getByRole("alert")
    .or(page.getByRole("main").getByRole("status"))
    .filter({ has: page.getByText("Last error", { exact: true }) })
    .first();
}

/** Opens the page of a channel against the mock and waits for it. */
export async function openChannel(
  page: Page,
  channel: { id: string; label: string },
  switches: MockSwitches = {},
): Promise<void> {
  await openMock(page, `/channels/${channel.id}`, switches);
  await loaded(page, channel.label);
}
