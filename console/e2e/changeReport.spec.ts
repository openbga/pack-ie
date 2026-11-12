import { expect, type Page, test } from "@playwright/test";
import { COLUMN } from "./audit.support";
import { downloadFrom, downloadText, parseCsv } from "./download.support";
import {
  fill,
  general,
  openEditor,
  openVersions,
  reviewDialog,
  saveAndApply,
  setChecked,
  versionRow,
} from "./editors.support";
import { dataRows, eventPanel, mockHandle, open } from "./support";

// Per-channel outcomes of an apply (VO) and the change report (CR): docs/spec/configuration.md
// section 10 (Result, Failure, In place) and requirement P4 in docs/requirements.md. The seeded history is
// versions 35 to 47: 35 is the startup import, 36 added the dashboard feed, 43 retired the Cobas
// legacy channel, 44 had a channel fail, 46 differs from the active 47 only in the feed's
// retention. Versions up to 45 were signed off by LAB\mlopez; 46 and 47 were not.
const ME = "LAB\\jdoe";
const FAILURE = "Cannot open COM24: access denied";

const rollbackDialog = (page: Page, version: number) =>
  page.getByRole("alertdialog", { name: `Roll back to version ${version}?` });
const reportDialog = (page: Page) => page.getByRole("dialog", { name: "Export change report" });

/** Chooses `option` in the dialog's select called `label`. */
async function choose(page: Page, label: string, option: string | RegExp) {
  await reportDialog(page)
    .getByRole("button", { name: new RegExp(`${label}$`) })
    .click();
  await page.getByRole("option", { name: option }).click();
}

type Sheet = Record<string, string>[];

function sheetOf(text: string): Sheet {
  const [header = [], ...rows] = parseCsv(text);
  return rows.map((cells) => Object.fromEntries(header.map((name, i) => [name, cells[i] ?? ""])));
}

/** Exports the report with the given format and scope and returns the file's text and name. */
async function exportReport(
  page: Page,
  options: { format?: "JSON"; scope?: string } = {},
): Promise<{ text: string; name: string }> {
  await page.getByRole("button", { name: "Export change report…" }).click();
  const dialog = reportDialog(page);
  await expect(dialog).toBeVisible();
  if (options.scope) await choose(page, "Versions", options.scope);
  if (options.format) await choose(page, "Format", /^JSON/);
  const file = await downloadFrom(page, dialog.getByRole("button", { name: "Export" }));
  await expect(dialog).toHaveCount(0);
  return { text: await downloadText(file), name: file.suggestedFilename() };
}

test("VO-01 the history and each version's detail show what applying it did to every channel, a failure with its reason", async ({
  page,
}) => {
  await open(page, "/settings/versions");
  await expect(versionRow(page, 47)).toBeVisible();
  // One line per version in the grid; the failure is flagged by its own label.
  await expect(versionRow(page, 47)).toContainText("1 updated");
  await expect(versionRow(page, 43)).toContainText("1 removed");
  await expect(versionRow(page, 36)).toContainText("1 started");
  await expect(versionRow(page, 41)).toContainText("No channel changed");
  await expect(versionRow(page, 37)).toContainText(/\d+ restarted/);
  await expect(versionRow(page, 44)).toContainText("1 failed");
  await expect(versionRow(page, 35)).toContainText("No outcomes recorded");

  // The detail lists the channels by label with the outcome in words and the engine's reason.
  await versionRow(page, 44).getByRole("gridcell").first().click();
  const detail = page.getByRole("region", { name: "Version 44" });
  const outcomes = detail.getByRole("list", { name: "Channel outcomes" });
  await expect(outcomes.getByRole("listitem")).toHaveCount(1);
  await expect(outcomes).toContainText("Failed");
  await expect(outcomes).toContainText("STA Compact results");
  await expect(outcomes).toContainText("ch-msg-sta");
  await expect(outcomes).toContainText(FAILURE);
  // Version 47 is active: the failure of 44 is history, in the past tense, with no call to act.
  await expect(
    detail.getByText(
      "When applied, this version became active, and a channel that failed kept its previous definition. Later versions have replaced it since.",
    ),
  ).toBeVisible();
  await expect(detail.getByText(/the next apply retries it/)).toHaveCount(0);
  await expect(detail.getByText("35 channels were unchanged.")).toBeVisible();

  // A removed channel is named by its label from the version before.
  await versionRow(page, 43).getByRole("gridcell").first().click();
  const removal = page.getByRole("region", { name: "Version 43" });
  await expect(removal.getByRole("list", { name: "Channel outcomes" })).toContainText(
    "Cobas legacy orders",
  );
  await expect(removal.getByRole("list", { name: "Channel outcomes" })).toContainText("Removed");

  // The startup import records none, and says so.
  await versionRow(page, 35).getByRole("gridcell").first().click();
  await expect(
    page
      .getByRole("region", { name: "Version 35" })
      .getByText(/No per-channel outcomes are recorded/),
  ).toBeVisible();
});

test("VO-02 a label or retention change is shown as updated in place, never as a restart", async ({
  page,
}) => {
  await openEditor(page, "/channels/ch-msg-dashboard/edit", "Dashboard feed");
  await fill(page, { Label: "Dashboard feed (clinic)" }, general(page));
  await saveAndApply(page).click();
  const dialog = reviewDialog(page);
  await expect(dialog.getByText("Updated in place: Dashboard feed (clinic)")).toBeVisible();
  await expect(dialog.getByText(/^Restarted/)).toHaveCount(0);
  await expect(dialog.getByText("No channel is restarted.")).toBeVisible();
  await dialog.getByRole("textbox", { name: /^Change note/ }).fill("Rename the feed");
  await dialog.getByRole("button", { name: "Apply version 48" }).click();
  await expect(page.getByText("Configuration version 48 applied")).toBeVisible();
  await expect(page.getByText("Updated: Dashboard feed (clinic). 35 unchanged.")).toBeVisible();
  await expect(page.getByText(/Restarted/)).toHaveCount(0);

  await openVersions(page);
  await expect(versionRow(page, 48)).toContainText("1 updated");
  await expect(versionRow(page, 48)).not.toContainText("restarted");
});

test("VO-03 a partial apply keeps the new version active, names the failed channel and its reason until dismissed", async ({
  page,
}) => {
  await openEditor(page, "/channels/ch-msg-sta/edit", "STA Compact results");
  await setChecked(page, "Detect duplicate messages", false);
  await page.evaluate(
    (reason) => window.__packieMock?.failNextApply("ch-msg-sta", reason),
    FAILURE,
  );
  await saveAndApply(page).click();
  const dialog = reviewDialog(page);
  await expect(dialog.getByText("Restarted (changed): STA Compact results")).toBeVisible();
  // Before applying, the dialog tells the two kinds of failure apart.
  await expect(dialog.getByText(/If the engine rejects the configuration/)).toBeVisible();
  await expect(dialog.getByText(/that channel keeps its previous definition/)).toBeVisible();
  await dialog.getByRole("textbox", { name: /^Change note/ }).fill("Stop detecting duplicates");
  await dialog.getByRole("button", { name: "Apply version 48" }).click();

  const notice = page.getByRole("region", { name: "Notifications" });
  await expect(
    notice.getByText("Configuration version 48 applied, but not everywhere"),
  ).toBeVisible();
  await expect(notice).toContainText("Version 48 is active");
  await expect(notice).toContainText("keeps the previous definition");
  await expect(notice).toContainText(`STA Compact results: ${FAILURE}`);

  // The new version is active and records the failure.
  await openVersions(page);
  await expect(versionRow(page, 48)).toContainText("Current");
  await expect(versionRow(page, 48)).toContainText("1 failed");
  await versionRow(page, 48).getByRole("gridcell").first().click();
  const detail = page.getByRole("region", { name: "Version 48" });
  await expect(detail.getByRole("list", { name: "Channel outcomes" })).toContainText(
    `STA Compact results ch-msg-sta: ${FAILURE}`,
  );
  // The active version's failure still waits for a fix, and says so in the present tense.
  await expect(
    detail.getByText(
      "The version is active. A channel that failed kept its previous definition; the next apply retries it.",
    ),
  ).toBeVisible();
  await expect(detail.getByText(/When applied, this version became active/)).toHaveCount(0);
  // A warning stays until the operator dismisses it.
  await expect(notice).toContainText(FAILURE);
  await notice.getByRole("button", { name: "Dismiss" }).click();
  await expect(notice.getByText("applied, but not everywhere")).toHaveCount(0);
});

test("VO-04 the rollback preview is the engine's plan: a retention-only difference updates in place, and it is off while the plan is not there", async ({
  page,
}) => {
  // Version 46 differs from the active 47 only in the dashboard feed's retention (30 → 7 days).
  await open(page, "/settings/versions?version=46");
  const detail = page.getByRole("region", { name: "Version 46" });
  await expect(detail.getByText("Retention › Keep for (days): 30 → 7")).toBeVisible();
  await expect(detail.getByText("Channels (1 changed)")).toBeVisible();

  // The engine cannot be asked: the dialog says so and Roll back stays off.
  await mockHandle(page, (mock) => mock.simulateOutage());
  await detail.getByRole("button", { name: "Roll back to this version…" }).click();
  const dialog = rollbackDialog(page, 46);
  await expect(dialog.getByText(/could not be loaded, so Roll back is off/)).toBeVisible();
  await expect(dialog.getByRole("button", { name: "Roll back to version 46" })).toBeDisabled();
  await mockHandle(page, (mock) => mock.endOutage());
  await dialog.getByRole("button", { name: "Try again" }).click();

  // The plan: updated in place, no restart.
  await expect(dialog.getByText("Updated in place: Dashboard feed")).toBeVisible();
  await expect(dialog.getByText(/Restarted/)).toHaveCount(0);
  await expect(dialog.getByText("No channel is restarted.")).toBeVisible();
  await expect(dialog.getByRole("button", { name: "Roll back to version 46" })).toBeEnabled();

  await dialog
    .getByRole("textbox", { name: /^Reason for the rollback/ })
    .fill("Keep the feed for 7 days again");
  await dialog.getByRole("button", { name: "Roll back to version 46" }).click();
  await expect(page.getByText("Rolled back to version 46")).toBeVisible();
  await expect(
    page.getByText("Saved as version 48. Updated: Dashboard feed. 35 unchanged."),
  ).toBeVisible();
  await expect(versionRow(page, 48)).toContainText("1 updated");
});

test("CR-01 Export change report (CSV) is one sheet with every version, its outcomes, its before and after values and every verification", async ({
  page,
}) => {
  await open(page, "/settings/versions");
  await expect(versionRow(page, 47)).toBeVisible();
  const { text, name } = await exportReport(page);
  expect(name).toMatch(/^packie-change-report-\d{4}-\d{2}-\d{2}\.csv$/);
  await expect(page.getByText("Change report saved")).toBeVisible();
  await expect(page.getByText(`13 versions saved to ${name}.`)).toBeVisible();

  const sheet = sheetOf(text);
  const kind = (value: string) => sheet.filter((row) => row.kind === value);
  // Who made it, and how to read the comparison.
  const [report] = kind("report");
  expect(report?.note).toContain(`generated`);
  expect(report?.note).toContain(`by ${ME}.`);
  expect(report?.note).toContain("before is the value in N-1, after is the value in N");

  // A row per version: account, time, note.
  const versions = kind("version");
  expect(versions.map((row) => row.version)).toEqual(
    Array.from({ length: 13 }, (_, i) => String(35 + i)),
  );
  const retire = versions.find((row) => row.version === "43");
  expect(retire).toMatchObject({
    applied_by: ME,
    note: "Retire the Cobas legacy orders channel",
    compared_with: "42",
  });
  expect(retire?.applied_at).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?Z$/);
  expect(versions.find((row) => row.version === "35")).toMatchObject({
    applied_by: "system",
    compared_with: "",
  });

  // Outcomes: every channel; a failure with its reason; a removed channel named by label.
  expect(kind("outcome").filter((row) => row.version === "47")).toHaveLength(36);
  expect(
    kind("outcome").find((row) => row.version === "44" && row.outcome === "failed"),
  ).toMatchObject({
    channel_id: "ch-msg-sta",
    channel: "STA Compact results",
    reason: FAILURE,
  });
  expect(
    kind("outcome").find((row) => row.version === "43" && row.outcome === "removed"),
  ).toMatchObject({
    channel_id: "ch-msg-cobas-legacy",
    channel: "Cobas legacy orders",
  });
  expect(kind("outcome").filter((row) => row.version === "35")).toHaveLength(0);

  // Changes: field, before and after as values, compared with the version before.
  expect(
    kind("change").find(
      (row) => row.version === "47" && row.field?.endsWith("Retention › Keep for (days)"),
    ),
  ).toMatchObject({
    field: "Channel Dashboard feed (ch-msg-dashboard) › Retention › Keep for (days)",
    before: "7",
    after: "30",
    compared_with: "46",
  });
  expect(
    kind("change").find((row) => row.version === "41" && row.field?.includes("queue warning")),
  ).toMatchObject({
    field: "Alert thresholds › Delivery queue warning",
    before: "500",
    after: "1000",
  });
  expect(
    kind("change").find(
      (row) => row.version === "43" && row.field?.includes("ch-msg-cobas-legacy"),
    ),
  ).toMatchObject({ before: "present", after: "removed" });
  expect(kind("change").filter((row) => row.version === "35")).toHaveLength(0);

  // Every verification: the signed-off versions carry their account, time and reference; 46 and 47 none.
  const signed = kind("verification");
  expect(signed.find((row) => row.version === "40")).toMatchObject({
    verified_by: "LAB\\mlopez",
    reference: "Validation record VR-2026-040",
  });
  expect(signed.find((row) => row.version === "40")?.verified_at).toMatch(/^\d{4}-\d{2}-\d{2}T/);
  expect(signed.map((row) => row.version)).toEqual(
    Array.from({ length: 11 }, (_, i) => String(35 + i)),
  );
  expect(signed.some((row) => row.version === "46" || row.version === "47")).toBe(false);
});

test("CR-02 the JSON report holds the same facts, nested by version, and can be limited to the open version", async ({
  page,
}) => {
  await open(page, "/settings/versions?version=44");
  await expect(page.getByRole("region", { name: "Version 44" })).toBeVisible();
  const { text, name } = await exportReport(page, { format: "JSON", scope: "Version 44 only" });
  expect(name).toMatch(/^packie-change-report-v44-\d{4}-\d{2}-\d{2}\.json$/);
  await expect(page.getByText(`1 version saved to ${name}.`)).toBeVisible();
  const report = JSON.parse(text);
  expect(report.generated_by).toBe(ME);
  expect(report.comparison_direction).toContain("N compared with N-1");
  expect(report.versions).toHaveLength(1);
  const [version] = report.versions;
  expect(version).toMatchObject({
    version: 44,
    applied_by: ME,
    note: "STA Compact baud rate set to 19200 after the cable swap",
    verifications: [{ verified_by: "LAB\\mlopez", reference: "Validation record VR-2026-044" }],
    comparison: {
      against: 43,
      changes: [
        {
          field: "Endpoint STA Compact coag (ep-sta) › Baud rate",
          before: "9600",
          after: "19200",
        },
      ],
    },
  });
  expect(version.outcomes).toHaveLength(36);
  expect(version.outcomes.find((o: { outcome: string }) => o.outcome === "failed")).toEqual({
    channel_id: "ch-msg-sta",
    channel: "STA Compact results",
    outcome: "failed",
    reason: FAILURE,
  });

  // All versions, as JSON: the same versions in the same order, the oldest without a comparison.
  const all = JSON.parse((await exportReport(page, { format: "JSON" })).text);
  expect(all.versions.map((v: { version: number }) => v.version)).toEqual(
    Array.from({ length: 13 }, (_, i) => 35 + i),
  );
  expect(all.versions[0].comparison).toBeNull();
  expect(all.versions[1].comparison.against).toBe(35);
});

test("CR-03 a verification added in the UI appears in the next export", async ({ page }) => {
  await open(page, "/settings/versions?version=47");
  const detail = page.getByRole("region", { name: "Version 47" });
  await detail.getByRole("button", { name: "Mark as verified…" }).click();
  const dialog = page.getByRole("alertdialog", { name: "Mark version 47 as verified?" });
  await dialog.getByRole("textbox", { name: /^Verification reference/ }).fill("VR-2026-099");
  await dialog.getByRole("button", { name: "Mark version 47 as verified" }).click();
  await expect(page.getByText("Version 47 marked as verified")).toBeVisible();

  const { text } = await exportReport(page);
  const verification = sheetOf(text).filter(
    (row) => row.kind === "verification" && row.version === "47",
  );
  expect(verification).toHaveLength(1);
  expect(verification[0]).toMatchObject({ verified_by: ME, reference: "VR-2026-099" });
  expect(verification[0]?.verified_at).toMatch(/^\d{4}-\d{2}-\d{2}T/);
});

test("CR-04 a blank verification reference is refused in the lab's own words", async ({ page }) => {
  await open(page, "/settings/versions?version=47");
  await page
    .getByRole("region", { name: "Version 47" })
    .getByRole("button", { name: "Mark as verified…" })
    .click();
  const dialog = page.getByRole("alertdialog", { name: "Mark version 47 as verified?" });
  await dialog.getByRole("button", { name: "Mark version 47 as verified" }).click();
  await expect(
    dialog.getByText("Enter a verification reference, such as a validation record number."),
  ).toBeVisible();
  await expect(dialog.getByText("Enter why you are making this change.")).toHaveCount(0);
  await expect(dialog.getByRole("textbox", { name: /^Verification reference/ })).toBeFocused();
  await expect(page.getByText("Version 47 marked as verified")).toHaveCount(0);
});

test("CR-05 cancelling the save is silent and a failure to read stays in the dialog", async ({
  page,
}) => {
  await open(page, "/settings/versions");
  await expect(versionRow(page, 47)).toBeVisible();
  await page.getByRole("button", { name: "Export change report…" }).click();
  const dialog = reportDialog(page);
  await expect(dialog).toContainText("account names, times, notes and references");
  await expect(dialog).toContainText("recorded in the audit log");
  // The engine cannot be reached: nothing is saved, the dialog says why and stays.
  await mockHandle(page, (mock) => mock.simulateOutage());
  await dialog.getByRole("button", { name: "Export" }).click();
  await expect(dialog.getByText("The change report could not be built")).toBeVisible();
  await expect(page.getByText("Change report saved")).toHaveCount(0);
  await mockHandle(page, (mock) => mock.endOutage());
  await dialog.getByRole("button", { name: "Cancel" }).click();
  await expect(dialog).toHaveCount(0);
});

test("CR-06 the export warns that notes and references can hold patient information", async ({
  page,
}) => {
  await open(page, "/settings/versions");
  await expect(versionRow(page, 47)).toBeVisible();
  await page.getByRole("button", { name: "Export change report…" }).click();
  const dialog = page.getByRole("dialog", { name: "Export change report" });
  const warning = dialog.getByRole("alert").filter({ hasText: "patient information" });
  await expect(warning).toContainText("The file may contain patient information");
  await expect(warning).toContainText("patient identifiers");
  await expect(warning).toContainText("lab's protected location");
});

test("CR-07 the export is recorded in the audit log, with the format's scope and the number of versions", async ({
  page,
}) => {
  await open(page, "/settings/versions");
  await expect(versionRow(page, 47)).toBeVisible();
  await exportReport(page);
  // Inside the app, so the mock engine keeps its state.
  await page.evaluate(() => {
    window.location.hash = "/audit?action=client_export";
  });
  const events = dataRows(page, "Audit events");
  await expect(events).toHaveCount(2);
  const [outcome, requested] = [events.nth(0), events.nth(1)];
  await expect(outcome.getByRole("gridcell").nth(COLUMN.action)).toHaveText(
    "Client export written",
  );
  await expect(outcome.getByRole("gridcell").nth(COLUMN.actor)).toHaveText("LAB\\jdoe");
  await expect(requested.getByRole("gridcell").nth(COLUMN.outcome)).toContainText("Requested");
  await requested.click();
  await expect(eventPanel(page)).toContainText("client_export");
  await expect(eventPanel(page)).toContainText("change_report");
  await expect(eventPanel(page)).toContainText("13");
  // The report itself is not in the log.
  await expect(eventPanel(page)).not.toContainText("VR-2026");
});

test("CR-08 with the audit log unavailable the export is refused and nothing is written", async ({
  page,
}) => {
  await open(page, "/settings/versions");
  await expect(versionRow(page, 47)).toBeVisible();
  await mockHandle(page, (mock) => mock.setAuditAvailable(false));
  await page.getByRole("button", { name: "Export change report…" }).click();
  const dialog = reportDialog(page);
  let downloaded = false;
  page.on("download", () => {
    downloaded = true;
  });
  await dialog.getByRole("button", { name: "Export" }).click();
  await expect(dialog.getByText("The change report could not be built")).toBeVisible();
  await expect(dialog).toContainText(/audit log/i);
  await expect(page.getByText("Change report saved")).toHaveCount(0);
  expect(downloaded).toBe(false);
  // Once the log can be written the export goes through.
  await mockHandle(page, (mock) => mock.setAuditAvailable(true));
  const file = await downloadFrom(page, dialog.getByRole("button", { name: "Export" }));
  expect(file.suggestedFilename()).toMatch(/^packie-change-report-/);
});
