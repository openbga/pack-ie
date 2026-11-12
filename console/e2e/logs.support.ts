import { expect, type Locator, type Page } from "@playwright/test";
import { columnReader, countFrom, readGridRows } from "./grid.support";
import { openMock } from "./operate.support";
import { dataRows, loaded, type MockSwitches } from "./support";

/** The columns of the log grid, in order. */
export const COLUMN = { time: 0, level: 1, event: 2, channel: 3, endpoint: 4, message: 5 } as const;

export const GRID = "Log events";

/** Opens the Logs page (with `query` such as `?level=warn`) and waits for its first rows. */
export async function openLogs(page: Page, query = "", switches: MockSwitches = {}): Promise<void> {
  await openMock(page, `/logs${query}`, switches);
  await loaded(page, "Logs");
  await expect(dataRows(page, GRID).first()).toBeVisible();
}

/** The status text beside the filters: "207 loaded · older events available". */
export function countText(page: Page): Locator {
  return page.getByRole("status").filter({ hasText: /loaded/ });
}

/** How many events the list says it holds. */
export async function loadedCount(page: Page): Promise<number> {
  return countFrom(countText(page), /^([\d,]+) loaded/);
}

/** The follow state in words: "Following · …", "Paused — 3 new events", "Not following". */
export function followState(page: Page): Locator {
  return page.getByRole("status").filter({ hasText: /^(Following|Paused|Not following|Waiting)/ });
}

/**
 * Every row the grid holds, read by scrolling through it (the grid draws only the rows in view).
 * Scrolling away from the newest event pauses following, so use it on a list that is not
 * followed or already paused; it scrolls back to the top at the end.
 */
export const readRows = (page: Page): Promise<string[][]> => readGridRows(page, GRID);

/** One column of the rows, as text. */
export const column = columnReader(COLUMN);

/** The distinct values of a column. */
export const distinct = (values: readonly string[]): string[] => [...new Set(values)];

/** Parses a displayed time ("2026-11-06 18:51:01.860") as a plain clock reading, in ms. */
export function clockMs(shown: string): number {
  const match = /^(\d{4})-(\d\d)-(\d\d) (\d\d):(\d\d):(\d\d)\.(\d{3})/.exec(shown);
  if (!match) throw new Error(`not a log time: ${shown}`);
  const [, y, mo, d, h, mi, s, ms] = match.map(Number);
  return Date.UTC(y ?? 0, (mo ?? 1) - 1, d ?? 1, h ?? 0, mi ?? 0, s ?? 0, ms ?? 0);
}

/** Presses "Load older events" until the list says there is nothing older. */
export async function loadAllOlder(page: Page): Promise<void> {
  const button = page.getByRole("button", { name: "Load older events" });
  const done = countText(page).filter({ hasText: "no older events" });
  for (let guard = 0; guard < 60; guard++) {
    await expect(button.or(done)).toBeVisible();
    if (await done.isVisible()) return;
    const before = await loadedCount(page);
    await button.click();
    await expect.poll(() => loadedCount(page), { timeout: 15_000 }).toBeGreaterThan(before);
  }
}

/** The engine logs `count` more events now (they reach followers on the next tick). */
export async function publishEvents(page: Page, count: number): Promise<void> {
  await page.evaluate(`window.__packieMock.publishLogEvents(${count})`);
}

/** The grid element, to scroll it. */
export const gridElement = (page: Page): Locator => page.getByRole("grid", { name: GRID });

/** Scrolls the grid away from its newest row with the mouse wheel, as an operator would. */
export async function scrollAway(page: Page): Promise<void> {
  const box = await gridElement(page).boundingBox();
  if (!box) throw new Error("grid is not visible");
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.wheel(0, 700);
}

/** The number of events a "Paused — N new events" status reports; zero when not paused. */
export async function heldCount(page: Page): Promise<number> {
  const text = (await followState(page).textContent()) ?? "";
  const match = /^Paused — ([\d,]+) new/.exec(text);
  return match?.[1] ? Number(match[1].replaceAll(",", "")) : 0;
}

/** The first row's time cell: the newest event shown. */
export async function newestTime(page: Page): Promise<string> {
  const cell = dataRows(page, GRID).first().getByRole("gridcell").first();
  return (await cell.textContent()) ?? "";
}

const BODY_MARKERS = /MSH\||PID\||OBX\||ORC\||<STX>|<ETX>|\bH\|\\\^&/;
const FRAMING_BYTES = [2, 3, 4].map((code) => String.fromCharCode(code));

/** Whether text holds what a message body holds: HL7 or ASTM records, or their framing bytes. */
export function looksLikeBody(text: string): boolean {
  return BODY_MARKERS.test(text) || FRAMING_BYTES.some((byte) => text.includes(byte));
}
