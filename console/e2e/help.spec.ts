import { readFileSync } from "node:fs";
import { expect, type Page, test } from "@playwright/test";
import { article, articleTitle, browseTopics, TOPICS } from "./help.support";
import {
  APP_PAGES,
  type AppPage,
  helpButton,
  helpPanel,
  openHelp,
  RAW_FAILURE,
  visit,
} from "./states.support";
import { heading } from "./support";

/**
 * "Short" is about 300 words. NOTE: four articles are a little over that today (Channel editor,
 * Endpoint editor, Message, Saving and versions: 305 to 330 words), so the limit has room.
 */
const MAX_WORDS = 350;

/** Routes that are not in `APP_PAGES` but still have help: redirects, versions, not-found. */
const EXTRA_PAGES: readonly Pick<AppPage, "name" | "route" | "topic">[] = [
  { name: "Home redirect", route: "/", topic: "Dashboard" },
  { name: "Settings redirect", route: "/settings", topic: "Settings" },
  { name: "A version", route: "/settings/versions?version=46", topic: "Saving and versions" },
  { name: "Channel not found", route: "/channels/nope", topic: "Channel" },
  { name: "Endpoint not found", route: "/endpoints/nope", topic: "Endpoint" },
  { name: "Message not found", route: "/messages/nope", topic: "Message" },
  { name: "Channel editor not found", route: "/channels/nope/edit", topic: "Channel editor" },
  { name: "Endpoint editor not found", route: "/endpoints/nope/edit", topic: "Endpoint editor" },
];

/** Waits for the drawer to show the article called `title` (it follows the page asynchronously). */
async function expectArticle(page: Page, title: string): Promise<void> {
  await expect(article(page).getByRole("heading", { level: 3 }).first()).toHaveText(title);
}

function wordCount(text: string): number {
  return text.split(/\s+/).filter((word) => /\w/.test(word)).length;
}

/** Routes of `src/app/routes.tsx`, read from the source (the built console has no copy; journeys run in `console/`). */
function routePatterns(): string[] {
  const source = readFileSync("src/app/routes.tsx", "utf8");
  return [...source.matchAll(/lazyRoute\("([^"]+)"/g)].map((match) => match[1] ?? "");
}

// H-09: every route has help ------------------------------------------------------------------

test.describe("H-09 every route has a help panel with a short article about the page", () => {
  test("H-09 routes: the pages exercised here are exactly the routes of the app", () => {
    const routes = routePatterns();
    expect(routes.length).toBeGreaterThan(10);
    expect([...new Set(APP_PAGES.map((app) => app.pattern))].sort()).toEqual([...routes].sort());
    for (const app of APP_PAGES) expect(TOPICS).toContain(app.topic);
  });

  for (const app of [...APP_PAGES, ...EXTRA_PAGES]) {
    test(`H-09 help: ${app.name} opens "Page help" with the article "${app.topic}"`, async ({
      page,
    }) => {
      await visit(page, app.route);
      await expect(page.getByRole("heading", { level: 1 }).first()).toBeVisible();
      const button = helpButton(page);
      await expect(button).toHaveAttribute("aria-expanded", "false");
      const panel = await openHelp(page);
      await expect(button).toHaveAttribute("aria-expanded", "true");
      await expect(panel.getByRole("heading", { level: 2, name: "Page help" })).toBeFocused();
      expect(await articleTitle(page)).toBe(app.topic);
      const text = await article(page).innerText();
      expect(wordCount(text), `${app.topic} has ${wordCount(text)} words`).toBeLessThan(MAX_WORDS);
      expect(wordCount(text)).toBeGreaterThan(40);
      expect(text).not.toMatch(RAW_FAILURE);
    });
  }
});

// H-09: the drawer ----------------------------------------------------------------------------

test.describe("H-09 the help drawer", () => {
  test("H-09 drawer: Browse all topics lists every article and opens one, Current page returns", async ({
    page,
  }) => {
    await visit(page, "/channels");
    const panel = await openHelp(page);
    expect(await articleTitle(page)).toBe("Channels");
    const topics = await browseTopics(page);
    expect([...topics].sort()).toEqual([...TOPICS].sort());
    await expect(panel.getByRole("heading", { level: 3, name: "Help topics" })).toBeVisible();
    // Every topic has a one-line summary beside it.
    await expect(panel.getByRole("listitem")).toHaveCount(TOPICS.length);

    await panel.getByRole("button", { name: "Mapping rules", exact: true }).click();
    expect(await articleTitle(page)).toBe("Mapping rules");
    await panel.getByRole("button", { name: "Current page" }).click();
    expect(await articleTitle(page)).toBe("Channels");
  });

  test("H-09 drawer: every topic opens and is short", async ({ page }) => {
    await visit(page, "/dashboard");
    const panel = await openHelp(page);
    for (const topic of TOPICS) {
      await panel.getByRole("button", { name: "Browse all topics" }).click();
      await panel.getByRole("button", { name: topic, exact: true }).click();
      expect(await articleTitle(page)).toBe(topic);
      const words = wordCount(await article(page).innerText());
      expect(words, `${topic} has ${words} words`).toBeLessThan(MAX_WORDS);
      expect(words).toBeGreaterThan(40);
    }
  });

  test("H-09 drawer: navigating to another page resets it to that page's topic", async ({
    page,
  }) => {
    await visit(page, "/channels");
    const panel = await openHelp(page);
    await browseTopics(page);
    await panel.getByRole("button", { name: "Delivery states", exact: true }).click();
    await expectArticle(page, "Delivery states");
    const nav = page.getByRole("navigation", { name: "Main" });
    await nav.getByRole("link", { name: "Messages" }).click();
    await expect(heading(page, "Messages")).toBeVisible();
    await expect(panel).toBeVisible();
    await expectArticle(page, "Messages");

    // Also from the browse list, and for a page reached from a link inside the page.
    await browseTopics(page);
    await nav.getByRole("link", { name: "Logs" }).click();
    await expect(heading(page, "Logs")).toBeVisible();
    await expectArticle(page, "Logs");
    await nav.getByRole("link", { name: "Settings" }).click();
    await expect(heading(page, "Settings")).toBeVisible();
    await expectArticle(page, "Settings");
    await page.getByRole("tab", { name: "Versions" }).click();
    await expect(page).toHaveURL(/#\/settings\/versions/);
    await expectArticle(page, "Saving and versions");
    await page.getByRole("tab", { name: "About" }).click();
    await expectArticle(page, "Settings");
  });

  test("H-09 drawer: Escape closes it and focus returns to the Help button", async ({ page }) => {
    await visit(page, "/alerts");
    const panel = await openHelp(page);
    await expect(helpButton(page)).toHaveAttribute("aria-expanded", "true");
    await page.keyboard.press("Escape");
    await expect(panel).toBeHidden();
    await expect(helpButton(page)).toBeFocused();
    await expect(helpButton(page)).toHaveAttribute("aria-expanded", "false");

    // From the browse list, with the focus on a topic button.
    await openHelp(page);
    await browseTopics(page);
    await helpPanel(page).getByRole("button", { name: "Logs", exact: true }).focus();
    await page.keyboard.press("Escape");
    await expect(helpPanel(page)).toBeHidden();
    await expect(helpButton(page)).toBeFocused();
  });

  // Every opening mounts the drawer afresh, so focus goes in each time and Escape works.
  test("H-09 drawer: every opening moves the focus into the drawer, so Escape closes it", async ({
    page,
  }) => {
    await visit(page, "/alerts");
    await openHelp(page);
    await page.keyboard.press("Escape");
    await expect(helpPanel(page)).toBeHidden();
    const panel = await openHelp(page);
    await expect(panel.getByRole("heading", { level: 2, name: "Page help" })).toBeFocused({
      timeout: 3000,
    });
    await page.keyboard.press("Escape");
    await expect(panel).toBeHidden({ timeout: 3000 });
  });

  test("H-09 drawer: Close page help and the Help button both close it", async ({ page }) => {
    await visit(page, "/endpoints");
    await openHelp(page);
    await page.getByRole("button", { name: "Close page help" }).click();
    await expect(helpPanel(page)).toBeHidden();
    await expect(helpButton(page)).toBeFocused();
    await openHelp(page);
    await helpButton(page).click();
    await expect(helpPanel(page)).toBeHidden();
    // Opened again, it shows the article of the page once more.
    await openHelp(page);
    expect(await articleTitle(page)).toBe("Endpoints");
  });

  test("H-09 drawer: it is operable with the keyboard alone", async ({ page }) => {
    await visit(page, "/logs");
    await expect(heading(page, "Logs")).toBeVisible();
    await helpButton(page).focus();
    await page.keyboard.press("Enter");
    const panel = helpPanel(page);
    await expect(panel.getByRole("heading", { level: 2, name: "Page help" })).toBeFocused();
    await page.keyboard.press("Tab");
    await expect(panel.getByRole("button", { name: "Close page help" })).toBeFocused();
    await page.keyboard.press("Tab");
    await expect(panel.getByRole("button", { name: "Current page" })).toBeFocused();
    await page.keyboard.press("Tab");
    await expect(panel.getByRole("button", { name: "Browse all topics" })).toBeFocused();
    await page.keyboard.press("Enter");
    await expect(panel.getByRole("heading", { level: 3, name: "Help topics" })).toBeVisible();
    await panel.getByRole("button", { name: "Logs", exact: true }).focus();
    await page.keyboard.press("Escape");
    await expect(panel).toBeHidden();
    await expect(helpButton(page)).toBeFocused();
  });

  // Neither topic button disables itself, so the focus stays in the drawer and Escape works.
  test("H-09 drawer: focus stays inside the drawer after Browse all topics", async ({ page }) => {
    await visit(page, "/logs");
    await expect(heading(page, "Logs")).toBeVisible();
    await helpButton(page).focus();
    await page.keyboard.press("Enter");
    const panel = helpPanel(page);
    await expect(panel.getByRole("heading", { level: 2, name: "Page help" })).toBeFocused();
    await panel.getByRole("button", { name: "Browse all topics" }).focus();
    await page.keyboard.press("Enter");
    await expect(panel.getByRole("heading", { level: 3, name: "Help topics" })).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(panel).toBeHidden({ timeout: 3000 });
  });
});
