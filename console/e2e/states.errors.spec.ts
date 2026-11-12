import { expect, type Locator, type Page, test } from "@playwright/test";
import {
  APP_PAGES,
  expectNotBlank,
  helpButton,
  helpPanel,
  MESSAGE_CHANNEL,
  visit,
} from "./states.support";
import { heading, navigateTo } from "./support";

const FAILURE_REASON = "The mock engine was asked to fail every request.";

/** The pages that read their own data, which is where the error state can be reached. */
const FAILING_PAGES = APP_PAGES.filter((app) => app.error !== null);

// H-08 (c): errors ----------------------------------------------------------------------------

async function tryAgainButtons(page: Page): Promise<Locator> {
  const buttons = page.getByRole("alert").getByRole("button", { name: "Try again" });
  await expect(buttons.first()).toBeVisible();
  return buttons;
}

/** The "Support reference: c-…" lines on the page, in order; each request gets a new one. */
async function supportReferences(page: Page): Promise<string[]> {
  return page.getByText(/^Support reference: c-/).allInnerTexts();
}

test.describe("H-08 error: every data page says why it failed and offers Try again", () => {
  for (const app of FAILING_PAGES) {
    test(`H-08 error: ${app.name} shows the reason and Try again, keeps its heading`, async ({
      page,
    }) => {
      await visit(page, app.route, { scenario: "errors" });
      await expect(heading(page, app.genericHeading)).toBeVisible();
      const alert = page.getByRole("alert").filter({ hasText: app.error ?? "" });
      await expect(alert).toBeVisible();
      await expect(alert).toContainText(FAILURE_REASON);
      await expect(alert).toContainText(/Support reference: c-/);
      await expect(alert.getByRole("button", { name: "Try again" })).toBeVisible();
      await expectNotBlank(page);
      await expect(page.getByText(/^Loading /)).toHaveCount(0);
    });

    test(`H-08 error: ${app.name} keeps navigation, the top bar and Help working`, async ({
      page,
    }) => {
      await visit(page, app.route, { scenario: "errors" });
      await expect(page.getByRole("alert").first()).toBeVisible();
      await expect(page.getByRole("banner")).toContainText("PACK-IE Console");
      await expect(page.getByRole("navigation", { name: "Main" })).toBeVisible();
      await helpButton(page).click();
      await expect(
        helpPanel(page).getByRole("article").getByRole("heading", { level: 3 }).first(),
      ).toHaveText(app.topic);
      await page.keyboard.press("Escape");
      await expect(helpPanel(page)).toBeHidden();
      await expect(helpButton(page)).toBeFocused();
      await navigateTo(page, "Channels");
      await expect(heading(page, "Channels")).toBeVisible();
      await expect(page.getByRole("grid", { name: "Channels" })).toBeVisible();
    });
  }

  test("H-08 error: Try again asks the engine again (a new support reference appears)", async ({
    page,
  }) => {
    // NOTE: with `scenario=errors` the retry fails again and the page shows no pending state
    // (no "Loading …", the button is not marked pending), so the request is proven by the
    // mock's support reference, which is new for every request.
    for (const app of FAILING_PAGES) {
      await visit(page, app.route, { scenario: "errors" });
      const buttons = await tryAgainButtons(page);
      const [first = ""] = await supportReferences(page);
      await buttons.first().click();
      await expect
        .poll(async () => (await supportReferences(page))[0], { message: app.name })
        .not.toBe(first);
      await expect(page.getByRole("alert").first()).toBeVisible();
    }
  });

  test("H-08 error: Dashboard, Channels and Endpoints keep showing the last snapshot when reads fail", async ({
    page,
  }) => {
    // NOTE: these pages follow the engine's pushed snapshot, so `scenario=errors` does not
    // produce their error state; they must at least show data, never a blank page.
    const grids = { Dashboard: "Channels", Channels: "Channels", Endpoints: "Endpoints" };
    for (const [name, grid] of Object.entries(grids)) {
      const app = APP_PAGES.find((each) => each.name === name);
      expect(app).toBeDefined();
      await visit(page, app?.route ?? "/", { scenario: "errors" });
      await expect(heading(page, name)).toBeVisible();
      await expect(page.getByRole("grid", { name: grid })).toBeVisible();
    }
  });

  test("H-08 error: a command that fails names the channel and the operation", async ({ page }) => {
    await visit(page, "/channels", { scenario: "errors" });
    await page.getByRole("button", { name: `Actions for ${MESSAGE_CHANNEL.label}` }).click();
    await page.getByRole("menuitem", { name: "Pause" }).click();
    await page.getByRole("alertdialog").getByRole("button", { name: "Pause channel" }).click();
    const toast = page.getByRole("region", { name: "Notifications" });
    await expect(toast).toContainText(`Could not pause ${MESSAGE_CHANNEL.label}`);
    await expect(toast).toContainText(FAILURE_REASON);
    await expect(page.getByRole("alertdialog")).toHaveCount(0);
  });

  test("H-08 error: the engine gate names what it tried and offers Start the service", async ({
    page,
  }) => {
    await visit(page, "/dashboard", { offline: true });
    await expect(page.getByText("The engine is not running")).toBeVisible();
    await expect(page.getByText(/No engine answered at mock:\/\/engine\./)).toBeVisible();
    await expect(page.getByRole("button", { name: "Start the service" })).toBeVisible();
    await expect(page.locator("code")).toContainText("service start");
  });
});

interface MissingCase {
  name: string;
  route: string;
  title: string;
  /** The `h1` of the not-found page; `null` where the page has none (a defect, see below). */
  heading: string | null;
  back: { role: "link" | "button"; name: string };
  list: string;
}

const MISSING_CASES: readonly MissingCase[] = [
  {
    name: "channel",
    route: "/channels/nope",
    title: "This channel is not in the configuration",
    heading: "Channel not found",
    back: { role: "link", name: "Back to channels" },
    list: "Channels",
  },
  {
    name: "endpoint",
    route: "/endpoints/nope",
    title: "This endpoint is not in the configuration",
    heading: "Endpoint not found",
    back: { role: "link", name: "Back to endpoints" },
    list: "Endpoints",
  },
  {
    name: "message",
    route: "/messages/nope",
    title: "No stored message has this ID",
    heading: "Message not found",
    back: { role: "button", name: "Back to messages" },
    list: "Messages",
  },
  {
    name: "channel editor",
    route: "/channels/nope/edit",
    title: "This channel was not found",
    heading: null,
    back: { role: "link", name: "Back to channels" },
    list: "Channels",
  },
  {
    name: "endpoint editor",
    route: "/endpoints/nope/edit",
    title: "This endpoint was not found",
    heading: null,
    back: { role: "link", name: "Back to endpoints" },
    list: "Endpoints",
  },
];

test.describe("H-08 not found: a record that does not exist says so and links back to the list", () => {
  for (const missing of MISSING_CASES) {
    test(`H-08 not found: ${missing.name}`, async ({ page }) => {
      await visit(page, missing.route);
      await expect(page.getByText(missing.title)).toBeVisible();
      if (missing.heading) await expect(heading(page, missing.heading)).toBeVisible();
      await expectNotBlank(page);
      await expect(page.getByRole("alert")).toHaveCount(0);
      await page.getByRole(missing.back.role, { name: missing.back.name }).click();
      await expect(heading(page, missing.list)).toBeVisible();
      await expect(page).toHaveURL(new RegExp(`#/${missing.list.toLowerCase()}$`));
    });
  }
});

test.describe("H-08 not found: the editors keep a page heading", () => {
  // An unknown record on an edit URL keeps the page's one `h1` and a breadcrumb to the list.
  for (const route of ["/channels/nope/edit", "/endpoints/nope/edit"]) {
    test(`H-08 not found: ${route} has an h1`, async ({ page }) => {
      await visit(page, route);
      await expect(page.getByText(/was not found/)).toBeVisible();
      await expect(page.getByRole("heading", { level: 1 })).toHaveCount(1, { timeout: 3000 });
    });
  }
});
