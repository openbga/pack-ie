import { expect, type Page, test } from "@playwright/test";
import { article, articleTitle, browseTopics, TOPICS } from "./help.support";
import { SPOTS, type Spot } from "./helpSpots.support";
import {
  APP_PAGES,
  helpButton,
  helpPanel,
  mainText,
  openHelp,
  RAW_FAILURE,
  visit,
} from "./states.support";
import { heading } from "./support";

/** Topics of pages that export a file: their articles may say so. */
const EXPORTING_TOPICS = ["Audit", "Messages", "Saving and versions", "Transmission report"];

async function expectMentions(page: Page, mentions: Spot["mentions"]): Promise<void> {
  const body = (await article(page).innerText()).toLowerCase();
  for (const mention of mentions) {
    const found =
      typeof mention === "string" && !mention.includes("|")
        ? body.includes(mention.toLowerCase())
        : new RegExp(typeof mention === "string" ? mention : mention.source, "i").test(body);
    expect(found, `the article "${await articleTitle(page)}" should mention ${mention}`).toBe(true);
  }
}

test.describe("H-09 the article names things that exist on the page", () => {
  for (const spot of SPOTS) {
    test(`H-09 spot check: ${spot.name}`, async ({ page }) => {
      await visit(page, spot.route);
      await expect(page.getByRole("heading", { level: 1 }).first()).toBeVisible();
      await openHelp(page);
      expect(await articleTitle(page)).toBe(spot.topic);
      await expectMentions(page, spot.mentions);
      for (const find of spot.present) await expect(find(page)).toBeVisible();
      for (const find of spot.absent ?? []) await expect(find(page)).toHaveCount(0);
    });
  }

  test("H-09 spot check: Messages help says identifier and text are never in the address, and they are not", async ({
    page,
  }) => {
    await visit(page, "/messages");
    await openHelp(page);
    await expect(article(page)).toContainText(
      /Identifier, Message text and Message ID can hold patient data: they are never put in the address or saved/,
    );
    await helpButton(page).click();
    await page.getByRole("searchbox", { name: "Identifier" }).fill("PRIVATE-123");
    await page.getByRole("searchbox", { name: "Message text" }).fill("SECRET-456");
    await page.keyboard.press("Enter");
    await expect(page.getByRole("button", { name: "Clear filters" }).first()).toBeVisible();
    expect(page.url()).not.toMatch(/PRIVATE|SECRET/);
    expect(await page.title()).not.toMatch(/PRIVATE|SECRET/);
    await page.reload();
    await expect(page.getByRole("searchbox", { name: "Identifier" })).toHaveValue("");
  });

  test("H-09 spot check: Messages help lists the columns Columns brings back", async ({ page }) => {
    await visit(page, "/messages");
    await openHelp(page);
    await expect(article(page)).toContainText(/Columns brings any back/);
    await page.getByRole("button", { name: /^Columns/ }).click();
    for (const column of [
      "Patient ID",
      "Size",
      "Message ID",
      "Source peer",
      "Configuration version",
      "Resent from",
    ]) {
      await expect(page.getByRole("checkbox", { name: column })).toBeVisible();
      await expect(article(page)).toContainText(new RegExp(column.replace(" from", "-from"), "i"));
    }
  });

  test("H-09 spot check: Logs help names Jump to live and Load older, which the page offers", async ({
    page,
  }) => {
    await visit(page, "/logs");
    await openHelp(page);
    await expect(article(page)).toContainText(/Jump to live shows them and follows again/);
    await page.getByRole("button", { name: "Pause" }).click();
    await expect(page.getByRole("button", { name: "Jump to live" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Load older events" })).toBeVisible();
  });

  test("H-09 spot check: Delivery states says AU class A reaches only Handed off", async ({
    page,
  }) => {
    await visit(page, "/messages/msg-7");
    await openHelp(page);
    await helpPanel(page).getByRole("button", { name: "Browse all topics" }).click();
    await helpPanel(page).getByRole("button", { name: "Delivery states", exact: true }).click();
    await expect(article(page).getByRole("heading", { level: 3 })).toHaveText("Delivery states");
    await expect(article(page)).toContainText(/AU class A \(no acknowledgement\) only Handed off/);
    await expect(article(page)).toContainText(/ASTM and AU class B .* Received/);
    // The acknowledgement levels the article names are the ones the delivery card shows.
    await expect(page.getByRole("main")).toContainText(/Outcome unknown, may have been delivered/);
  });

  test("H-09 spot check: each help article of a page agrees with the labels of its page (columns)", async ({
    page,
  }) => {
    const checks = [
      {
        route: "/dashboard",
        grid: "Channels",
        columns: ["Channel", "State", "Endpoints", "Traffic", "Queue"],
      },
      {
        route: "/messages",
        grid: "Messages",
        columns: [
          "Received",
          "Channel",
          "Direction",
          "Message state",
          "Delivery summary",
          "Message type",
          "Identifier",
        ],
      },
      {
        route: "/alerts",
        grid: "Alerts",
        columns: ["Severity", "Next step", "State", "Concerns", "First seen", "Last seen"],
      },
    ];
    for (const check of checks) {
      await visit(page, check.route);
      const headers = page.getByRole("grid", { name: check.grid }).getByRole("columnheader");
      await expect(headers.first()).toBeVisible();
      const shown = (await headers.allInnerTexts()).map((header) => header.trim());
      await openHelp(page);
      const body = (await article(page).innerText()).toLowerCase();
      for (const column of check.columns) {
        expect(shown, `${check.route} shows ${column}`).toContain(column);
        expect(body, `the help of ${check.route} names ${column}`).toContain(column.toLowerCase());
      }
      await helpButton(page).click();
      await expect(helpPanel(page)).toBeHidden();
    }
  });
});

// H-09: Settings > Help and the truth of the text ---------------------------------------------

test.describe("H-09 Settings > Help and what the articles claim", () => {
  test("H-09 settings help: the tab lists the same topics as the drawer and opens them", async ({
    page,
  }) => {
    await visit(page, "/settings/help");
    await expect(heading(page, "Settings")).toBeVisible();
    const topics = page.getByRole("navigation", { name: "Help topics" }).getByRole("link");
    const listed = await topics.allInnerTexts();
    expect([...listed].sort()).toEqual([...TOPICS].sort());
    await openHelp(page);
    const inDrawer = await browseTopics(page);
    expect([...inDrawer].sort()).toEqual([...listed].sort());
    await page.keyboard.press("Escape");
    await topics.filter({ hasText: /^Delivery states$/ }).click();
    await expect(
      page.getByRole("main").getByRole("article").getByRole("heading", { level: 3 }),
    ).toHaveText("Delivery states");
  });

  test("H-09 no invented features: the articles name no control the app does not have", async ({
    page,
  }) => {
    // The words of features that do not exist in the console. "exports" in the Secrets text is
    // a noun (values are never written to exports); an Export control would be the verb. The
    // Audit page, the Transmission report and the version history export (their own articles
    // and buttons), so they are the exceptions.
    const invented = [
      /\bexport\b/i,
      /\bdownload/i,
      /\bprint/i,
      /\bemail/i,
      /\bundo\b/i,
      /\bclone\b/i,
    ];
    await visit(page, "/dashboard");
    const panel = await openHelp(page);
    const all: { topic: string; body: string }[] = [];
    for (const topic of TOPICS) {
      await panel.getByRole("button", { name: "Browse all topics" }).click();
      await panel.getByRole("button", { name: topic, exact: true }).click();
      all.push({ topic, body: await article(page).innerText() });
    }
    for (const word of invented) {
      expect(
        all
          .filter(({ topic }) => !(EXPORTING_TOPICS.includes(topic) && word.test("export")))
          .filter(({ body }) => word.test(body)),
        `help mentions ${word}`,
      ).toEqual([]);
    }
    // And the pages offer none of them either.
    await page.keyboard.press("Escape");
    for (const app of APP_PAGES) {
      await visit(page, app.route);
      await expect(page.getByRole("heading", { level: 1 }).first()).toBeVisible();
      const offered = EXPORTING_TOPICS.includes(app.topic)
        ? /^(download|print)\b/i
        : /^(export|download|print)\b/i;
      await expect(page.getByRole("button", { name: offered })).toHaveCount(0);
      await expect(page.getByRole("link", { name: /^(export|download|print)\b/i })).toHaveCount(0);
      expect(await mainText(page)).not.toMatch(RAW_FAILURE);
    }
  });
});
