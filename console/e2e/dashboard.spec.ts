import { expect, type Locator, type Page, test } from "@playwright/test";
import { CHANNELS, channelRow, escapeRegExp, openMock } from "./operate.support";
import { dataRows, loaded, navigateTo } from "./support";

const GRID = "Channels";

/** The words of a channel state chip, as the dashboard shows them (operator-console.md section 2). */
const STATE_WORDS = [
  "Running",
  "Starting",
  "Retrying",
  "Degraded",
  "Paused by operator",
  "Disabled",
  "Stopped",
] as const;
type StateWord = (typeof STATE_WORDS)[number];

interface RowFacts {
  label: string;
  state: StateWord;
  critical: boolean;
  unresolved: number;
}

/** Rank of a row under operator-console.md section 3 (running ranks 4 to 6 are told apart apart elsewhere). */
function rankOf(row: RowFacts): number {
  if (row.critical) return 1;
  switch (row.state) {
    case "Retrying":
      return 2;
    case "Degraded":
      return 3;
    case "Running":
      return 4;
    case "Starting":
      return 7;
    default:
      return 8;
  }
}

function stateWordOf(text: string): StateWord {
  const word = STATE_WORDS.find((candidate) => text.startsWith(candidate));
  if (word === undefined) throw new Error(`A state chip with no known state words: "${text}"`);
  return word;
}

/** Every dashboard row in display order, as the words it shows. */
async function readRows(page: Page): Promise<RowFacts[]> {
  const rows = dataRows(page, GRID);
  await expect(rows.first()).toBeVisible();
  const cells = await rows.evaluateAll((elements) =>
    elements.map((row) =>
      Array.from(row.querySelectorAll('[role="gridcell"]')).map((cell) => cell.textContent ?? ""),
    ),
  );
  return cells.map(([channel = "", state = "", ...rest]) => ({
    label: channel.replace(/\s(Bridge|Message)$/, "").trim(),
    state: stateWordOf(state.trim()),
    critical: state.includes("Critical alert"),
    unresolved: Number(/([\d,]+) unresolved/.exec(rest.join(" "))?.[1]?.replaceAll(",", "") ?? 0),
  }));
}

const indexOf = (rows: readonly RowFacts[], label: string) => {
  const index = rows.findIndex((row) => row.label === label);
  expect(index, `${label} is on the dashboard`).toBeGreaterThanOrEqual(0);
  return index;
};

/** The shape-and-words chips of a row's State cell (an icon directly inside a span). */
const chipsOf = (row: Locator) => row.getByRole("gridcell").nth(1).locator("span:has(> svg)");

const styleOf = (locator: Locator, property: "boxShadow" | "backgroundColor" | "color") =>
  locator.evaluate((element, name) => getComputedStyle(element)[name], property);

/** Opens the Alerts page and reads which channels have an open critical alert. */
async function criticalChannelLabels(page: Page): Promise<string[]> {
  await navigateTo(page, /^Alerts/);
  await loaded(page, "Alerts");
  const rows = dataRows(page, "Alerts").filter({
    has: page.getByRole("gridcell", { name: "Critical", exact: true }),
  });
  await expect(rows.first()).toBeVisible();
  const labels = await rows.evaluateAll((elements) =>
    elements.flatMap((row) =>
      Array.from(row.querySelectorAll('a[href^="#/channels/"]')).map((a) => a.textContent ?? ""),
    ),
  );
  await navigateTo(page, "Dashboard");
  await loaded(page, "Dashboard");
  return [...new Set(labels)];
}

test("H-02: rows follow the rank order, failing first and stopped or paused last", async ({
  page,
}) => {
  await openMock(page, "/dashboard");
  await loaded(page, "Dashboard");
  const rows = await readRows(page);

  // Rank order: critical, retrying, degraded, running, starting, stopped, paused, disabled.
  const ranks = rows.map(rankOf);
  expect(ranks, rows.map((row) => `${row.label} (${row.state})`).join("\n")).toEqual(
    [...ranks].sort((a, b) => a - b),
  );
  expect(new Set(ranks)).toEqual(new Set([1, 2, 3, 4, 7, 8]));

  // The named cases: retrying above degraded above a healthy running channel; the paused
  // channel after every active one.
  const retrying = indexOf(rows, CHANNELS.c503.label);
  const degraded = indexOf(rows, "Dashboard feed");
  const healthy = indexOf(rows, CHANNELS.urisys.label);
  const paused = indexOf(rows, CHANNELS.vitros350.label);
  expect(rows[retrying]?.state).toBe("Retrying");
  expect(rows[degraded]?.state).toBe("Degraded");
  expect(rows[healthy]?.state).toBe("Running");
  expect(rows[paused]?.state).toBe("Paused by operator");
  expect(retrying).toBeLessThan(degraded);
  expect(degraded).toBeLessThan(healthy);
  expect(healthy).toBeLessThan(paused);
  rows.forEach((row, index) => {
    if (["Running", "Retrying", "Degraded", "Starting"].includes(row.state)) {
      expect(index, `${row.label} is active, so above ${CHANNELS.vitros350.label}`).toBeLessThan(
        paused,
      );
    }
  });
  // A starting channel comes after the running ones.
  const starting = rows.findIndex((row) => row.state === "Starting");
  expect(starting).toBeGreaterThan(healthy);
  // A running channel with unresolved deliveries is above the quiet running ones.
  const running = rows.filter((row) => row.state === "Running" && !row.critical);
  const firstQuiet = running.findIndex((row) => row.unresolved === 0);
  expect(
    running.slice(firstQuiet < 0 ? running.length : firstQuiet).every((r) => r.unresolved === 0),
  ).toBe(true);
});

test("H-02: every state has its words and its own shape, never colour alone", async ({ page }) => {
  await openMock(page, "/dashboard");
  await loaded(page, "Dashboard");
  const rows = dataRows(page, GRID);
  await expect(rows.first()).toBeVisible();

  // Row by row: the state cell has chips; each has an icon and words that name the state.
  const count = await rows.count();
  const shapes = new Map<StateWord, string>();
  for (let index = 0; index < count; index++) {
    const chips = chipsOf(rows.nth(index));
    const first = chips.first();
    const words = (await first.innerText()).trim();
    const state = stateWordOf(words);
    await expect(first.locator("> svg")).toHaveCount(1);
    await expect(first.locator("> svg")).toHaveAttribute("aria-hidden", "true");
    const shape = await first.locator("> svg").innerHTML();
    expect(shapes.get(state) ?? shape, `every ${state} chip has the same shape`).toBe(shape);
    shapes.set(state, shape);
  }
  // The shapes differ from state to state.
  expect(new Set(shapes.values()).size).toBe(shapes.size);
  expect([...shapes.keys()].sort()).toEqual(
    [
      "Degraded",
      "Disabled",
      "Paused by operator",
      "Retrying",
      "Running",
      "Starting",
      "Stopped",
    ].sort(),
  );

  // The header strip's counts carry icon and words too.
  const counts = page.getByRole("list", { name: "Channels by state" }).getByRole("listitem");
  const total = await counts.count();
  expect(total).toBeGreaterThanOrEqual(4);
  for (let index = 0; index < total; index++) {
    await expect(counts.nth(index).locator("svg")).toHaveCount(1);
    await expect(counts.nth(index)).toHaveText(
      /^\d+ (Running|Starting|Retrying|Degraded|Stopped)$/,
    );
  }
});

test("H-02: a critical alert is in words and in the row, and the failing channels are first", async ({
  page,
}) => {
  await openMock(page, "/dashboard");
  await loaded(page, "Dashboard");
  const critical = await criticalChannelLabels(page);
  expect(critical).toEqual(expect.arrayContaining([CHANNELS.sta.label, CHANNELS.reflab.label]));

  const facts = await readRows(page);
  const withWords = facts.filter((row) => row.critical).map((row) => row.label);
  // The words "Critical alert" are on exactly the channels the Alerts page lists as critical.
  expect(withWords.sort()).toEqual([...critical].sort());
  // They are the first rows, together.
  expect(facts.slice(0, critical.length).every((row) => row.critical)).toBe(true);

  const rows = dataRows(page, GRID);
  const quiet = channelRow(page, GRID, CHANNELS.urisys.label);
  const quietShadow = await styleOf(quiet, "boxShadow");
  const quietBackground = await styleOf(quiet, "backgroundColor");
  expect(quietShadow).toBe("none");
  for (const label of critical) {
    const row = channelRow(page, GRID, label);
    await expect(row.getByText("Critical alert", { exact: true })).toBeVisible();
    // The row's own marking: a bar at its start and a tint, as no other row has.
    expect(await styleOf(row, "boxShadow")).not.toBe("none");
    expect(await styleOf(row, "backgroundColor")).not.toBe(quietBackground);
  }
  // A failing channel that is not critical has no such marking.
  const retrying = channelRow(page, GRID, CHANNELS.c503.label);
  await expect(retrying.getByText("Critical alert")).toHaveCount(0);
  expect(await styleOf(retrying, "boxShadow")).toBe("none");
  await expect(rows.first().getByText("Critical alert", { exact: true })).toBeVisible();
});

test("H-02: the header strip counts match the channels and the alerts", async ({ page }) => {
  await openMock(page, "/dashboard");
  await loaded(page, "Dashboard");
  const facts = await readRows(page);
  const strip = page.getByRole("region", { name: "Engine" });

  const expected = new Map<string, number>();
  for (const row of facts) {
    const key = ["Running", "Starting", "Retrying", "Degraded"].includes(row.state)
      ? row.state
      : "Stopped";
    expected.set(key, (expected.get(key) ?? 0) + 1);
  }
  const items = strip.getByRole("list", { name: "Channels by state" }).getByRole("listitem");
  await expect(items).toHaveCount(expected.size);
  for (const [state, count] of expected) {
    await expect(items.filter({ hasText: new RegExp(`^${count} ${state}$`) })).toHaveCount(1);
  }

  const unresolved = facts.reduce((sum, row) => sum + row.unresolved, 0);
  await expect(strip.getByText(`${unresolved} need attention`)).toBeVisible();

  // Alerts by severity: as many as the Alerts page lists.
  await navigateTo(page, /^Alerts/);
  await loaded(page, "Alerts");
  const alerts = dataRows(page, "Alerts");
  await expect(alerts.first()).toBeVisible();
  const critical = await alerts
    .filter({ has: page.getByRole("gridcell", { name: "Critical", exact: true }) })
    .count();
  const warning = await alerts
    .filter({ has: page.getByRole("gridcell", { name: "Warning", exact: true }) })
    .count();
  await navigateTo(page, "Dashboard");
  await loaded(page, "Dashboard");
  await expect(strip.getByText(`${critical} critical`, { exact: true })).toBeVisible();
  await expect(strip.getByText(`${warning} warning`, { exact: true })).toBeVisible();
});

test("H-02: a paused channel is neutral and reads as intentional, not as a failure", async ({
  page,
}) => {
  await openMock(page, "/dashboard");
  await loaded(page, "Dashboard");
  const paused = channelRow(page, GRID, CHANNELS.vitros350.label);
  const running = channelRow(page, GRID, CHANNELS.urisys.label);
  const retrying = channelRow(page, GRID, CHANNELS.c503.label);
  const disabled = channelRow(page, GRID, "Worklist export");

  await expect(paused).toBeVisible();
  const state = paused.getByRole("gridcell").nth(1);
  await expect(state).toHaveText("Paused by operator");
  await expect(paused.getByText("Critical alert")).toHaveCount(0);
  for (const failure of ["Error", "Retrying", "Degraded", "Failed"]) {
    await expect(state.getByText(failure)).toHaveCount(0);
  }
  // No failure marking on the row, and the chip is coloured as the other intentional state
  // (disabled) and not as a warning or a danger.
  expect(await styleOf(paused, "boxShadow")).toBe("none");
  expect(await styleOf(paused, "backgroundColor")).toBe(await styleOf(running, "backgroundColor"));
  const chipColor = (row: Locator) => styleOf(chipsOf(row).first(), "color");
  const pausedColor = await chipColor(paused);
  expect(pausedColor).toBe(await chipColor(disabled));
  expect(pausedColor).not.toBe(await chipColor(retrying));
  expect(pausedColor).not.toBe(
    await styleOf(
      channelRow(page, GRID, CHANNELS.sta.label).getByText("Critical alert", { exact: true }),
      "color",
    ),
  );
  // No last error either: the Last error column is not even needed to know why it is stopped.
  await expect(paused.getByText(new RegExp(escapeRegExp("connection refused")))).toHaveCount(0);
});

test("H-02: the first failing channel is visible without scrolling at 1280 x 800", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await openMock(page, "/dashboard");
  await loaded(page, "Dashboard");
  const first = dataRows(page, GRID).first();
  await expect(first).toBeVisible();
  await expect(first.getByText("Critical alert", { exact: true })).toBeVisible();
  await expect(first).toBeInViewport({ ratio: 1 });
  expect(await page.evaluate("window.scrollY")).toBe(0);
});

test("H-02: Channel, State and Endpoints always stay and other columns drop right to left", async ({
  page,
}) => {
  // Drop order, first dropped first (operator-console.md section 3).
  const dropOrder = ["Retry", "Last error", "Session", "Last activity", "Traffic", "Queue"];
  const byWidth = new Map<number, string[]>();
  for (const width of [1920, 1600, 1280, 1024, 800]) {
    // A fresh page per width: the columns are fitted to the window the page opens in.
    await page.setViewportSize({ width, height: 800 });
    await openMock(page, "/dashboard");
    await loaded(page, "Dashboard");
    await expect(dataRows(page, GRID).first()).toBeVisible();
    const shown = await page
      .getByRole("grid", { name: GRID })
      .getByRole("columnheader")
      .allInnerTexts();
    byWidth.set(width, shown);
    expect(shown.slice(0, 3), `at ${width}: Channel, State, Endpoints first`).toEqual([
      "Channel",
      "State",
      "Endpoints",
    ]);
    // Columns leave right to left: while one stays, every column that leaves after it stays.
    const present = dropOrder.map((name) => shown.includes(name));
    present.slice(0, -1).forEach((stays, index) => {
      if (stays) expect(present[index + 1], `at ${width}: ${shown.join(", ")}`).toBe(true);
    });
  }
  const counts = [...byWidth.values()].map((shown) => shown.length);
  expect(counts, "narrower windows never show more columns").toEqual(
    [...counts].sort((a, b) => b - a),
  );

  const at1280 = byWidth.get(1280) ?? [];
  const at1024 = byWidth.get(1024) ?? [];
  expect(at1280).toEqual(expect.arrayContaining(["Channel", "State", "Endpoints", "Traffic"]));
  expect(at1024.length).toBeLessThan(at1280.length);
  expect(at1024).toEqual(expect.arrayContaining(["Channel", "State", "Endpoints"]));
  for (const dropped of ["Retry", "Last error", "Session", "Last activity"]) {
    expect(at1024).not.toContain(dropped);
  }
  expect((byWidth.get(1920) ?? []).length).toBeGreaterThan(at1280.length);
  expect(byWidth.get(800)).toEqual(["Channel", "State", "Endpoints"]);
});
