import { expect, type Locator, type Page, test } from "@playwright/test";
import { expectNoAxeViolations } from "./a11y.support";
import { loadedCount } from "./audit.support";
import { fill, form, openEditor, pick, setChecked, text } from "./editors.support";
import { dataRows, eventPanel, loaded, mockHandle, navigateTo, open } from "./support";

// "Test connection" on an endpoint's unsaved or saved settings (docs/spec/diagnostics.md
// section 10): the mock probes by the definition itself, so the journeys drive it from the form.

const testButton = (page: Page): Locator => page.getByRole("button", { name: "Test connection" });
const stepsTable = (page: Page): Locator => page.getByRole("table", { name: "Test steps" });
const stepRow = (page: Page, step: string): Locator =>
  stepsTable(page)
    .getByRole("row")
    .filter({ has: page.getByRole("rowheader", { name: step }) });
const STALE = /These settings changed since the test/;
/** What the panel says before a folder is tested: it is the one kind that writes (diagnostics §10). */
const FOLDER_PROMISE =
  "The test reads the folder and counts the files that match, never their names. To prove the folder can be written it also creates one empty hidden file there and deletes it again. No message or instrument data is sent, the configuration is not changed and no test file is left behind. Each test is recorded in the audit log with your account.";
const READ_ONLY_PROMISE =
  "The test only opens, connects or reads. No message or instrument data is sent, the configuration is not changed and no test file is left behind. Each test is recorded in the audit log with your account.";

/** An in-app move to `route`: the mock keeps its state, unlike opening the address again. */
async function go(page: Page, route: string, title: string | RegExp): Promise<void> {
  await page.evaluate((hash) => {
    window.location.hash = hash;
  }, route);
  await loaded(page, title);
}

/** The newest configuration version, from Settings > Versions. */
async function activeVersion(page: Page): Promise<number> {
  await go(page, "/settings/versions", "Settings");
  const row = dataRows(page, "Configuration versions").first();
  return Number(/Version (\d+)/.exec((await row.textContent()) ?? "")?.[1]);
}

/** How many endpoint test events the audit log holds (the seeded history has none). */
async function auditedTests(page: Page): Promise<number> {
  await go(page, "/audit?action=endpoint_test", "Audit");
  const none = page.getByText("No audit events match the filters");
  await expect(dataRows(page, "Audit events").first().or(none)).toBeVisible();
  return (await none.isVisible()) ? 0 : loadedCount(page);
}

/** Leaves a form that has unsaved entries, as the operator does. */
async function leaveDirtyForm(page: Page): Promise<void> {
  await navigateTo(page, "Audit");
  await page
    .getByRole("alertdialog", { name: "Leave without saving?" })
    .getByRole("button", { name: "Leave without saving" })
    .click();
  await loaded(page, "Audit");
}

test("TE-01 an unsaved endpoint is tested with its steps and timings, and nothing is saved", async ({
  page,
}) => {
  await open(page, "/settings/versions");
  await loaded(page, "Settings");
  const version = await activeVersion(page);

  // A new endpoint that cannot be reached: the failure is at the connect step, after a good resolve.
  await go(page, "/endpoints/new", "New endpoint");
  await pick(page, "Kind", "TCP client");
  await fill(page, { Label: "Unreachable LIS", Host: "unreachable.example", Port: "2575" });
  await testButton(page).click();
  await expect(page.getByText("Test failed")).toBeVisible();
  await expect(page.getByText("The connect step failed: connection timed out.")).toBeVisible();
  await expect(stepsTable(page).getByRole("columnheader")).toHaveText([
    "Step",
    "Result",
    "What was found",
    "Time",
  ]);
  await expect(stepRow(page, "Resolve host")).toContainText("Passed");
  await expect(stepRow(page, "Resolve host")).toContainText(/Resolved to 10\.0\.\d+\.\d+/);
  await expect(stepRow(page, "Resolve host")).toContainText(/\d+ ms/);
  await expect(stepRow(page, "Connect")).toContainText("Failed");
  await expect(stepRow(page, "Connect")).toContainText("Connection timed out");
  await expect(stepRow(page, "Connect")).toContainText("5.0 s");
  await expect(stepRow(page, "Connect")).toContainText(/Nothing answered in time/);
  await expect(stepsTable(page).getByRole("row")).toHaveCount(3);

  // The typed entries are the operator's own: leaving asks, and nothing was saved.
  await leaveDirtyForm(page);
  await go(page, "/endpoints", "Endpoints");
  await expect(page.getByText("Unreachable LIS")).toHaveCount(0);
  expect(await activeVersion(page)).toBe(version);

  // An existing, healthy endpoint, tested as it is: the form stays clean.
  await go(page, "/endpoints/ep-lis-out/edit", "LIS HL7 out");
  await testButton(page).click();
  await expect(page.getByText("Test passed")).toBeVisible();
  await expect(
    page.getByText(/^lis\.lab\.local:2575 accepted a connection in \d+ ms\.$/),
  ).toBeVisible();
  await expect(stepRow(page, "Resolve host")).toContainText("Passed");
  await expect(stepRow(page, "Connect")).toContainText(
    /Connected to 10\.0\.\d+\.\d+:2575 in \d+ ms, then closed/,
  );
  // Leaving needed no question, and the configuration is the version it was.
  await navigateTo(page, "Endpoints");
  await loaded(page, "Endpoints");
  await expect(page.getByRole("alertdialog")).toHaveCount(0);
  expect(await activeVersion(page)).toBe(version);
});

test("TE-02 a healthy edited endpoint passes, and a later edit marks the result stale", async ({
  page,
}) => {
  await openEditor(page, "/endpoints/ep-lis-backup/edit", "LIS HL7 out (backup)");
  await fill(page, { Port: "2576" });
  await testButton(page).click();
  await expect(page.getByText("Test passed")).toBeVisible();
  await expect(
    page.getByText(/^10\.0\.4\.21:2576 accepted a connection in \d+ ms\.$/),
  ).toBeVisible();
  await expect(stepRow(page, "Resolve host")).toContainText("Resolved to 10.0.4.21");
  await expect(stepRow(page, "Connect")).toContainText("Passed");
  await expect(page.getByText(STALE)).toHaveCount(0);

  // The result stays readable, marked as of the older settings.
  await fill(page, { Host: "10.0.4.22" });
  await expect(page.getByText(STALE)).toBeVisible();
  await expect(stepsTable(page)).toBeVisible();

  // Running it again replaces the result.
  await testButton(page).click();
  await expect(
    page.getByText(/^10\.0\.4\.22:2576 accepted a connection in \d+ ms\.$/),
  ).toBeVisible();
  await expect(page.getByText(STALE)).toHaveCount(0);
});

test("TE-03 settings that are incomplete or rejected are marked on the form, and no test runs", async ({
  page,
}) => {
  await open(page, "/audit?action=endpoint_test");
  await loaded(page, "Audit");
  const before = await auditedTests(page);

  // Missing required settings: the form says what, the engine is not asked.
  await go(page, "/endpoints/new", "New endpoint");
  await pick(page, "Kind", "TCP client");
  await testButton(page).click();
  await expect(page.getByText("Fill in the required settings first")).toBeVisible();
  await expect(form(page).getByText("Enter host.")).toBeVisible();
  await expect(stepsTable(page)).toHaveCount(0);

  // A listener the network can reach needs an allowlist: the engine's rule, before any test.
  await pick(page, "Kind", "TCP listener");
  await fill(page, { Label: "Bench listener", Port: "4999" });
  await testButton(page).click();
  await expect(page.getByText("Settings rejected: no test ran")).toBeVisible();
  await expect(page.getByText(/rejected these settings before any test ran/)).toBeVisible();
  await expect(form(page).getByRole("group", { name: "Peer allowlist" })).toContainText(
    "List the peers allowed to connect",
  );
  await expect(stepsTable(page)).toHaveCount(0);
  await expect(page.getByText("Test passed")).toHaveCount(0);

  // Fixing the field and testing again works, on the same page.
  await setChecked(page, "Allow any peer to connect", true);
  await testButton(page).click();
  await expect(page.getByText("Test passed")).toBeVisible();
  await expect(stepRow(page, "Bind address and port")).toContainText("Passed");

  // Only the tests that reached the engine are audited: the refusal and the pass, a request each.
  await leaveDirtyForm(page);
  expect(await auditedTests(page)).toBe(before + 4);
  const newest = dataRows(page, "Audit events");
  await expect(newest.nth(0)).toContainText("Succeeded");
  await expect(newest.nth(2)).toContainText("Rejected");
});

test("TE-04 every test is recorded in the audit log as an endpoint test, without secrets", async ({
  page,
}) => {
  await open(page, "/audit?action=endpoint_test");
  await loaded(page, "Audit");
  const before = await auditedTests(page);

  await go(page, "/endpoints/ep-reflab-sftp/edit", "Reference lab SFTP");
  await testButton(page).click();
  await expect(page.getByText("Test passed")).toBeVisible();
  await expect(stepsTable(page).getByRole("row")).toHaveCount(8);
  await fill(page, { Host: "nosuchhost.invalid" });
  await testButton(page).click();
  await expect(page.getByText("Test failed")).toBeVisible();
  await expect(stepRow(page, "Resolve host")).toContainText("Failed");
  await expect(stepsTable(page).getByRole("row")).toHaveCount(3);

  await leaveDirtyForm(page);
  expect(await auditedTests(page)).toBe(before + 4);
  const rows = dataRows(page, "Audit events");
  for (const row of [0, 1, 2, 3]) {
    await expect(rows.nth(row).getByRole("gridcell").nth(1)).toHaveText("LAB\\jdoe");
    await expect(rows.nth(row)).toContainText("Endpoint tested");
  }
  await expect(rows.nth(0)).toContainText("Succeeded");
  await expect(rows.nth(1)).toContainText("Requested");
  await rows.nth(0).click();
  const panel = eventPanel(page);
  await expect(panel).toContainText("nosuchhost.invalid:22");
  await expect(panel).toContainText("sftp");
  await expect(panel).not.toContainText("reflab-sftp-password");
});

test("TE-05 the endpoint page tests the saved settings and explains a port a channel holds", async ({
  page,
}) => {
  await open(page, "/endpoints/ep-lis-out");
  await loaded(page, "LIS HL7 out");
  await expect(page.getByText(/is reported as in use/)).toHaveCount(0);
  await page.getByRole("button", { name: "Test connection" }).click();
  await expect(page.getByText("Test passed")).toBeVisible();
  await expect(stepRow(page, "Connect")).toContainText("Passed");

  // A listener a running channel holds: in use by design, and the page says so.
  await go(page, "/endpoints/ep-a-xn3000", "Sysmex XN-3000 (bridge)");
  await expect(
    page.getByText(
      "A port this engine already holds is reported as in use: the endpoint is working if a channel is running it.",
    ),
  ).toBeVisible();
  await testButton(page).click();
  await expect(page.getByText("Test failed")).toBeVisible();
  await expect(stepRow(page, "Bind address and port")).toContainText(
    "Port 4001 on 10.0.4.5 is already in use (by this engine or another program)",
  );
  await expect(
    page.getByText(/If a channel is running this endpoint, it is working/),
  ).toBeVisible();

  // A serial port a running channel holds is reported without being opened.
  await go(page, "/endpoints/ep-a-au5800", "AU5800 chem analyzer (COM4)");
  await testButton(page).click();
  await expect(stepRow(page, "Open port")).toContainText(/In use by this engine \(channel .+\)/);
  await expect(stepRow(page, "Open port")).toContainText("under 1 ms");
  await expect(stepsTable(page).getByRole("row")).toHaveCount(2);

  // A serial port nothing holds opens.
  await go(page, "/endpoints/ep-a-d100", "D-100 HbA1c (COM12)");
  await expect(page.getByText(/is reported as in use/)).toHaveCount(0);
  await testButton(page).click();
  await expect(page.getByText("Test passed")).toBeVisible();
  await expect(stepRow(page, "Open port")).toContainText(
    "COM12 opened at 9600 baud, 8N1, no flow control",
  );
});

test("TE-06 the panel says the test sends nothing, and has no axe violations passed or failed", async ({
  page,
}) => {
  await openEditor(page, "/endpoints/ep-qc-in/edit", "QC export folder");
  await expect(page.getByText(FOLDER_PROMISE)).toBeVisible();
  await expectNoAxeViolations(page);
  await testButton(page).click();
  await expect(page.getByText("Test passed")).toBeVisible();
  await expect(stepRow(page, "Write test")).toContainText("Passed");
  await expectNoAxeViolations(page);

  await fill(page, { "Folder path": "D:\\LabData\\readonly\\QC" });
  await testButton(page).click();
  await expect(page.getByText("Test failed")).toBeVisible();
  await expect(page.getByText("The write test step failed: permission denied.")).toBeVisible();
  await expect(stepRow(page, "Write test")).toContainText("Failed");
  await expect(stepRow(page, "Write test")).toContainText(
    /account the engine runs as must be able to create files/,
  );
  await expectNoAxeViolations(page);
});

test("TE-07 an engine that cannot be reached shows an error with Try again", async ({ page }) => {
  await openEditor(page, "/endpoints/ep-lis-out/edit", "LIS HL7 out");
  await mockHandle(page, (mock) => mock.simulateOutage());
  await testButton(page).click();
  const failure = page.getByRole("alert").filter({ hasText: "The test could not run" });
  await expect(failure).toBeVisible();
  await expect(failure.getByText(/Support reference/)).toBeVisible();
  await expect(stepsTable(page)).toHaveCount(0);

  await mockHandle(page, (mock) => mock.endOutage());
  await failure.getByRole("button", { name: "Try again" }).click();
  await expect(page.getByText("Test passed")).toBeVisible();
  await expect(failure).toHaveCount(0);
  // The form was never changed by any of it.
  await expect(text(page, "Host")).toHaveValue("lis.lab.local");
});

test("TE-08 a folder test says before it runs that it writes and removes one hidden file; the other kinds keep the read-only promise", async ({
  page,
}) => {
  // The saved folder on its page, before and after the test.
  await open(page, "/endpoints/ep-archive");
  await expect(
    page.getByRole("heading", { level: 1, name: "Results archive folder" }),
  ).toBeVisible();
  await expect(page.getByText(FOLDER_PROMISE)).toBeVisible();
  await expect(page.getByText("The test only opens, connects or reads.")).toHaveCount(0);
  await testButton(page).click();
  await expect(page.getByText("Test passed")).toBeVisible();
  await expect(stepRow(page, "Write test")).toContainText(
    "empty hidden test file was created and deleted",
  );
  // The promise is still on screen next to the result that says the file was written.
  await expect(page.getByText(FOLDER_PROMISE)).toBeVisible();

  // A TCP client's editor keeps the read-only promise, and a folder's editor the folder one.
  await go(page, "/endpoints/ep-lis-out/edit", /LIS/);
  await expect(page.getByText(READ_ONLY_PROMISE)).toBeVisible();
  await expect(page.getByText(/creates one empty hidden file/)).toHaveCount(0);
  await go(page, "/endpoints/ep-qc-in/edit", "QC export folder");
  await expect(page.getByText(FOLDER_PROMISE)).toBeVisible();
});
