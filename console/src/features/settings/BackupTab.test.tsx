import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { BackupAttempt, BackupSummary } from "@/api/types";
import { engineFailure } from "@/features/config/testFixtures";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { renderRoutes } from "@/lib/test/router";
import {
  backupApi,
  DIRECTORY,
  hoursAgo,
  openBackup,
  startBackup,
  status,
  success,
  summary,
} from "./backupTestFixtures";
import { fact, settingsRoutes } from "./testFixtures";

describe("Settings > Backup status", () => {
  it("says a current backup is up to date, with schedule, limit and last success", async () => {
    await openBackup(backupApi(status()));
    expect(within(fact("State")).getByText("Up to date")).toBeTruthy();
    expect(fact("Backup directory").textContent).toBe(DIRECTORY);
    expect(fact("Schedule").textContent).toMatch(
      /^Every day at 02:00, keeps the newest 14 · next run /,
    );
    expect(fact("Stale limit").textContent).toBe(
      "Alerts after 26 hours without a successful backup",
    );
    expect(fact("Last success").textContent).toContain("9 h ago");
    expect(fact("Last success").textContent).toContain(`${DIRECTORY}/packie-backup-`);
    expect(screen.queryByText("Backups are stale")).toBeNull();
    // The last attempt is the last success: it is not told twice.
    expect(screen.queryByText("Last attempt", { selector: "dt" })).toBeNull();
  });

  it("warns when stale, naming the age, the limit and the engine's next step", async () => {
    const old = success(40);
    await openBackup(backupApi(status({ stale: true, lastSuccess: old, lastAttempt: old })));
    expect(within(fact("State")).getByText("Stale")).toBeTruthy();
    const alert = screen.getByRole("alert");
    expect(within(alert).getByText("Backups are stale")).toBeTruthy();
    expect(alert.textContent).toContain(
      "The newest successful backup was 1 d ago. The limit is 26 hours.",
    );
    expect(alert.textContent).toContain(
      "Check the backup directory (free space, permissions, mounted) and the last backup error (Settings → Backup or `packie backup status`), then run `packie backup create`. The alert clears after the next successful backup.",
    );
  });

  it("says when nothing has succeeded, with no directory, schedule or limit", async () => {
    await openBackup(
      backupApi(
        status({
          directory: null,
          schedule: null,
          staleAfterHours: null,
          lastAttempt: null,
          lastSuccess: null,
        }),
      ),
    );
    expect(within(fact("State")).getByText("No backup yet")).toBeTruthy();
    expect(fact("Backup directory").textContent).toContain("Not set");
    expect(fact("Backup directory").textContent).toContain("backup.directory in bootstrap.json");
    expect(fact("Schedule").textContent).toBe("No schedule");
    expect(fact("Stale limit").textContent).toBe("Not checked");
    expect(fact("Last success").textContent).toBe("None yet");
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("tells a stale state with no success at all in words", async () => {
    await openBackup(backupApi(status({ stale: true, lastAttempt: null, lastSuccess: null })));
    expect(within(fact("State")).getByText("No backup yet")).toBeTruthy();
    expect(screen.getByRole("alert").textContent).toContain(
      "No backup has succeeded since the engine started. The limit is 26 hours.",
    );
  });

  it("shows a failed last attempt with the error, apart from the last success", async () => {
    const failed: BackupAttempt = {
      at: hoursAgo(1),
      directory: DIRECTORY,
      ok: false,
      error: "No space left on device",
      summary: null,
    };
    await openBackup(backupApi(status({ lastAttempt: failed })));
    const attempt = fact("Last attempt");
    expect(within(attempt).getByText("Failed")).toBeTruthy();
    expect(attempt.textContent).toContain("1 h ago");
    expect(attempt.textContent).toContain("No space left on device");
    expect(fact("Last success").textContent).toContain("9 h ago");
    expect(within(fact("State")).getByText("Up to date")).toBeTruthy();
  });

  it("shows loading, then an error with a retry that works", async () => {
    const read = vi
      .fn()
      .mockRejectedValueOnce(engineFailure("engine_unavailable", "The engine is not running."))
      .mockResolvedValue(status());
    const { user } = renderRoutes(settingsRoutes, "/settings/backup", {
      api: backupApi(status(), { getBackupStatus: read }),
    });
    expect(screen.getByText("Loading backup status…")).toBeTruthy();
    expect(await screen.findByText("The backup status could not be loaded")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByText("Backup directory")).toBeTruthy();
  });
});

describe("Settings > Backup > Back up now", () => {
  it("asks first, and does nothing when cancelled", async () => {
    const api = backupApi(status());
    const { user } = await openBackup(api);
    await user.click(screen.getByRole("button", { name: "Back up now" }));
    const dialog = await screen.findByRole("alertdialog", { name: "Back up now?" });
    expect(dialog.textContent).toContain("message store, the audit log and the configuration");
    expect(dialog.textContent).toContain("Message traffic is not interrupted");
    expect(dialog.textContent).toContain("recorded in the audit log");
    expect(dialog.textContent).toContain("message content (PHI)");
    expect(dialog.textContent).toContain("encrypted volume");
    expect(dialog.textContent).toContain("Secret values are never included");
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
    expect(api.createBackup).not.toHaveBeenCalled();
    expect(screen.queryByRole("region", { name: "Backup result" })).toBeNull();
  });

  it("shows progress while it runs, then the result, a notice and a refreshed status", async () => {
    let finish: (value: BackupSummary) => void = () => undefined;
    const taken = summary();
    const read = vi.fn(async () => status());
    const api = backupApi(status(), {
      getBackupStatus: read,
      createBackup: vi.fn(() => new Promise<BackupSummary>((resolve) => (finish = resolve))),
    });
    const { user } = await openBackup(api);
    await user.click(screen.getByRole("button", { name: "Back up now" }));
    const dialog = await screen.findByRole("alertdialog");
    await user.click(within(dialog).getByRole("button", { name: "Start backup" }));

    expect(await screen.findByText("Backing up…")).toBeTruthy();
    expect(screen.getByText(/cannot be cancelled/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Back up now" }).getAttribute("aria-disabled")).toBe(
      "true",
    );
    expect(screen.queryByRole("region", { name: "Backup result" })).toBeNull();

    finish(taken);
    const result = await screen.findByRole("region", { name: "Backup result" });
    const facts = within(result);
    expect(facts.getByText("Backup complete")).toBeTruthy();
    expect(result.textContent).toContain(taken.directory);
    expect(result.textContent).toContain("15");
    expect(result.textContent).toContain("7.0 MB");
    expect(result.textContent).toContain("4,200");
    expect(result.textContent).toContain("Configuration version9");
    expect(result.textContent).toContain("Audit head sequence311");
    expect(result.textContent).toContain("1.8 s");
    expect(await screen.findByText("15 files, 7.0 MB")).toBeTruthy();
    expect(screen.queryByText("Backing up…")).toBeNull();
    await waitFor(() => expect(read.mock.calls.length).toBeGreaterThan(1));
  });

  it("says when the state had no message store, configuration or audit log", async () => {
    const api = backupApi(status(), {
      createBackup: vi.fn(async () =>
        summary({ messages: null, configVersion: null, auditHeadSeq: null }),
      ),
    });
    const { user } = await openBackup(api);
    await startBackup(user);
    const result = await screen.findByRole("region", { name: "Backup result" });
    expect(result.textContent).toContain("None: no message store");
    expect(result.textContent).toContain("No audit log");
  });

  it("shows the engine's words on failure and retries without asking again", async () => {
    const createBackup = vi
      .fn()
      .mockRejectedValueOnce(
        engineFailure("internal", "The backup failed: No space left on device"),
      )
      .mockResolvedValue(summary());
    const read = vi.fn(async () => status());
    const { user } = await openBackup(backupApi(status(), { createBackup, getBackupStatus: read }));
    await startBackup(user);
    expect(await screen.findByText("The backup failed")).toBeTruthy();
    // The failed attempt is in the status, so the status is read again.
    await waitFor(() => expect(read).toHaveBeenCalledTimes(2));
    expect(screen.getByText("The backup failed: No space left on device")).toBeTruthy();
    expect(screen.getByText("test-ref")).toBeTruthy();
    expect(screen.queryByRole("region", { name: "Backup result" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByRole("region", { name: "Backup result" })).toBeTruthy();
    expect(createBackup).toHaveBeenCalledTimes(2);
    expect(screen.queryByText("The backup failed")).toBeNull();
  });

  it("is refused in words while the audit log cannot be written", async () => {
    const createBackup = vi.fn(async () => {
      throw engineFailure("unavailable", "The audit log cannot be written.");
    });
    const { user } = await openBackup(backupApi(status(), { createBackup }));
    await startBackup(user);
    expect(await screen.findByText("The backup was not taken")).toBeTruthy();
    expect(
      screen.getByText(/Not backed up: the audit log cannot be written, and backups are audited\./),
    ).toBeTruthy();
    expect(screen.queryByText(/not available in this version/)).toBeNull();
  });
});

describe("Settings > Backup accessibility", () => {
  it("has no axe violations in the status, with the stale warning, the dialog and the result", async () => {
    const old = success(40);
    const { user, container } = await openBackup(
      backupApi(status({ stale: true, lastSuccess: old, lastAttempt: old })),
    );
    await expectNoA11yViolations(container, { wholePage: true });
    await user.click(screen.getByRole("button", { name: "Back up now" }));
    const dialog = await screen.findByRole("alertdialog");
    await expectNoA11yViolations(dialog);
    await user.click(within(dialog).getByRole("button", { name: "Start backup" }));
    await screen.findByRole("region", { name: "Backup result" });
    await expectNoA11yViolations(container, { wholePage: true });
  });
});
