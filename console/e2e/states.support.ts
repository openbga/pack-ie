import { expect, type Locator, type Page } from "@playwright/test";
import { type MockSwitches, open } from "./support";

/**
 * One page of the app, as the state journeys (states.spec.ts, H-08) and the help journeys
 * (help.spec.ts, H-09) visit it. Adding a page to the app is one line here.
 */
export interface AppPage {
  /** Names the page in test titles. */
  name: string;
  /** The hash route, with concrete ids from the mock. */
  route: string;
  /** The route of `src/app/routes.tsx` this page exercises. */
  pattern: string;
  /** The `h1` once the page has loaded. */
  heading: string | RegExp;
  /** The `h1` while the record loads or when it cannot be loaded (the page's generic title). */
  genericHeading: string;
  /**
   * What the page says while the engine answers. `null` for a page that follows the engine's
   * pushed snapshot (Dashboard, Channels, Endpoints) or holds no engine read (Settings > Help).
   */
  loading: string | null;
  /** The title of the failure the page shows when every request fails; `null` when it cannot. */
  error: string | null;
  /** The title of the help article for this page. */
  topic: string;
}

const CONFIG = "Loading configuration…";
const CONFIG_ERROR = "The configuration could not be loaded";

const MESSAGE_ID = "msg-7";
export const MESSAGE_CHANNEL = { id: "ch-msg-sta", label: "STA Compact results" } as const;
export const BRIDGE_CHANNEL = {
  id: "ch-br-acltop",
  label: "ACL TOP 550 coag to middleware",
} as const;
export const ENDPOINT = { id: "ep-xn2000", label: "Sysmex XN-2000" } as const;

// NOTE: the pages of Dashboard, Channels and Endpoints read the snapshot the engine pushes
// (`useSnapshotSync` in the shell), so `scenario=slow` and `scenario=errors` never reach their
// own loading and error states with the mock; the table says `loading: null, error: null`.
export const APP_PAGES: readonly AppPage[] = [
  entry("Dashboard", "/dashboard", "/dashboard", "Dashboard", "Dashboard", null, null, "Dashboard"),
  entry("Channels", "/channels", "/channels", "Channels", "Channels", null, null, "Channels"),
  entry(
    "New channel",
    "/channels/new",
    "/channels/new",
    "New channel",
    "New channel",
    CONFIG,
    CONFIG_ERROR,
    "Channel editor",
  ),
  entry(
    "Message channel",
    `/channels/${MESSAGE_CHANNEL.id}`,
    "/channels/:id",
    MESSAGE_CHANNEL.label,
    "Channel",
    "Loading channel…",
    "The channel could not be loaded",
    "Channel",
  ),
  entry(
    "Bridge channel",
    `/channels/${BRIDGE_CHANNEL.id}`,
    "/channels/:id",
    BRIDGE_CHANNEL.label,
    "Channel",
    "Loading channel…",
    "The channel could not be loaded",
    "Channel",
  ),
  entry(
    "Edit message channel",
    `/channels/${MESSAGE_CHANNEL.id}/edit`,
    "/channels/:id/edit",
    MESSAGE_CHANNEL.label,
    "Edit channel",
    CONFIG,
    CONFIG_ERROR,
    "Channel editor",
  ),
  entry(
    "Edit bridge channel",
    `/channels/${BRIDGE_CHANNEL.id}/edit`,
    "/channels/:id/edit",
    BRIDGE_CHANNEL.label,
    "Edit channel",
    CONFIG,
    CONFIG_ERROR,
    "Channel editor",
  ),
  entry("Endpoints", "/endpoints", "/endpoints", "Endpoints", "Endpoints", null, null, "Endpoints"),
  entry(
    "New endpoint",
    "/endpoints/new",
    "/endpoints/new",
    "New endpoint",
    "New endpoint",
    CONFIG,
    CONFIG_ERROR,
    "Endpoint editor",
  ),
  entry(
    "Endpoint",
    `/endpoints/${ENDPOINT.id}`,
    "/endpoints/:id",
    ENDPOINT.label,
    "Endpoint",
    "Loading endpoint…",
    "The endpoint could not be loaded",
    "Endpoint",
  ),
  entry(
    "Edit endpoint",
    `/endpoints/${ENDPOINT.id}/edit`,
    "/endpoints/:id/edit",
    ENDPOINT.label,
    "Edit endpoint",
    CONFIG,
    CONFIG_ERROR,
    "Endpoint editor",
  ),
  entry(
    "Messages",
    "/messages",
    "/messages",
    "Messages",
    "Messages",
    "Loading messages…",
    "The messages could not be loaded",
    "Messages",
  ),
  entry(
    "Message",
    `/messages/${MESSAGE_ID}`,
    "/messages/:id",
    /^[A-Z]{2}\d+$/,
    "Message",
    "Loading message…",
    "The message could not be loaded",
    "Message",
  ),
  entry(
    "Logs",
    "/logs",
    "/logs",
    "Logs",
    "Logs",
    "Loading log events…",
    "The log events could not be loaded",
    "Logs",
  ),
  entry(
    "Alerts",
    "/alerts",
    "/alerts",
    "Alerts",
    "Alerts",
    "Loading alerts…",
    "The alerts could not be loaded",
    "Alerts",
  ),
  entry(
    "Audit",
    "/audit",
    "/audit",
    "Audit",
    "Audit",
    "Loading audit events…",
    "The audit events could not be loaded",
    "Audit",
  ),
  entry(
    "Transmission report",
    "/transmission",
    "/transmission",
    "Transmission report",
    "Transmission report",
    null,
    null,
    "Transmission report",
  ),
  settings(
    "Settings Engine",
    "engine",
    "Loading engine settings…",
    "The engine settings could not be loaded",
    "Settings",
  ),
  settings(
    "Settings Retention",
    "retention",
    "Loading retention settings…",
    "The retention settings could not be loaded",
    "Settings",
  ),
  settings(
    "Settings Secrets",
    "secrets",
    "Loading secrets…",
    "The secrets could not be loaded",
    "Settings",
  ),
  settings(
    "Settings Versions",
    "versions",
    "Loading version history…",
    "The version history could not be loaded",
    "Saving and versions",
  ),
  settings(
    "Settings About",
    "about",
    "Loading version information…",
    "The version information could not be loaded",
    "Settings",
  ),
  settings("Settings Help", "help", null, null, "Settings"),
];

function entry(
  name: string,
  route: string,
  pattern: string,
  heading: string | RegExp,
  genericHeading: string,
  loading: string | null,
  error: string | null,
  topic: string,
): AppPage {
  return { name, route, pattern, heading, genericHeading, loading, error, topic };
}

function settings(
  name: string,
  tab: string,
  loading: string | null,
  error: string | null,
  topic: string,
): AppPage {
  return entry(
    name,
    `/settings/${tab}`,
    "/settings/:tab",
    "Settings",
    "Settings",
    loading,
    error,
    topic,
  );
}

/**
 * Opens a route against a small mock (500 messages): these journeys do not depend on the size
 * of the history, and the default 412,000 messages make every page load slower.
 */
export function visit(page: Page, route: string, switches: MockSwitches = {}): Promise<void> {
  return open(page, route, { messages: 500, ...switches });
}

/** The top bar's Help button (not the drawer's "Close page help"). */
export function helpButton(page: Page): Locator {
  return page.getByRole("banner").getByRole("button", { name: "Help", exact: true });
}

/** The help drawer, titled "Page help". */
export function helpPanel(page: Page): Locator {
  return page.getByRole("region", { name: "Page help" });
}

/** Opens the drawer from the top bar and waits for its article. */
export async function openHelp(page: Page): Promise<Locator> {
  await helpButton(page).click();
  const panel = helpPanel(page);
  await expect(panel.getByRole("heading", { level: 2, name: "Page help" })).toBeVisible();
  await expect(panel.getByRole("article")).toBeVisible();
  return panel;
}

/** Waits until `locator` has the focus, or holds the element that has it. */
export async function expectFocusWithin(locator: Locator): Promise<void> {
  await expect
    .poll(() => locator.evaluate((node) => node.contains(document.activeElement)))
    .toBe(true);
}

/** The visible text of the main area. */
export function mainText(page: Page): Promise<string> {
  return page.getByRole("main").innerText();
}

/** Text that only a crash or a developer would see; no page may show it. */
export const RAW_FAILURE = /TypeError|ReferenceError|Unhandled|undefined|\[object |at \S+\.tsx?:/;

/** The main area has text, and none of it is a crash's. */
export async function expectNotBlank(page: Page): Promise<void> {
  const text = await mainText(page);
  expect(text.trim().length).toBeGreaterThan(0);
  expect(text).not.toMatch(RAW_FAILURE);
}

/**
 * Creates an unused TCP endpoint through the editor (every endpoint of the mock belongs to a
 * channel), and leaves the page on its detail screen.
 */
export async function createEndpoint(page: Page, label: string, id: string): Promise<void> {
  await visit(page, "/endpoints/new");
  await page.getByRole("button", { name: /^Select… Kind/ }).click();
  await page.getByRole("option", { name: "TCP client" }).click();
  await page.getByRole("textbox", { name: /^Label/ }).fill(label);
  const idField = page.getByRole("textbox", { name: /^Id\b/ });
  await idField.fill(id);
  await page.getByRole("textbox", { name: /^Host/ }).fill("lis.example.test");
  await page.getByRole("textbox", { name: /^Port/ }).fill("3999");
  await page.getByRole("button", { name: "Save and apply" }).click();
  await page.getByRole("textbox", { name: /^Change note/ }).fill("Journey change");
  await page.getByRole("button", { name: /^Apply version \d+/ }).click();
  await expect(page).toHaveURL(new RegExp(`#/endpoints/${id}$`));
}

export function confirmDialog(page: Page, name: string | RegExp): Locator {
  return page.getByRole("alertdialog", { name });
}

/** A dialog opened by a keyboard-reachable control: Cancel has the focus, Escape gives it back. */
export async function expectEscapeReturnsFocus(
  page: Page,
  dialog: Locator,
  trigger: Locator,
): Promise<void> {
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole("button", { name: "Cancel" })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  await expectFocusWithin(trigger);
}

/** A dialog that Escape and Cancel both close, leaving the page as it was; Escape returns the focus. */
export async function expectDismissals(
  page: Page,
  dialog: Locator,
  trigger: Locator,
): Promise<void> {
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  await expectFocusWithin(trigger);
  await trigger.click();
  await dialog.getByRole("button", { name: "Cancel" }).click();
  await expect(dialog).toHaveCount(0);
}
