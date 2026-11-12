import { expect, test } from "@playwright/test";
import {
  BACKUP_DIRECTORY,
  backUpNow,
  fact,
  goTo,
  openBackup,
  result,
  STALE_NEXT_ACTION,
  staleAlertRows,
} from "./backup.support";
import { toast } from "./operate.support";
import { heading, mockHandle, open } from "./support";

const JUST_TAKEN = /\((just now|\d+ s ago|\d+ min ago)\)/;

test("the backup panel shows the schedule, the stale limit and the last success", async ({
  page,
}) => {
  await openBackup(page);
  await expect(fact(page, "State")).toHaveText("Up to date");
  await expect(fact(page, "Backup directory")).toHaveText(BACKUP_DIRECTORY);
  await expect(fact(page, "Schedule")).toContainText("Every day at 02:00, keeps the newest 14");
  await expect(fact(page, "Schedule")).toContainText("next run");
  await expect(fact(page, "Stale limit")).toHaveText(
    "Alerts after 26 hours without a successful backup",
  );
  await expect(fact(page, "Last success")).toContainText("9 h ago");
  await expect(fact(page, "Last success")).toContainText(`${BACKUP_DIRECTORY}\\packie-backup-`);
  // Nothing is wrong: no warning, no second "last attempt".
  await expect(page.getByText("Backups are stale")).toHaveCount(0);
  await expect(page.getByText("Last attempt", { exact: true })).toHaveCount(0);
});

test("Back up now confirms first, shows the progress, then the result and a fresh last success", async ({
  page,
}) => {
  await openBackup(page, { scenario: "slow" });
  await expect(fact(page, "Last success")).toContainText("9 h ago");

  // It says what will happen, and a cancel changes nothing.
  await page.getByRole("button", { name: "Back up now" }).click();
  const dialog = page.getByRole("alertdialog", { name: "Back up now?" });
  await expect(dialog).toContainText("message store, the audit log and the configuration");
  await expect(dialog).toContainText("Message traffic is not interrupted");
  await expect(dialog).toContainText("message content (PHI)");
  await expect(dialog).toContainText("Secret values are never included");
  await dialog.getByRole("button", { name: "Cancel" }).click();
  await expect(dialog).toHaveCount(0);
  await expect(result(page)).toHaveCount(0);

  await backUpNow(page);
  await expect(page.getByText("Backing up…")).toBeVisible();
  await expect(page.getByText(/cannot be cancelled/)).toBeVisible();
  await expect(page.getByRole("button", { name: "Back up now" })).toHaveAttribute(
    "aria-disabled",
    "true",
  );

  await expect(result(page)).toBeVisible();
  await expect(page.getByText("Backing up…")).toHaveCount(0);
  await expect(result(page)).toContainText("Backup complete");
  for (const label of [
    "Directory",
    "Taken",
    "Files",
    "Size",
    "Messages",
    "Configuration version",
    "Audit head sequence",
    "Duration",
  ]) {
    await expect(result(page).locator("dt", { hasText: new RegExp(`^${label}$`) })).toBeVisible();
  }
  await expect(result(page)).toContainText(`${BACKUP_DIRECTORY}\\packie-backup-`);
  await expect(toast(page, "Backup complete")).toBeVisible();

  // The status was read again: the last success is the backup just taken.
  await expect(fact(page, "Last success")).not.toContainText("9 h ago");
  await expect(fact(page, "Last success")).toContainText(JUST_TAKEN);
  await expect(fact(page, "State")).toHaveText("Up to date");
});

test("a stale backup is a warning on the panel and in Alerts until a backup succeeds", async ({
  page,
}) => {
  // Start away from the panel, so it reads the status afresh when it is opened.
  await open(page, "/alerts");
  await expect(heading(page, "Alerts")).toBeVisible();
  await expect(staleAlertRows(page)).toHaveCount(0);

  await mockHandle(page, (mock) => mock.makeBackupStale(40));
  await expect(staleAlertRows(page)).toHaveCount(1);
  await expect(staleAlertRows(page)).toContainText("Warning");
  await expect(staleAlertRows(page)).toContainText("Open");
  await expect(staleAlertRows(page)).toContainText(STALE_NEXT_ACTION);

  await goTo(page, "Settings", "Settings");
  await page.getByRole("tab", { name: "Backup" }).click();
  await expect(fact(page, "State")).toHaveText("Stale");
  const warning = page.getByRole("alert").filter({ hasText: "Backups are stale" });
  await expect(warning).toContainText("The limit is 26 hours.");
  await expect(warning).toContainText("1 d ago");
  await expect(warning).toContainText(STALE_NEXT_ACTION);

  await backUpNow(page);
  await expect(result(page)).toBeVisible();
  await expect(fact(page, "State")).toHaveText("Up to date");
  await expect(page.getByText("Backups are stale")).toHaveCount(0);

  // The alert clears with the next successful backup.
  await goTo(page, "Alerts", "Alerts");
  await expect(staleAlertRows(page).filter({ hasText: "Open" })).toHaveCount(0);
});

test("a failed backup shows the engine's words, retries, and is in the status as the last attempt", async ({
  page,
}) => {
  await openBackup(page);
  await mockHandle(page, (mock) => mock.failNextBackup("No space left on device"));

  await backUpNow(page);
  const failure = page.getByRole("alert").filter({ hasText: "The backup failed" });
  await expect(failure).toContainText("No space left on device");
  await expect(result(page)).toHaveCount(0);

  // The status says what happened, apart from the older success.
  await expect(fact(page, "Last attempt")).toContainText("Failed");
  await expect(fact(page, "Last attempt")).toContainText("No space left on device");
  await expect(fact(page, "Last success")).toContainText("9 h ago");

  // Try again works (the failure was only for the one attempt) without asking again.
  await failure.getByRole("button", { name: "Try again" }).click();
  await expect(result(page)).toBeVisible();
  await expect(page.getByText("The backup failed")).toHaveCount(0);
  await expect(fact(page, "Last success")).toContainText(JUST_TAKEN);
  await expect(page.getByText("Last attempt", { exact: true })).toHaveCount(0);
});

/** The mock engine's reported bootstrap file (src/api/mock/settings.ts). */
const BOOTSTRAP = "C:\\ProgramData\\PACK-IE\\bootstrap.json";

test("BR-01: restore is shown as commands for the engine's own bootstrap file, never as a button", async ({
  page,
}) => {
  await openBackup(page);
  const restore = page.getByRole("region", { name: "Restore" });
  await expect(restore).toContainText("never from the console");
  await expect(restore).toContainText("with the engine stopped");
  // The mock engine runs on Windows as a service: an administrator PowerShell, no sudo.
  await expect(restore).toContainText("PowerShell as administrator");
  await expect(restore).toContainText(
    `The commands use this engine's own bootstrap file, ${BOOTSTRAP}`,
  );
  const directory = (await fact(page, "Last success").locator("p").innerText()).trim();
  expect(directory).toContain(BACKUP_DIRECTORY);
  const quoted = /\s/.test(directory) ? `"${directory}"` : directory;
  await expect(restore.locator("div > code")).toHaveText([
    `packie --bootstrap ${BOOTSTRAP} backup verify ${quoted}`,
    `packie --bootstrap ${BOOTSTRAP} service stop`,
    `packie --bootstrap ${BOOTSTRAP} backup restore ${quoted} --yes`,
    `packie --bootstrap ${BOOTSTRAP} service start`,
  ]);
  await expect(restore).toContainText("Check the backup. This changes nothing.");
  await expect(restore).toContainText("pre-restore-<time>");
  await expect(page.getByRole("button", { name: /^restore/i })).toHaveCount(0);
});

test("BR-02: the procedure ends with start, a health check and the secrets reminder, in that order", async ({
  page,
}) => {
  await openBackup(page);
  const restore = page.getByRole("region", { name: "Restore" });
  const text = await restore.innerText();
  const order = [
    "backup verify",
    "service stop",
    "backup restore",
    "service start",
    "Check that the engine is running and healthy",
    "Secret values are not in a backup",
  ].map((part) => text.indexOf(part));
  expect(order.every((at) => at >= 0)).toBe(true);
  expect(order).toEqual([...order].sort((a, b) => a - b));

  const checks = restore.getByRole("list").last();
  await expect(checks).toContainText("Dashboard shows the channels running");
  await expect(checks).toContainText("config_rejected");
  await expect(checks).toContainText(
    "Settings → Versions shows the restored configuration version",
  );
  await expect(checks).toContainText("backup_restore");
  await expect(restore).toContainText("On a replacement computer, enter the missing secrets");
  await expect(restore.getByRole("link", { name: "Settings → Secrets" })).toBeVisible();
});

test("BR-03: a command is copied with a notice", async ({ page, context }) => {
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await openBackup(page);
  const restore = page.getByRole("region", { name: "Restore" });
  await restore.getByRole("button", { name: "Copy command: restore the backup" }).click();
  await expect(toast(page, "Command copied")).toBeVisible();
  const copied = await page.evaluate(() => navigator.clipboard.readText());
  expect(copied.startsWith(`packie --bootstrap ${BOOTSTRAP} backup restore `)).toBe(true);
  expect(copied.endsWith(" --yes")).toBe(true);
  expect(copied).toContain(BACKUP_DIRECTORY);
});

test("a backup is refused in words while the audit log cannot be written", async ({ page }) => {
  await openBackup(page);
  await mockHandle(page, (mock) => mock.setAuditAvailable(false));

  await backUpNow(page);
  const refusal = page.getByRole("alert").filter({ hasText: "The backup was not taken" });
  await expect(refusal).toContainText(
    "Not backed up: the audit log cannot be written, and backups are audited.",
  );
  await expect(result(page)).toHaveCount(0);
});
