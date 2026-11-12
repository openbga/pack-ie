import { screen, within } from "@testing-library/react";
import { vi } from "vitest";
import type { BackupAttempt, BackupStatus, BackupSummary } from "@/api/types";
import { stubApi } from "@/features/config/testFixtures";
import { renderRoutes } from "@/lib/test/router";
import { settingsRoutes } from "./testFixtures";

const HOUR = 3_600_000;
export const DIRECTORY = "/var/backups/packie";

export function hoursAgo(hours: number): string {
  return new Date(Date.now() - hours * HOUR).toISOString();
}

export function summary(overrides: Partial<BackupSummary> = {}): BackupSummary {
  return {
    directory: `${DIRECTORY}/packie-backup-20261107T020000Z`,
    createdAt: hoursAgo(0),
    files: 15,
    bytes: 7_340_032,
    messages: 4_200,
    configVersion: 9,
    auditHeadSeq: 311,
    durationMs: 1_800,
    ...overrides,
  };
}

export function success(hours: number, directory?: string): BackupAttempt {
  const at = hoursAgo(hours);
  const taken = summary({ createdAt: at, directory: directory ?? summary().directory });
  return { at, directory: taken.directory, ok: true, error: null, summary: taken };
}

export function status(overrides: Partial<BackupStatus> = {}): BackupStatus {
  const last = success(9);
  return {
    directory: DIRECTORY,
    schedule: { dailyAt: "02:00", keep: 14, nextAt: hoursAgo(-15) },
    staleAfterHours: 26,
    lastAttempt: last,
    lastSuccess: last,
    stale: false,
    ...overrides,
  };
}

export function backupApi(current: BackupStatus, overrides = {}) {
  return stubApi({
    getBackupStatus: vi.fn(async () => current),
    createBackup: vi.fn(async () => summary()),
    ...overrides,
  });
}

export async function openBackup(api: ReturnType<typeof backupApi>) {
  const view = renderRoutes(settingsRoutes, "/settings/backup", { api });
  await screen.findByText("Backup directory");
  return view;
}

/** Answers "Back up now?" with the start button. */
export async function startBackup(user: Awaited<ReturnType<typeof openBackup>>["user"]) {
  await user.click(screen.getByRole("button", { name: "Back up now" }));
  await user.click(
    within(await screen.findByRole("alertdialog")).getByRole("button", { name: "Start backup" }),
  );
}
