import type { Locator, Page } from "@playwright/test";

/**
 * Every row the grid called `label` holds, as the text of its cells, in grid order. The grid draws
 * only the rows in view, so this scrolls through it (and back to the top at the end). Scrolling
 * away from the newest row pauses following: use it on a list that is not followed or already
 * paused.
 */
export async function readGridRows(page: Page, label: string): Promise<string[][]> {
  return page.evaluate(async (name) => {
    const grid = document.querySelector(`[role="grid"][aria-label="${name}"]`);
    if (!(grid instanceof HTMLElement)) return [];
    const seen = new Map<number, string[]>();
    const read = () => {
      for (const row of grid.querySelectorAll('[role="row"]')) {
        const cells = [...row.querySelectorAll('[role="gridcell"]')];
        if (cells.length === 0) continue;
        seen.set(
          Number(row.getAttribute("aria-rowindex")),
          cells.map((cell) => cell.textContent ?? ""),
        );
      }
    };
    const settle = () => new Promise((resolve) => setTimeout(resolve, 40));
    grid.scrollTop = 0;
    await settle();
    for (let guard = 0; guard < 1000; guard++) {
      read();
      if (grid.scrollTop + grid.clientHeight >= grid.scrollHeight - 1) break;
      grid.scrollTop += grid.clientHeight * 0.8;
      await settle();
    }
    grid.scrollTop = 0;
    await settle();
    return [...seen.entries()].sort(([a], [b]) => a - b).map(([, cells]) => cells);
  }, label);
}

/** The reader of one column, by name, of rows from `readGridRows`: `column(rows, "time")`. */
export function columnReader<Columns extends Record<string, number>>(columns: Columns) {
  return (rows: readonly string[][], which: keyof Columns): string[] =>
    rows.map((row) => row[columns[which] as number] ?? "");
}

/** The number a count line starts with: `pattern` captures it, thousands separators allowed. */
export async function countFrom(line: Locator, pattern: RegExp): Promise<number> {
  const text = ((await line.textContent()) ?? "").trim();
  const match = pattern.exec(text);
  if (!match?.[1]) throw new Error(`no count in "${text}"`);
  return Number(match[1].replaceAll(",", ""));
}
