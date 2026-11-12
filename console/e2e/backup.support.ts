import { expect, type Locator, type Page } from "@playwright/test";
import { fact as factIn } from "./operate.support";
import { dataRows, heading, type MockSwitches, navigateTo, open } from "./support";

/** The mock's backup directory (src/api/mock/backup.ts). */
export const BACKUP_DIRECTORY = "D:\\PACK-IE\\Backups";

/** The engine's next step for a stale backup, as Alerts and the panel both say it. */
export const STALE_NEXT_ACTION =
  "Check the backup directory (free space, permissions, mounted) and the last backup error (Settings → Backup or `packie backup status`), then run `packie backup create`. The alert clears after the next successful backup.";

/** Opens Settings → Backup and waits for its status. */
export async function openBackup(page: Page, switches: MockSwitches = {}): Promise<void> {
  await open(page, "/settings/backup", switches);
  await expect(heading(page, "Settings")).toBeVisible();
  await expect(page.getByRole("tab", { name: "Backup" })).toHaveAttribute("aria-selected", "true");
  await expect(fact(page, "Backup directory")).toBeVisible();
}

/** The value beside `label` in the status facts. */
export function fact(page: Page, label: string): Locator {
  return factIn(page.getByRole("main"), label);
}

/** The result region of a backup just taken. */
export const result = (page: Page): Locator => page.getByRole("region", { name: "Backup result" });

/** Starts a backup through the confirmation, as an operator does. */
export async function backUpNow(page: Page): Promise<void> {
  await page.getByRole("button", { name: "Back up now" }).click();
  const dialog = page.getByRole("alertdialog", { name: "Back up now?" });
  await dialog.getByRole("button", { name: "Start backup" }).click();
  await expect(dialog).toHaveCount(0);
}

/** Goes to a page by the main navigation, so the mock engine keeps its state. */
export async function goTo(page: Page, link: string, title: string): Promise<void> {
  await navigateTo(page, link);
  await expect(heading(page, title)).toBeVisible();
}

/** The Alerts page rows about stale backups. */
export function staleAlertRows(page: Page): Locator {
  return dataRows(page, "Alerts").filter({ hasText: /newest successful backup is from/ });
}
