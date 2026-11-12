import { expect, type Locator, type Page, test } from "@playwright/test";
import { APP_PAGES, expectNotBlank, visit } from "./states.support";
import { dataRows, heading } from "./support";

/** The pages that show a loading state of their own. */
const LOADING_PAGES = APP_PAGES.filter((app) => app.loading !== null);

function statusText(page: Page, text: string): Locator {
  return page.getByRole("status").filter({ hasText: text });
}

// H-08 (a): loading ---------------------------------------------------------------------------

test.describe("H-08 loading: every page shows its heading and a loading state, then its content", () => {
  for (const app of LOADING_PAGES) {
    test(`H-08 loading: ${app.name}`, async ({ page }) => {
      await visit(page, app.route, { scenario: "slow" });
      await expect(heading(page, app.genericHeading)).toBeVisible();
      await expect(statusText(page, app.loading ?? "")).toBeVisible();
      await expectNotBlank(page);
      await expect(heading(page, app.heading)).toBeVisible();
      await expect(statusText(page, app.loading ?? "")).toHaveCount(0);
      await expect(page.getByText(/^Loading /)).toHaveCount(0);
      await expectNotBlank(page);
    });
  }

  // NOTE: Dashboard, Channels and Endpoints follow the pushed snapshot, which `scenario=slow`
  // does not delay, so their "Loading …" state cannot be reached with the mock. The strongest
  // assertion is that they are never blank: heading and content are there at once.
  for (const app of APP_PAGES.filter((each) => each.loading === null && each.error === null)) {
    test(`H-08 loading: ${app.name} is never blank while requests are slow`, async ({ page }) => {
      await visit(page, app.route, { scenario: "slow" });
      await expect(heading(page, app.heading)).toBeVisible();
      await expectNotBlank(page);
    });
  }

  test("H-08 loading: a version of Settings Versions says it is loading, then shows it", async ({
    page,
  }) => {
    await visit(page, "/settings/versions?version=46", { scenario: "slow" });
    await expect(page.getByRole("grid", { name: "Configuration versions" })).toBeVisible();
    await expect(statusText(page, "Loading version 46…")).toBeVisible();
    await expect(page.getByRole("region", { name: "Version 46" })).toBeVisible();
    await expect(
      page.getByRole("heading", { name: "Configuration in this version (read-only)" }),
    ).toBeVisible();
    await expect(page.getByText(/^Loading /)).toHaveCount(0);
  });
});

// H-08 (b): empty and no results --------------------------------------------------------------

interface EmptyCase {
  name: string;
  route: string;
  title: string;
  description: RegExp;
  /** The control that is the next action, and where it leads. */
  action: { role: "link" | "button"; name: string; opens?: string };
}

const EMPTY_CASES: readonly EmptyCase[] = [
  {
    name: "Dashboard",
    route: "/dashboard",
    title: "No channels yet",
    description: /Add one to start moving data/,
    action: { role: "link", name: "Add channel", opens: "New channel" },
  },
  {
    name: "Channels",
    route: "/channels",
    title: "No channels yet",
    description: /A channel connects an instrument or system to another/,
    action: { role: "link", name: "Add channel", opens: "New channel" },
  },
  {
    name: "Endpoints",
    route: "/endpoints",
    title: "No endpoints yet",
    description: /An endpoint is one side of a connection/,
    action: { role: "link", name: "Add endpoint", opens: "New endpoint" },
  },
  {
    name: "Messages",
    route: "/messages",
    title: "No messages yet",
    description: /Messages appear here as the message channels receive them/,
    action: { role: "button", name: "Refresh" },
  },
  {
    name: "Alerts",
    route: "/alerts",
    title: "No open alerts",
    description: /An alert appears here with the next step when a condition needs someone/,
    action: { role: "button", name: "Refresh" },
  },
  {
    name: "Settings Retention",
    route: "/settings/retention",
    title: "No message channels yet",
    description: /Retention applies to message channels/,
    action: { role: "link", name: "Settings", opens: "Settings" },
  },
];

test.describe("H-08 empty: a fresh install says so in words, with the next action", () => {
  for (const empty of EMPTY_CASES) {
    test(`H-08 empty: ${empty.name}`, async ({ page }) => {
      await visit(page, empty.route, { scenario: "empty" });
      await expect(page.getByText(empty.title, { exact: true })).toBeVisible();
      await expect(page.getByText(empty.description)).toBeVisible();
      await expectNotBlank(page);
      // The next step is on the page; the ones that lead somewhere are followed.
      const action = page.getByRole(empty.action.role, { name: empty.action.name }).first();
      await expect(action).toBeVisible();
      if (
        empty.action.opens &&
        empty.action.role === "link" &&
        empty.route !== "/settings/retention"
      ) {
        await action.click();
        await expect(heading(page, empty.action.opens)).toBeVisible();
      }
    });
  }

  test("H-08 empty: the dashboard of a fresh install also offers the first endpoint and says no alerts", async ({
    page,
  }) => {
    await visit(page, "/dashboard", { scenario: "empty" });
    await expect(page.getByText("No open alerts.")).toBeVisible();
    await expect(page.getByText("No endpoints yet.")).toBeVisible();
    await expect(page.getByRole("link", { name: "Add endpoint" })).toBeVisible();
    await expect(page.getByRole("link", { name: "Add channel" })).toBeVisible();
  });

  test("H-08 empty: Messages tells a fresh install from a filter that matches nothing", async ({
    page,
  }) => {
    await visit(page, "/messages", { scenario: "empty" });
    await expect(page.getByText("No messages yet")).toBeVisible();
    await page.getByRole("searchbox", { name: "Identifier" }).fill("zzzz");
    await page.keyboard.press("Enter");
    await expect(page.getByText("No messages match", { exact: true })).toBeVisible();
    await expect(page.getByText("No messages yet")).toHaveCount(0);
    await page.getByRole("button", { name: "Clear filters" }).last().click();
    await expect(page.getByText("No messages yet")).toBeVisible();
  });

  test("H-08 empty: Secrets says there are none, after the last one is deleted", async ({
    page,
  }) => {
    // NOTE: `scenario=empty` keeps five seeded secrets (and the configuration history of
    // Settings > Versions), so the empty state is reached by deleting them through the dialog.
    await visit(page, "/settings/secrets", { scenario: "empty" });
    const grid = page.getByRole("grid", { name: "Secrets" });
    await expect(grid).toBeVisible();
    while ((await dataRows(page, "Secrets").count()) > 0) {
      const before = await dataRows(page, "Secrets").count();
      await grid
        .getByRole("button", { name: /^Actions for / })
        .first()
        .click();
      await page.getByRole("menuitem", { name: "Delete secret…" }).click();
      await page.getByRole("alertdialog").getByRole("button", { name: "Delete secret" }).click();
      await expect(page.getByRole("alertdialog")).toHaveCount(0);
      await expect(dataRows(page, "Secrets")).toHaveCount(before - 1);
    }
    await expect(page.getByText("No secrets yet")).toBeVisible();
    await expect(
      page.getByText(/Add a secret for an endpoint that needs a password or key/),
    ).toBeVisible();
    await expect(page.getByRole("button", { name: "Add secret" })).toBeVisible();
  });

  test("H-08 empty: Versions and Logs are never empty with the mock", async ({ page }) => {
    // NOTE: not provable. `scenario=empty` still has a version history ("Version 47 ... edited
    // channel Dashboard feed") and heartbeat log events, so "No versions yet" and an empty log
    // cannot be reached. This test records that observation.
    await visit(page, "/settings/versions", { scenario: "empty" });
    await expect(page.getByRole("grid", { name: "Configuration versions" })).toBeVisible();
    await visit(page, "/logs", { scenario: "empty" });
    await expect(page.getByRole("grid", { name: "Log events" })).toBeVisible();
  });
});

interface SearchCase {
  name: string;
  route: string;
  search: string;
  field: string;
  /** The grid that is replaced by the no-results state. */
  grid: string;
  noun: string;
}

const SEARCH_CASES: readonly SearchCase[] = [
  {
    name: "Channels",
    route: "/channels",
    search: "Find channels",
    field: "zzzz",
    grid: "Channels",
    noun: "channels",
  },
  {
    name: "Endpoints",
    route: "/endpoints",
    search: "Find endpoints",
    field: "zzzz",
    grid: "Endpoints",
    noun: "endpoints",
  },
  {
    name: "Alerts",
    route: "/alerts",
    search: "Find alerts",
    field: "zzzz",
    grid: "Alerts",
    noun: "alerts",
  },
  {
    name: "Logs",
    route: "/logs",
    search: "Search log messages",
    field: "zzzz",
    grid: "Log events",
    noun: "events",
  },
];

test.describe("H-08 no results: a search that excludes everything says what was searched and offers Clear", () => {
  for (const found of SEARCH_CASES) {
    test(`H-08 no results: ${found.name}`, async ({ page }) => {
      await visit(page, found.route);
      await expect(heading(page, found.name)).toBeVisible();
      await expect(page.getByRole("grid", { name: found.grid })).toBeVisible();
      const box = page.getByRole("searchbox", { name: found.search });
      await box.fill(found.field);
      await expect(page.getByText(`No ${found.noun} match 'zzzz'`)).toBeVisible();
      await expect(page.getByRole("grid", { name: found.grid })).toHaveCount(0);
      await page.getByRole("button", { name: "Clear search" }).last().click();
      await expect(box).toHaveValue("");
      await expect(page.getByRole("grid", { name: found.grid })).toBeVisible();
    });
  }

  test("H-08 no results: Messages says nothing matches the filters and offers Clear filters", async ({
    page,
  }) => {
    await visit(page, "/messages");
    await expect(page.getByRole("grid", { name: "Messages" })).toBeVisible();
    await page.getByRole("searchbox", { name: "Identifier" }).fill("zzzz");
    await page.keyboard.press("Enter");
    await expect(page.getByText("No messages match", { exact: true })).toBeVisible();
    // NOTE: unlike the other lists, the sentence does not repeat what was searched ("zzzz");
    // the search box above still shows it.
    await expect(
      page.getByText("No stored message in the selected time range matches these filters."),
    ).toBeVisible();
    await page.getByRole("button", { name: "Clear filters" }).last().click();
    await expect(page.getByRole("searchbox", { name: "Identifier" })).toHaveValue("");
    await expect(page.getByRole("grid", { name: "Messages" })).toBeVisible();
  });
});
