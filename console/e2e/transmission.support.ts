import { expect, type Locator, type Page } from "@playwright/test";
import { downloadFrom, downloadText } from "./download.support";
import { columnReader, countFrom, readGridRows } from "./grid.support";
import { CHANNELS, openMock } from "./operate.support";
import { loaded, type MockSwitches } from "./support";

export const GRID = "Deliveries";

/** The columns of the deliveries grid, in order, with the extra columns off. */
export const COLUMN = {
  received: 0,
  message: 1,
  control: 2,
  contentHash: 3,
  destination: 4,
  ackLevel: 5,
  ackCode: 6,
  ackControlId: 7,
  acknowledgedAt: 8,
  outcome: 9,
} as const;

/** The mock channel of the journeys: two destinations, unresolved deliveries (operate.support). */
export const STA = CHANNELS.sta;
/** The busiest mock channel: a month of it holds more deliveries than a report lists. */
export const BUSY = { id: "ch-msg-au680", label: "AU680 results" } as const;

/** Opens the Transmission report page (with `query` such as `?channel=ch-msg-sta`). */
export async function openReport(
  page: Page,
  query = "",
  switches: MockSwitches = {},
): Promise<void> {
  await openMock(page, `/transmission${query}`, switches);
  await loaded(page, "Transmission report");
}

export const runButton = (page: Page) => page.getByRole("button", { name: "Run report" });

/** Runs the report and waits for its answer: the result, an empty period or an error. */
export async function runReport(page: Page): Promise<void> {
  await runButton(page).click();
  await expect(
    page
      .getByRole("grid", { name: GRID })
      .or(page.getByText("No messages were received in this period"))
      .or(page.getByText("The report could not be run")),
  ).toBeVisible();
}

export const totalsTable = (page: Page) =>
  page.getByRole("table", { name: "Totals by destination" });

/** The totals table as numbers: label, then deliveries, accepted, received, written, unresolved, closed, pending. */
export async function totals(page: Page): Promise<{ label: string; counts: number[] }[]> {
  const rows = await totalsTable(page).locator("tbody tr").all();
  return Promise.all(
    rows.map(async (row) => ({
      label: ((await row.getByRole("rowheader").textContent()) ?? "").trim(),
      counts: (await row.getByRole("cell").allTextContents()).map((text) =>
        Number(text.replaceAll(",", "")),
      ),
    })),
  );
}

/** The count beside the filters: "640 deliveries" or "320 of 640 deliveries". */
export function countText(page: Page): Locator {
  return page.getByRole("status").filter({ hasText: /deliver(y|ies)$/ });
}

/** The rows the count says are listed. */
export async function listedCount(page: Page): Promise<number> {
  return countFrom(countText(page), /^([\d,]+)(?: of [\d,]+)? deliver(?:y|ies)$/);
}

/** Every row the grid holds, read by scrolling through it (it draws only the rows in view). */
export const readRows = (page: Page): Promise<string[][]> => readGridRows(page, GRID);

/** One column of read rows. */
export const column = columnReader(COLUMN);

/** Exports the rows shown through the dialog; returns the downloaded file's name and text. */
export async function exportCsv(page: Page): Promise<{ name: string; text: string }> {
  await page.getByRole("button", { name: "Export CSV…" }).click();
  const dialog = page.getByRole("dialog", { name: "Export transmission report" });
  await expect(dialog).toBeVisible();
  const file = await downloadFrom(page, dialog.getByRole("button", { name: "Export CSV" }));
  return { name: file.suggestedFilename(), text: await downloadText(file) };
}
