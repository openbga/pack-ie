import { expect, type Locator, type Page, test } from "@playwright/test";
import { rowOfAction } from "./audit.support";
import { downloadFrom, downloadText, parseCsv } from "./download.support";
import {
  columnTexts,
  messageRows,
  openMessages,
  settled,
  tick,
  totalCount,
} from "./messages.support";
import { toast } from "./operate.support";
import { dataRows, eventPanel, loaded, mockHandle, navigateTo, open } from "./support";

test.use({ timezoneId: "UTC" });

const HEADER =
  "message_id,received_at,channel_id,direction,state,protocol,message_type,control_id,sample_id,patient_id,byte_length,deliveries";
const CONTENT = "Include message content (original bodies)";

/** A small history and a filter that selects a few dozen of it: every row can be compared. */
const HISTORY = { messages: 400 };
const FILTER = "?quick=attention&direction=inbound";

const exportDialog = (page: Page): Locator => page.getByRole("dialog", { name: "Export messages" });

/** Opens Export… and returns its dialog. */
async function openExport(page: Page): Promise<Locator> {
  await page.getByRole("button", { name: "Export…" }).click();
  const dialog = exportDialog(page);
  await expect(dialog).toBeVisible();
  return dialog;
}

/** Presses Export and returns what was downloaded. */
async function exportFile(page: Page, dialog: Locator): Promise<string> {
  const file = await downloadFrom(
    page,
    dialog.getByRole("button", { name: "Export", exact: true }),
  );
  expect(file.suggestedFilename()).toBe("packie-messages.csv");
  return downloadText(file);
}

/** What the grid shows as each row's identifier (control id, else sample id), newest first. */
async function gridIdentifiers(page: Page): Promise<string[]> {
  return columnTexts(page, "Identifier");
}

test("ME-01: Export… of a filtered grid downloads a CSV of exactly those messages, oldest first, without content", async ({
  page,
}) => {
  await openMessages(page, FILTER, HISTORY);
  const total = await totalCount(page);
  expect(total).toBeGreaterThan(2);
  expect(total).toBeLessThan(50);
  const shown = await gridIdentifiers(page);
  expect(shown).toHaveLength(total);

  const dialog = await openExport(page);
  await expect(dialog).toContainText(
    "Filters in effect: received in the last 24 hours, direction Inbound, quick filter Needs attention.",
  );
  await expect(dialog).toContainText(`${total} messages match.`);
  // The file lists patient identifiers even without content: said before anything is exported.
  await expect(dialog.getByText("The file lists patient identifiers")).toBeVisible();
  await expect(dialog).toContainText("Save the file to a protected location");
  await expect(dialog.getByRole("checkbox", { name: CONTENT })).not.toBeChecked();
  await expect(dialog.getByText("The file will contain message content")).toHaveCount(0);

  const records = parseCsv(await exportFile(page, dialog));
  expect(records[0]?.join(",")).toBe(HEADER);
  expect(records[0]).not.toContain("body");
  const rows = records.slice(1);
  expect(rows).toHaveLength(total);
  // The same messages as the grid, in the opposite (oldest first) order.
  expect(rows.map((row) => row[7] || row[8])).toEqual([...shown].reverse());
  const times = rows.map((row) => row[1] ?? "");
  expect(times).toEqual([...times].sort());
  expect(new Set(rows.map((row) => row[3]))).toEqual(new Set(["inbound"]));
  for (const row of rows) {
    expect(row).toHaveLength(12);
    expect(row[0]).toMatch(/^msg-\d+$/);
    // At least one delivery needs attention: `destination:state:ack_level`.
    expect(row[11]).toMatch(/^[^:;]+:[a-z_]+:[a-z_]+(;[^:;]+:[a-z_]+:[a-z_]+)*$/);
    expect(row[11]).toMatch(/:(failed|rejected|timed_out|outcome_unknown):/);
  }

  await expect(dialog).toHaveCount(0);
  await expect(toast(page, "Messages exported")).toContainText(
    `${total} messages saved to packie-messages.csv.`,
  );
});

test("ME-02: content needs its own choice, which shows the stronger warning, and then the file has bodies", async ({
  page,
}) => {
  await openMessages(page, FILTER, HISTORY);
  const total = await totalCount(page);
  const dialog = await openExport(page);
  await tick(dialog, CONTENT);
  const warning = dialog.getByText("The file will contain message content");
  await expect(warning).toBeVisible();
  await expect(dialog).toContainText("can include patient information");
  await expect(dialog).toContainText(
    "The engine records this export in the audit log as a content view.",
  );
  // The warning is above the button that acts on it.
  const warningBox = await warning.boundingBox();
  const buttonBox = await dialog.getByRole("button", { name: "Export", exact: true }).boundingBox();
  expect(warningBox && buttonBox && warningBox.y < buttonBox.y).toBe(true);

  const records = parseCsv(await exportFile(page, dialog));
  expect(records[0]?.join(",")).toBe(`${HEADER},body`);
  const rows = records.slice(1);
  expect(rows).toHaveLength(total);
  for (const row of rows) {
    expect(row).toHaveLength(13);
    // The original bytes: a whole message of its protocol, not a summary.
    expect(row[12]?.length).toBeGreaterThan(40);
    expect(row[12]?.length).toBe(Number(row[10]));
  }
  await expect(toast(page, "Messages exported")).toContainText("with their content");
});

test("ME-02: unticking the content choice takes the stronger warning away again", async ({
  page,
}) => {
  await openMessages(page, FILTER, HISTORY);
  const dialog = await openExport(page);
  await tick(dialog, CONTENT);
  await expect(dialog.getByText("The file will contain message content")).toBeVisible();
  await tick(dialog, CONTENT, false);
  await expect(dialog.getByText("The file will contain message content")).toHaveCount(0);
  // The safe-storage warning stays whatever is chosen.
  await expect(dialog.getByText("The file lists patient identifiers")).toBeVisible();
});

test("ME-03: a failed export shows its error in the dialog and leaves no download", async ({
  page,
}) => {
  const downloads: string[] = [];
  page.on("download", (download) => downloads.push(download.suggestedFilename()));
  await openMessages(page, FILTER, HISTORY);
  const dialog = await openExport(page);
  // The audit log cannot be written: the engine refuses audited requests before doing them.
  await mockHandle(page, (handle) => handle.setAuditAvailable(false));
  await dialog.getByRole("button", { name: "Export", exact: true }).click();
  const alert = dialog.getByRole("alert").filter({ hasText: "The export failed" });
  await expect(alert).toContainText("The audit log cannot be written.");
  await expect(dialog).toContainText("A partial file is not kept.");
  await expect(dialog).toBeVisible();
  expect(downloads).toEqual([]);
  await expect(
    page.getByRole("region", { name: "Notifications" }).getByText("Messages exported"),
  ).toHaveCount(0);

  // Once the log can be written the same dialog exports.
  await mockHandle(page, (handle) => handle.setAuditAvailable(true));
  const records = parseCsv(await exportFile(page, dialog));
  expect(records.length).toBeGreaterThan(1);
  expect(downloads).toEqual(["packie-messages.csv"]);
});

test("ME-04: the export is in the audit log with its filter, format, content choice and count", async ({
  page,
}) => {
  await openMessages(page, FILTER, HISTORY);
  const total = await totalCount(page);
  const dialog = await openExport(page);
  await tick(dialog, CONTENT);
  await exportFile(page, dialog);
  await expect(dialog).toHaveCount(0);

  // The mock's audit log lives in the page: go there inside the app, not by reloading.
  await navigateTo(page, "Audit");
  await loaded(page, "Audit");
  const newest = dataRows(page, "Audit events").first();
  // Which columns fit depends on the width, so the row is read as text.
  await expect(newest).toContainText("Messages exported");
  await expect(newest).toContainText("Succeeded");
  await expect(newest).toContainText("LAB\\jdoe");
  await rowOfAction(page, "Messages exported").click();
  const panel = eventPanel(page);
  await expect(panel.getByText("message_export", { exact: true })).toBeVisible();
  /** The value of a detail: its row is the `div` holding the key's code and the value. */
  const detail = (key: string) =>
    panel
      .locator("dl > div")
      .filter({ has: page.locator("code", { hasText: new RegExp(`^${key}$`) }) });
  await expect(detail("format")).toContainText("csv");
  await expect(detail("include_bodies")).toContainText("true");
  await expect(detail("count")).toContainText(new RegExp(`${total}$`));
  await expect(detail("filter")).toContainText("inbound");
  await expect(detail("filter")).toContainText("attention");
});

test("ME-05: the export covers every match of the filter, not only the page on screen", async ({
  page,
}) => {
  // 1,500 messages routed and inbound: more than the 100 of a page.
  await openMessages(page, "?state=routed&range=any", { messages: 1500 });
  const total = await totalCount(page);
  expect(total).toBeGreaterThan(100);
  expect(await messageRows(page).count()).toBeLessThan(total);
  await settled(page);
  const dialog = await openExport(page);
  await expect(dialog).toContainText(`${total.toLocaleString("en-US")} messages match.`);
  const rows = parseCsv(await exportFile(page, dialog)).slice(1);
  expect(rows).toHaveLength(total);
  expect(new Set(rows.map((row) => row[4]))).toEqual(new Set(["routed"]));
  expect(new Set(rows.map((row) => row[0])).size).toBe(total);
});

test("ME-06: Export… is there when no message matches, and says so", async ({ page }) => {
  await open(page, "/messages?state=aborted&channel=ch-none", HISTORY);
  await loaded(page, "Messages");
  await expect(page.getByText("No messages match")).toBeVisible();
  const dialog = await openExport(page);
  await expect(dialog).toContainText("No messages match: the file would hold only its header.");
  const records = parseCsv(await exportFile(page, dialog));
  expect(records).toHaveLength(1);
  expect(records[0]?.join(",")).toBe(HEADER);
});
