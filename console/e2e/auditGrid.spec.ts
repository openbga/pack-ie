import { expect, type Locator, type Page, test } from "@playwright/test";
import { openAudit, rowOfAction } from "./audit.support";
import { tick } from "./messages.support";
import { dataRows, eventPanel } from "./support";

test.use({ timezoneId: "UTC" });

const GRID = "Audit events";
const VERIFIED = "Configuration version verified";

/**
 * Text metrics the grid must not depend on. The CI runners have no Helvetica or SF, so the
 * browser draws DejaVu Sans, which is about 15% wider; Verdana has the same proportions and is
 * on every macOS, so the journeys run once as drawn and once with it.
 */
const WIDE = 'font-family: Verdana, "DejaVu Sans", sans-serif !important';
const METRICS = [
  ["as drawn", ""],
  ["a wide font", `*, *::before, *::after { ${WIDE}; }`],
] as const;

/** Draws the page with `css` over it; the audit grid is measured, not the font. */
async function useMetrics(page: Page, css: string): Promise<void> {
  if (css === "") return;
  await page.addInitScript((rules) => {
    const add = () => {
      const style = document.createElement("style");
      style.textContent = rules;
      document.head.append(style);
    };
    if (document.head) add();
    else document.addEventListener("DOMContentLoaded", add);
  }, css);
}

/** Whether `box` lies inside `outer` (a pixel of rounding allowed). */
function inside(
  box: { x: number; y: number; width: number; height: number },
  outer: { x: number; y: number; width: number; height: number },
): boolean {
  return (
    box.x >= outer.x - 1 &&
    box.x + box.width <= outer.x + outer.width + 1 &&
    box.y >= outer.y - 1 &&
    box.y + box.height <= outer.y + outer.height + 1
  );
}

async function boxOf(locator: Locator) {
  const box = await locator.boundingBox();
  if (box === null) throw new Error("the element has no box");
  return box;
}

/**
 * Outcome is on screen: its header and a cell are within the window and within the grid's own
 * visible area, and the grid does not scroll sideways.
 */
async function expectOutcomeVisible(page: Page): Promise<void> {
  const grid = page.getByRole("grid", { name: GRID });
  const viewport = page.viewportSize();
  if (viewport === null) throw new Error("no viewport");
  const window = { x: 0, y: 0, width: viewport.width, height: viewport.height };
  const header = grid.getByRole("columnheader", { name: "Outcome" });
  const cell = dataRows(page, GRID).first().getByRole("gridcell").last();
  await expect(header).toBeVisible();
  await expect(cell).toContainText(/Succeeded|Requested|Rejected|Failed/);
  const area = await boxOf(grid);
  for (const box of [await boxOf(header), await boxOf(cell)]) {
    expect(inside(box, window), "within the window").toBe(true);
    expect(inside({ ...box, y: area.y, height: 1 }, area), "within the grid").toBe(true);
  }
  // The badge is whole: not cut at the cell's end.
  const cut = await cell.evaluate((element) => element.scrollWidth - element.clientWidth);
  expect(cut, "Outcome badge not clipped").toBeLessThanOrEqual(0);
  const overflow = await grid.evaluate((element) => element.scrollWidth - element.clientWidth);
  expect(overflow, "no sideways scrolling").toBeLessThanOrEqual(1);
}

/** Every Action label drawn is whole: not cut at the end and not clipped to fewer lines. */
async function expectActionsWhole(page: Page): Promise<void> {
  const clipped = await page.evaluate((label) => {
    const grid = document.querySelector(`[role="grid"][aria-label="${label}"]`);
    if (!grid) return ["no grid"];
    const cells = grid.querySelectorAll('[role="row"] [role="gridcell"][aria-colindex]');
    const bad: string[] = [];
    for (const cell of cells) {
      const span = cell.querySelector("span[title]");
      if (!(span instanceof HTMLElement) || !span.title.startsWith("Configuration")) continue;
      if (span.scrollHeight > span.clientHeight + 1 || span.scrollWidth > span.clientWidth + 1) {
        bad.push(span.title);
      }
    }
    return bad;
  }, GRID);
  expect(clipped).toEqual([]);
}

async function headers(page: Page): Promise<string[]> {
  const names = await page.getByRole("grid", { name: GRID }).getByRole("columnheader").all();
  return Promise.all(names.map(async (header) => ((await header.textContent()) ?? "").trim()));
}

for (const [width, journey, [metrics, css]] of [
  [1280, "AG-01"],
  [1024, "AG-02"],
].flatMap(([width, journey]) => METRICS.map((m) => [width, journey, m] as const)) as [
  number,
  string,
  (typeof METRICS)[number],
][]) {
  const suffix = css === "" ? "" : ` (${metrics})`;
  test(`${journey}: at ${width} wide Outcome is on screen and Configuration version verified is whole${suffix}`, async ({
    page,
  }) => {
    await useMetrics(page, css);
    await page.setViewportSize({ width, height: 800 });
    await openAudit(page, "?action=config_verify");
    await expect(rowOfAction(page, VERIFIED)).toBeVisible();
    await expectOutcomeVisible(page);
    await expectActionsWhole(page);
    expect(await headers(page)).toEqual(
      expect.arrayContaining(["Time", "Actor", "Action", "Outcome"]),
    );
    expect(await headers(page)).not.toContain("Origin");
  });
}

for (const [width, journey, [metrics, css]] of [
  [1280, "AG-03"],
  [1024, "AG-04"],
].flatMap(([width, journey]) => METRICS.map((m) => [width, journey, m] as const)) as [
  number,
  string,
  (typeof METRICS)[number],
][]) {
  const suffix = css === "" ? "" : ` (${metrics})`;
  test(`${journey}: at ${width} wide with an event open Outcome stays on screen${suffix}`, async ({
    page,
  }) => {
    await useMetrics(page, css);
    await page.setViewportSize({ width, height: 800 });
    await openAudit(page, "?action=config_verify");
    await rowOfAction(page, VERIFIED).click();
    const panel = eventPanel(page);
    await expect(panel).toBeVisible();
    // Origin is out of the list but in the detail.
    await expect(panel.getByText("Origin", { exact: true })).toBeVisible();
    await expectOutcomeVisible(page);
    await expectActionsWhole(page);
    const shown = await headers(page);
    expect(shown).toEqual(expect.arrayContaining(["Time", "Action", "Outcome"]));
    expect(shown).not.toContain("Origin");
    // The list and the detail sit side by side, neither covering the other.
    const list = await boxOf(page.getByRole("grid", { name: GRID }));
    const side = await boxOf(panel);
    expect(list.x + list.width).toBeLessThanOrEqual(side.x + 1);
  });
}

test("AG-05: Origin is an optional column, offered by the Columns control", async ({ page }) => {
  await openAudit(page);
  const columns = page.getByRole("button", { name: /^Columns:/ });
  await expect(columns).toContainText("Default");
  await columns.click();
  const list = page.getByRole("dialog", { name: "Columns" });
  // Time, Action and Outcome always stay; the others can be chosen.
  await expect(list.getByRole("checkbox")).toHaveCount(3);
  for (const name of ["Actor", "Origin", "Object"]) {
    await expect(list.getByRole("checkbox", { name })).toBeVisible();
  }
  await tick(list, "Origin");
  await expect(columns).toContainText("Custom");
  expect(await headers(page)).toEqual(["Time", "Actor", "Origin", "Action", "Object", "Outcome"]);
  const origins = await page
    .getByRole("grid", { name: GRID })
    .locator('[role="gridcell"][aria-colindex="3"]')
    .allTextContents();
  expect(origins.length).toBeGreaterThan(0);
  expect(origins.every((origin) => /^(Console|Command line|Engine|Other)$/.test(origin))).toBe(
    true,
  );

  await list.getByRole("button", { name: "Default columns" }).click();
  expect(await headers(page)).not.toContain("Origin");
  await expect(columns).toContainText("Default");
});

for (const [query, journey, filtered] of [
  ["", "AG-06", false],
  ["?action=config_apply&outcome=succeeded", "AG-07", true],
] as const) {
  test(`${journey}: the export warns that the file may hold patient information${filtered ? ", with filters set" : ""}`, async ({
    page,
  }) => {
    await openAudit(page, query);
    await page.getByRole("button", { name: "Export…" }).click();
    const dialog = page.getByRole("dialog", { name: "Export audit events" });
    await expect(dialog).toContainText(filtered ? "Filters in effect" : "No filters are set");
    const warning = dialog.getByRole("alert");
    await expect(warning).toContainText("The file may contain patient information");
    await expect(warning).toContainText("patient identifiers");
    await expect(warning).toContainText("reasons and notes");
    await expect(warning).toContainText("lab's protected location");
    // The note that the export is itself recorded stays.
    await expect(dialog).toContainText("The export is recorded in the audit log");
  });
}
