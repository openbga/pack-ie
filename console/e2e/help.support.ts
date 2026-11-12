import { expect, type Locator, type Page } from "@playwright/test";
import { helpPanel } from "./states.support";

/** Every help article, by title (console/help/*.json). A new article is one line here. */
export const TOPICS = [
  "Alerts",
  "AU instrument settings",
  "Audit",
  "Backup",
  "Calculations and text steps",
  "Channel",
  "Channel editor",
  "Channels",
  "Connection to the engine",
  "Dashboard",
  "Delivery states",
  "Endpoint",
  "Endpoint editor",
  "Endpoints",
  "Filters and pass-through",
  "Live transcript",
  "Logs",
  "Mapping rules",
  "Message",
  "Messages",
  "Notifications",
  "Protocol settings",
  "Saving and versions",
  "Settings",
  "Source requirements",
  "States",
  "Transmission report",
] as const;

export function article(page: Page): Locator {
  return helpPanel(page).getByRole("article");
}

export async function articleTitle(page: Page): Promise<string> {
  return (await article(page).getByRole("heading", { level: 3 }).first().innerText()).trim();
}

/** The topics offered by "Browse all topics", in order. */
export async function browseTopics(page: Page): Promise<string[]> {
  await helpPanel(page).getByRole("button", { name: "Browse all topics" }).click();
  const list = helpPanel(page).getByRole("list");
  await expect(list.getByRole("button").first()).toBeVisible();
  return list.getByRole("button").allInnerTexts();
}
