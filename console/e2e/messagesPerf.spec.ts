import { expect, type Page, test } from "@playwright/test";
import {
  messageGrid,
  messageRows,
  openMoreFilters,
  paging,
  quick,
  rangeStatus,
  settled,
  tick,
} from "./messages.support";
import { choose, loaded, open } from "./support";

declare const process: {
  env: Record<string, string | undefined>;
  stdout: { write(text: string): void };
};

// I-03, the console half (PRF-03): with 500,000 stored messages the Messages list stays
// responsive. The mock answers like the engine would (server-side filter, sort and page, after
// 80 to 300 ms of simulated latency) and computes on the page's own thread, in steps with the
// page's other work between them (src/api/mock/steps.ts), so what is measured here is the console
// plus the mock's work, the worst case a real engine would not add to. The longest task is the
// console's own: drawing a page of rows.

/**
 * How many times slower than this computer the run is made (`PACKIE_E2E_CPU_THROTTLE=4`, the way a
 * two-core CI runner is slow). The budgets of time scale with it; the longest task does not.
 */
const SLOWDOWN = Math.max(1, Number(process.env.PACKIE_E2E_CPU_THROTTLE ?? 1) || 1);

/** Generous budgets that still catch a regression (milliseconds). */
const BUDGET = {
  firstRows: 3000 * SLOWDOWN,
  action: 1000 * SLOWDOWN,
  /** A filter change over the whole history in the mock, which scans it in steps on the page's thread. */
  scan: 2000 * SLOWDOWN,
  route: 3000 * SLOWDOWN,
  /** No page task may hold the page for longer, however slow the computer. */
  longTask: 500,
};

interface PageClock {
  __longTasks: number[];
  __t0: number;
  /** What the list shows now: the count line and the first row, which any new answer changes. */
  __signature: () => string;
}

/**
 * `PACKIE_E2E_CPU_THROTTLE=4` runs the journey on a computer that many times slower, the way a
 * two-core CI runner is: the long-task budget is checked where work takes longer.
 */
async function throttle(page: Page): Promise<void> {
  if (SLOWDOWN <= 1) return;
  const session = await page.context().newCDPSession(page);
  await session.send("Emulation.setCPUThrottlingRate", { rate: SLOWDOWN });
}

/** Records every long task of the page, the time of every click and the list's signature. */
async function installClock(page: Page): Promise<void> {
  await throttle(page);
  await page.addInitScript(() => {
    const clock = window as unknown as PageClock;
    clock.__longTasks = [];
    clock.__t0 = 0;
    clock.__signature = () => {
      const status = document.querySelector('nav[aria-label="Message pages"] [role="status"]');
      const first = document.querySelector(
        '[role="grid"] [role="row"][aria-rowindex]:nth-child(2)',
      );
      return `${status?.textContent ?? ""}|${first?.textContent ?? ""}`;
    };
    new PerformanceObserver((list) => {
      for (const entry of list.getEntries()) clock.__longTasks.push(entry.duration);
    }).observe({ type: "longtask", buffered: true });
    // The click's own time, so the Playwright round trip is not part of the measurement.
    document.addEventListener(
      "click",
      () => {
        clock.__t0 = performance.now();
      },
      true,
    );
  });
}

/**
 * Runs `act` and returns the milliseconds from its click until the list shows a new answer: the
 * count line or first row differ, and the grid is no longer marked busy.
 */
async function timed(page: Page, act: () => Promise<void>): Promise<number> {
  const before = await page.evaluate(() => (window as unknown as PageClock).__signature());
  await page.evaluate(() => {
    (window as unknown as PageClock).__t0 = performance.now();
  });
  await act();
  const done = await page.waitForFunction(
    (previous) => {
      const grid = document.querySelector('[role="grid"]');
      const now = (window as unknown as PageClock).__signature();
      const busy = grid?.getAttribute("aria-busy") === "true" || now.includes("Updating");
      return !busy && now !== previous ? performance.now() : false;
    },
    before,
    { timeout: 30_000 },
  );
  const end = (await done.jsonValue()) as number;
  const start = await page.evaluate(() => (window as unknown as PageClock).__t0);
  return Math.round(end - start);
}

/** Records a timing in the test report, so CI logs show it, and checks it against its budget. */
function record(label: string, ms: number, budget: number): void {
  test
    .info()
    .annotations.push({ type: "timing", description: `${label}: ${ms} ms (budget ${budget} ms)` });
  expect(ms, `${label} took ${ms} ms`).toBeLessThan(budget);
}

/** The grid's frame intervals while it is scrolled to the bottom and back, a step per frame. */
async function scrollFrames(page: Page): Promise<number[]> {
  return page.evaluate(
    () =>
      new Promise<number[]>((done) => {
        const grid = document.querySelector('[role="grid"]');
        if (!(grid instanceof HTMLElement)) throw new Error("no grid");
        const max = grid.scrollHeight - grid.clientHeight;
        const steps = 40;
        const frames: number[] = [];
        let last = performance.now();
        let step = 0;
        const frame = () => {
          const now = performance.now();
          frames.push(now - last);
          last = now;
          step += 1;
          grid.scrollTop = max * (step <= steps ? step / steps : 2 - step / steps);
          if (step < steps * 2) requestAnimationFrame(frame);
          else done(frames);
        };
        requestAnimationFrame(frame);
      }),
  );
}

/** A quick filter, a channel and a direction, each turned on and off again. */
async function filterRound(page: Page, where: string, budget: number): Promise<void> {
  record(
    `quick filter on ${where}`,
    await timed(page, () => quick(page, "Needs attention").click()),
    budget,
  );
  record(
    `quick filter off ${where}`,
    await timed(page, () => quick(page, "Needs attention").click()),
    budget,
  );
  await page.getByRole("button", { name: /^Channels:/ }).click();
  const channels = page.getByRole("dialog", { name: "Channels" });
  record(
    `channel filter on ${where}`,
    await timed(page, () => tick(channels, "AU680 results")),
    budget,
  );
  record(
    `channel filter off ${where}`,
    await timed(page, () => tick(channels, "AU680 results", false)),
    budget,
  );
  await page.keyboard.press("Escape");
  await openMoreFilters(page);
  record(
    `direction filter on ${where}`,
    await timed(page, () => choose(page, "Direction", "Side A to side B")),
    budget,
  );
  record(
    `direction filter off ${where}`,
    await timed(page, () => choose(page, "Direction", "Any direction")),
    budget,
  );
}

const median = (values: number[]): number =>
  [...values].sort((a, b) => a - b)[Math.floor(values.length / 2)] ?? 0;

test("I-03: with 500,000 messages the Messages list stays responsive", { tag: "@perf" }, async ({
  page,
}) => {
  test.setTimeout(180_000);
  await installClock(page);

  await test.step("the first page is visible and interactive", async () => {
    await open(page, "/messages", { messages: 500_000 });
    const handle = await page.waitForFunction(() =>
      document.querySelectorAll('[role="grid"] [role="gridcell"]').length > 0
        ? performance.now()
        : false,
    );
    record(
      "first rows visible after navigation",
      Math.round((await handle.jsonValue()) as number),
      BUDGET.firstRows,
    );
    await expect(rangeStatus(page)).toHaveText(/^Rows 1–100 of [\d,]+$/);
    await settled(page);
    // At the default 24 hours: about a day of the history, not all of it.
    expect(await messageRows(page).count()).toBe(100);
  });

  await test.step("paging and every filter in the default range", async () => {
    record(
      "Next page (24 hours)",
      await timed(page, () => paging(page).getByRole("button", { name: "Next" }).click()),
      BUDGET.action,
    );
    await expect(rangeStatus(page)).toHaveText(/^Rows 101–200 of /);
    record(
      "Previous page (24 hours)",
      await timed(page, () => paging(page).getByRole("button", { name: "Previous" }).click()),
      BUDGET.action,
    );
    await expect(rangeStatus(page)).toHaveText(/^Rows 1–100 of /);
    await filterRound(page, "(24 hours)", BUDGET.action);
  });

  await test.step("any time shows the whole history: 500,000", async () => {
    record(
      "time range to Any time",
      await timed(page, () => choose(page, "Time range", "Any time")),
      BUDGET.scan,
    );
    await expect(rangeStatus(page)).toHaveText(/^Rows 1–100 of 500,000$/);
  });

  await test.step("paging, sorting and rows per page over 500,000 messages", async () => {
    record(
      "Next page",
      await timed(page, () => paging(page).getByRole("button", { name: "Next" }).click()),
      BUDGET.action,
    );
    await expect(rangeStatus(page)).toHaveText(/^Rows 101–200 of 500,000$/);
    record(
      "Previous page",
      await timed(page, () => paging(page).getByRole("button", { name: "Previous" }).click()),
      BUDGET.action,
    );
    record(
      "Next page again",
      await timed(page, () => paging(page).getByRole("button", { name: "Next" }).click()),
      BUDGET.action,
    );
    record(
      "First page",
      await timed(page, () => paging(page).getByRole("button", { name: "First" }).click()),
      BUDGET.action,
    );
    record(
      "sort by a column",
      await timed(page, () =>
        messageGrid(page).getByRole("columnheader", { name: "Message state" }).click(),
      ),
      BUDGET.action,
    );
    record(
      "sort again",
      await timed(page, () =>
        messageGrid(page).getByRole("columnheader", { name: "Message state" }).click(),
      ),
      BUDGET.action,
    );
    record(
      "rows per page 50",
      await timed(page, () => choose(page, "Rows per page", "50")),
      BUDGET.action,
    );
    await expect(messageRows(page)).toHaveCount(50);
    record(
      "rows per page 200",
      await timed(page, () => choose(page, "Rows per page", "200")),
      BUDGET.action,
    );
  });

  await test.step("every filter over the whole history", async () => {
    // Each of these makes the mock scan all 500,000 messages again, for the list and for the
    // four quick-filter counts, on the page's own thread: a longer budget than the engine needs.
    await filterRound(page, "(Any time)", BUDGET.scan);
  });

  await test.step("a long page is drawn as a window, and scrolling it stays smooth", async () => {
    await expect(rangeStatus(page)).toHaveText(/^Rows 1–200 of 500,000$/);
    await settled(page);
    // 200 rows are on the page; only the ones in view (and a few around them) are in the document.
    const drawn = await messageRows(page).count();
    test.info().annotations.push({ type: "timing", description: `rows drawn of 200: ${drawn}` });
    expect(drawn).toBeGreaterThan(10);
    expect(drawn).toBeLessThan(100);
    const frames = (await scrollFrames(page)).slice(1);
    const slowest = Math.round(Math.max(...frames));
    test.info().annotations.push({
      type: "timing",
      description: `scroll to the bottom and back, ${frames.length} frames: median ${Math.round(median(frames))} ms, slowest ${slowest} ms`,
    });
    expect(slowest, "slowest frame while scrolling").toBeLessThan(300 * SLOWDOWN);
    expect(median(frames), "median frame while scrolling").toBeLessThan(50 * SLOWDOWN);
    // Back at the top the first rows are drawn again; at the bottom the last one was.
    await expect(page.locator('[role="row"][aria-rowindex="2"]')).toBeVisible();
    await messageGrid(page).evaluate((grid) => grid.scrollTo(0, grid.scrollHeight));
    await expect(page.locator('[role="row"][aria-rowindex="201"]')).toBeVisible();
    expect(await messageRows(page).count()).toBeLessThan(100);
    await messageGrid(page).evaluate((grid) => grid.scrollTo(0, 0));
    // The default page of 100 is drawn whole (the grid windows only above 100).
    await choose(page, "Rows per page", "100");
    await expect(rangeStatus(page)).toHaveText(/^Rows 1–100 of 500,000$/);
    await settled(page);
    expect(await messageRows(page).count()).toBeLessThanOrEqual(100);
  });

  await test.step("the Dashboard and Logs still open normally in the same session", async () => {
    const menu = page.getByRole("navigation", { name: "Main" });
    for (const name of ["Dashboard", "Logs"]) {
      const started = Date.now();
      await menu.getByRole("link", { name }).click();
      await loaded(page, name);
      record(`${name} opens`, Date.now() - started, BUDGET.route);
    }
    const started = Date.now();
    await menu.getByRole("link", { name: "Messages" }).click();
    await expect(messageRows(page).first()).toBeVisible();
    await settled(page);
    record("Messages opens again", Date.now() - started, BUDGET.route);
    // The menu's link opens the list as a visit starts (24 hours); the whole history is one choice away.
    await expect(rangeStatus(page)).toHaveText(/^Rows 1–100 of [\d,]+$/);
    record(
      "time range to Any time again",
      await timed(page, () => choose(page, "Time range", "Any time")),
      BUDGET.scan,
    );
    await expect(rangeStatus(page)).toHaveText(/of 500,000$/);
  });

  await test.step("the page never froze: no task over 500 ms", async () => {
    // The page answers a frame now, and its longest task of the whole journey was short.
    await page.evaluate(() => new Promise((done) => requestAnimationFrame(done)));
    const tasks = await page.evaluate(() => (window as unknown as PageClock).__longTasks);
    const longest = Math.round(Math.max(0, ...tasks));
    const summary = `long tasks (over 50 ms): ${tasks.length}, longest ${longest} ms (budget ${BUDGET.longTask} ms)`;
    test.info().annotations.push({ type: "timing", description: summary });
    process.stdout.write(`${summary}\n`);
    expect(longest, "longest task of the journey").toBeLessThan(BUDGET.longTask);
  });
});

test("I-03: a page deep in 500,000 messages opens in time", { tag: "@perf" }, async ({ page }) => {
  await installClock(page);
  await open(page, "/messages?range=any", { messages: 500_000 });
  await expect(rangeStatus(page)).toHaveText(/^Rows 1–100 of 500,000$/);
  await settled(page);
  // The last page: offset 499,900.
  const ms = await timed(page, () =>
    page.evaluate(() => {
      window.location.hash = "#/messages?range=any&page=5000";
    }),
  );
  await expect(rangeStatus(page)).toHaveText(/^Rows 499,901–500,000 of 500,000$/);
  await expect(messageRows(page)).toHaveCount(100);
  record("jump to the last page", ms, BUDGET.action);
});
