import { expect, type Locator, type Page } from "@playwright/test";
import { columnReader, countFrom, readGridRows } from "./grid.support";
import { dataRows, loaded, type MockSwitches, open } from "./support";

const GRID = "Audit events";

/** The default columns of the audit grid at 1280×800 with no event open, in order (Origin is optional). */
export const COLUMN = { time: 0, actor: 1, action: 2, object: 3, outcome: 4 } as const;

/** Opens the Audit page (with `query` such as `?action=config_apply`) and waits for its first rows. */
export async function openAudit(
  page: Page,
  query = "",
  switches: MockSwitches = {},
): Promise<void> {
  await open(page, `/audit${query}`, switches);
  await loaded(page, "Audit");
  await expect(dataRows(page, GRID).first()).toBeVisible();
}

/** The status text beside the filters: "100 events loaded · older events available". */
export function countText(page: Page): Locator {
  return page.getByRole("status").filter({ hasText: /events? loaded/ });
}

/** How many events the list says it holds. */
export async function loadedCount(page: Page): Promise<number> {
  return countFrom(countText(page), /^([\d,]+) events? loaded/);
}

/** One column of the rows the grid draws now (it draws only the rows in view), as text. */
export async function visibleColumn(page: Page, which: keyof typeof COLUMN): Promise<string[]> {
  const rows = await dataRows(page, GRID).all();
  return Promise.all(
    rows.map(
      async (row) => (await row.getByRole("gridcell").nth(COLUMN[which]).textContent()) ?? "",
    ),
  );
}

/** Every row the grid holds, read by scrolling through it, as the log journeys do. */
export const readRows = (page: Page): Promise<string[][]> => readGridRows(page, GRID);

/** One column of read rows. */
export const column = columnReader(COLUMN);

/** The result of "Verify chain". */
export function verification(page: Page): Locator {
  return page.getByRole("region", { name: "Chain verification" });
}

/** Presses Verify chain and waits for its result to show `text`. */
export async function verifyChain(page: Page, text: string | RegExp): Promise<Locator> {
  await page.getByRole("button", { name: "Verify chain" }).click();
  await expect(verification(page)).toContainText(text);
  return verification(page);
}

/** The row of the grid whose Action cell reads `action`, first match. */
export function rowOfAction(page: Page, action: string): Locator {
  return dataRows(page, GRID)
    .filter({ has: page.getByRole("gridcell", { name: action, exact: true }) })
    .first();
}
