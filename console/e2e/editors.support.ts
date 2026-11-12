import { expect, type Locator, type Page } from "@playwright/test";
import { heading, loaded, type MockSwitches, open } from "./support";

const escapeRe = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/** Opens an editor route (`/endpoints/new`, `/channels/x/edit`) and waits for its form. */
export async function openEditor(
  page: Page,
  route: string,
  title: string | RegExp,
  switches: MockSwitches = {},
): Promise<void> {
  await open(page, route, switches);
  await loaded(page, title);
  await expect(page.getByRole("form")).toBeVisible();
}

/** The editor's form. */
export const form = (page: Page): Locator => page.getByRole("form");

/** A text field by its label, within `scope` (the whole form by default). */
export function text(page: Page, label: string, scope?: Locator): Locator {
  return (scope ?? form(page)).getByRole("textbox", { name: label, exact: true });
}

/** A select by its label: its button is named "<chosen value> <label>". */
export function select(page: Page, label: string, scope?: Locator): Locator {
  return (scope ?? form(page)).getByRole("button", { name: new RegExp(`${escapeRe(label)}$`) });
}

/** Chooses `option` in the select called `label`. */
export async function pick(
  page: Page,
  label: string,
  option: string | RegExp,
  scope?: Locator,
): Promise<void> {
  await select(page, label, scope).click();
  await page.getByRole("option", { name: option, exact: typeof option === "string" }).click();
}

/** Fills the text fields named by the keys of `values`. */
export async function fill(
  page: Page,
  values: Record<string, string>,
  scope?: Locator,
): Promise<void> {
  for (const [label, value] of Object.entries(values)) {
    await text(page, label, scope).fill(value);
  }
}

/** The General section, whose Label and Id are the record's own. */
export const general = (page: Page): Locator => page.getByRole("group", { name: "General" });

/** A group (fieldset or section) of the form by its legend. */
export const group = (page: Page, name: string | RegExp): Locator =>
  page.getByRole("group", { name });

/** Asserts these text fields carry the required marker (and the "*" in their label). */
export async function expectRequired(
  page: Page,
  labels: readonly string[],
  scope?: Locator,
): Promise<void> {
  for (const label of labels) {
    await expect(text(page, label, scope), `${label} is required`).toHaveAttribute(
      "aria-required",
      "true",
    );
    await expect(
      (scope ?? form(page)).locator("label", { hasText: /\*/ }).filter({
        hasText: new RegExp(`^${escapeRe(label)}\\s*\\*$`),
      }),
    ).toHaveCount(1);
  }
}

/** Asserts these text fields are optional: no marker. */
export async function expectOptional(
  page: Page,
  labels: readonly string[],
  scope?: Locator,
): Promise<void> {
  for (const label of labels) {
    await expect(text(page, label, scope)).not.toHaveAttribute("aria-required", "true");
  }
}

export const validate = (page: Page) => page.getByRole("button", { name: "Validate" });
export const saveAndApply = (page: Page) => page.getByRole("button", { name: "Save and apply" });

/** The engine's validation results under the form. */
export const results = (page: Page): Locator =>
  page.getByRole("region", { name: "Validation results" });

/** Presses Validate and waits for its answer: results, or "No problems found". */
export async function runValidate(page: Page): Promise<void> {
  await validate(page).click();
  await expect(results(page).or(page.getByText("No problems found"))).toBeVisible();
}

/** An issue the engine reported: its message and its `path · code` line in the results. */
export async function expectIssue(
  page: Page,
  message: string | RegExp,
  pathAndCode: string | RegExp,
): Promise<void> {
  const item = results(page).getByRole("listitem").filter({ hasText: message });
  await expect(item).toHaveCount(1);
  await expect(item).toContainText(pathAndCode);
}

/** The confirmation dialog of Save and apply. */
export const reviewDialog = (page: Page): Locator =>
  page.getByRole("alertdialog", { name: /^Apply configuration version \d+\?$/ });

/** Validates, opens the plan and applies; returns the new version number. */
export async function saveAndConfirm(page: Page, note = "Journey change"): Promise<number> {
  await saveAndApply(page).click();
  const dialog = reviewDialog(page);
  await expect(dialog).toBeVisible();
  const title = (await dialog.getByRole("heading").first().textContent()) ?? "";
  const version = Number(/version (\d+)/.exec(title)?.[1]);
  await dialog.getByRole("textbox", { name: /^Change note/ }).fill(note);
  await dialog.getByRole("button", { name: `Apply version ${version}` }).click();
  await expect(page.getByText(`Configuration version ${version} applied`)).toBeVisible();
  return version;
}

/** Settings > Versions: the grid of versions. */
export async function openVersions(page: Page, version?: number): Promise<void> {
  await page.evaluate(
    (search) => {
      window.location.hash = `/settings/versions${search}`;
    },
    version === undefined ? "" : `?version=${version}`,
  );
  await expect(heading(page, "Settings")).toBeVisible();
  await expect(page.getByRole("grid", { name: "Configuration versions" })).toBeVisible();
}

/** The row of a version in Settings > Versions. */
export const versionRow = (page: Page, version: number): Locator =>
  page
    .getByRole("grid", { name: "Configuration versions" })
    .getByRole("row")
    .filter({ hasText: `Version ${version}` });

/** Ticks or clears a checkbox by its label (its box is drawn over the input). */
export async function setChecked(page: Page, label: string, checked: boolean): Promise<void> {
  const box = form(page).getByRole("checkbox", { name: label, exact: true });
  if ((await box.isChecked()) !== checked) {
    await form(page).locator("label", { hasText: label }).first().click();
  }
  await expect(box).toBeChecked({ checked });
}

/** Chooses a record in a searchable picker (an endpoint, say) by its id, shown in each option. */
export async function pickRecord(
  page: Page,
  label: string,
  id: string,
  scope?: Locator,
): Promise<void> {
  const root = scope ?? form(page);
  const options = page.getByRole("option");
  await root.getByRole("button", { name: `Show suggestions ${label}`, exact: true }).click();
  // The list sometimes closes again as focus moves from the picker used before; ask once more.
  if (
    !(await options
      .first()
      .isVisible({ timeout: 1_500 })
      .catch(() => false))
  ) {
    await root.getByRole("combobox", { name: label, exact: true }).press("ArrowDown");
  }
  await options.filter({ hasText: new RegExp(`· ${escapeRe(id)}$`) }).click();
}

/** The picker's current text. */
export const recordValue = (page: Page, label: string, scope?: Locator): Locator =>
  (scope ?? form(page)).getByRole("combobox", { name: label, exact: true });

/** Opens the new-message-channel editor with a valid source (HL7 over a listener) and destination. */
export async function startMessageChannel(page: Page, label: string): Promise<void> {
  await openEditor(page, "/channels/new", "New channel");
  await pick(page, "Kind", "Message");
  await fill(page, { Label: label }, general(page));
  const source = group(page, "Source");
  await pickRecord(page, "Endpoint", "ep-lis-orders", source);
  await pick(page, "Protocol", "HL7 v2", source);
  await pick(page, "Acknowledgement mode", "Original", source);
  const destination = group(page, "Destination 1");
  await fill(page, { Label: "To LIS" }, destination);
  await pickRecord(page, "Endpoint", "ep-lis-out", destination);
  await pick(page, "Protocol", "HL7 v2", destination);
  await pick(page, "Acknowledgement mode", "Original", destination);
}

/** A rule or filter row by its exact name, such as "Destination 1 mapping rule 2". */
export const row = (page: Page, name: string): Locator =>
  page.getByRole("group", { name, exact: true });

export async function optionsOf(page: Page, scope: Locator, label: string): Promise<string[]> {
  await select(page, label, scope).click();
  const names = await page.getByRole("option").allTextContents();
  await page.keyboard.press("Escape");
  return names;
}

const DEST = "Destination 1";
export const MAPPINGS = `${DEST} mapping rules`;
export const rule = (page: Page, number: string) => row(page, `${DEST} mapping rule ${number}`);
export const addRule = (page: Page) =>
  group(page, MAPPINGS)
    .getByRole("button", { name: "Add destination 1 mapping rule", exact: true })
    .click();

/** A message channel whose destination speaks ASTM, so it needs a mapping. */
export async function mappedChannel(page: Page, label: string): Promise<void> {
  await startMessageChannel(page, label);
  const destination = group(page, DEST);
  await pick(page, "Protocol", "ASTM", destination);
  await pick(page, "Link role", "Computer", destination);
}
