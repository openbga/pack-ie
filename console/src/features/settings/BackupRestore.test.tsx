import { screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { BackupStatus, EngineSettings } from "@/api/types";
import { engineFailure } from "@/features/config/testFixtures";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { backupApi, openBackup, status, success, summary } from "./backupTestFixtures";
import { BOOTSTRAP, engineInfo, engineSettings } from "./testFixtures";

describe("Settings > Backup > Restore", () => {
  /** An engine that reports where it runs and which bootstrap file it started with. */
  function restoreApi(
    settings: Partial<EngineSettings> = {},
    platform = "linux-x86_64",
    current: BackupStatus = status(),
  ) {
    return backupApi(current, {
      getEngineSettings: vi.fn(async () => engineSettings(settings)),
      getEngineInfo: vi.fn(async () => engineInfo(platform)),
    });
  }

  const restoreRegion = async () => {
    const restore = screen.getByRole("region", { name: "Restore" });
    await within(restore).findByText(
      /The commands use this engine's own bootstrap file|cannot tell/,
    );
    return restore;
  };
  /** The commands to copy, in order: each sits in its own box, not in a sentence. */
  const commandsIn = (restore: HTMLElement) =>
    [...restore.querySelectorAll("div > code")].map((code) => code.textContent);

  it("gives the commands for the engine's own bootstrap file, ending in start, health check and secrets", async () => {
    await openBackup(restoreApi());
    const restore = await restoreRegion();
    expect(restore.textContent).toContain("never from the console");
    expect(restore.textContent).toContain("with the engine stopped");
    expect(within(restore).getByText(BOOTSTRAP, { selector: "code" })).toBeTruthy();
    const directory = summary().directory;
    expect(commandsIn(restore)).toEqual([
      `packie --bootstrap ${BOOTSTRAP} backup verify ${directory}`,
      `sudo packie --bootstrap ${BOOTSTRAP} service stop`,
      `sudo packie --bootstrap ${BOOTSTRAP} backup restore ${directory} --yes`,
      `sudo packie --bootstrap ${BOOTSTRAP} service start`,
    ]);
    expect(restore.textContent).toContain("pre-restore-<time>");
    // The steps run in the order of the page: verify, stop, restore, start, then the checks.
    const text = restore.textContent ?? "";
    for (const [earlier, later] of [
      ["backup verify", "service stop"],
      ["service stop", "backup restore"],
      ["backup restore", "service start"],
      ["service start", "Check that the engine is running and healthy"],
      ["Check that the engine is running and healthy", "Secret values are not in a backup"],
    ] as const) {
      expect(text.indexOf(earlier), `${earlier} before ${later}`).toBeLessThan(text.indexOf(later));
    }
  });

  it("says what to look at once the engine is back, and where to enter the secrets", async () => {
    await openBackup(restoreApi());
    const restore = await restoreRegion();
    expect(within(restore).getByRole("link", { name: "Dashboard" }).getAttribute("href")).toBe(
      "/dashboard",
    );
    expect(restore.textContent).toContain("shows the channels running");
    expect(within(restore).getByText("config_rejected", { selector: "code" })).toBeTruthy();
    expect(
      within(restore).getByRole("link", { name: "Settings → Versions" }).getAttribute("href"),
    ).toBe("/settings/versions");
    expect(within(restore).getByText("backup_restore", { selector: "code" })).toBeTruthy();
    expect(
      within(restore).getByRole("link", { name: "Settings → Secrets" }).getAttribute("href"),
    ).toBe("/settings/secrets");
    expect(restore.textContent).toContain("Secret values are not in a backup");
    expect(restore.textContent).toContain("On a replacement computer");
  });

  it("needs no sudo in an administrator terminal on Windows, and quotes paths with spaces", async () => {
    await openBackup(
      restoreApi(
        { bootstrapPath: "C:\\Program Data\\PACK-IE\\bootstrap.json" },
        "windows-x86_64",
        status({ lastSuccess: success(2, "D:\\PACK-IE Backups\\packie-backup-1") }),
      ),
    );
    const restore = await restoreRegion();
    const boot = '"C:\\Program Data\\PACK-IE\\bootstrap.json"';
    expect(commandsIn(restore)).toEqual([
      `packie --bootstrap ${boot} backup verify "D:\\PACK-IE Backups\\packie-backup-1"`,
      `packie --bootstrap ${boot} service stop`,
      `packie --bootstrap ${boot} backup restore "D:\\PACK-IE Backups\\packie-backup-1" --yes`,
      `packie --bootstrap ${boot} service start`,
    ]);
    expect(restore.textContent).toContain("PowerShell as administrator");
  });

  it("mentions the per-user agent on macOS only", async () => {
    const view = await openBackup(restoreApi({}, "macos-aarch64"));
    const mac = await restoreRegion();
    expect(mac.textContent).toContain("packie service install --user");
    expect(mac.textContent).toContain("add --user to the service commands");
    view.unmount();
    await openBackup(restoreApi({}, "linux-x86_64"));
    expect((await restoreRegion()).textContent).not.toContain("--user");
  });

  it("stops and starts an engine run in a terminal there, not as a service", async () => {
    await openBackup(restoreApi({ hosting: "console" }));
    const restore = await restoreRegion();
    expect(restore.textContent).toContain("press Ctrl+C in the terminal");
    const directory = summary().directory;
    expect(commandsIn(restore)).toEqual([
      `packie --bootstrap ${BOOTSTRAP} backup verify ${directory}`,
      `sudo packie --bootstrap ${BOOTSTRAP} backup restore ${directory} --yes`,
      `packie --bootstrap ${BOOTSTRAP} run`,
    ]);
  });

  it("says how to find the bootstrap file when the engine does not report it, instead of a default", async () => {
    await openBackup(restoreApi({ bootstrapPath: null }));
    const restore = await restoreRegion();
    expect(restore.textContent).toContain("cannot tell which bootstrap file this engine uses");
    expect(restore.textContent).toContain("may belong to another installation");
    const directory = summary().directory;
    expect(commandsIn(restore)).toEqual([
      "packie service status",
      `packie --bootstrap <bootstrap-file> backup verify ${directory}`,
      "sudo packie --bootstrap <bootstrap-file> service stop",
      `sudo packie --bootstrap <bootstrap-file> backup restore ${directory} --yes`,
      "sudo packie --bootstrap <bootstrap-file> service start",
    ]);
    expect(restore.textContent).not.toContain('config/bootstrap.json"');
  });

  it("treats an engine that cannot say as one that does not report the file", async () => {
    await openBackup(
      backupApi(status(), {
        getEngineSettings: vi.fn(async () => {
          throw engineFailure("unavailable", "no settings");
        }),
      }),
    );
    const restore = await restoreRegion();
    expect(restore.textContent).toContain("cannot tell which bootstrap file");
    expect(within(restore).getByText("packie service status", { selector: "code" })).toBeTruthy();
  });

  it("uses a placeholder without a last success", async () => {
    await openBackup(
      restoreApi({}, "linux-x86_64", status({ lastAttempt: null, lastSuccess: null, stale: true })),
    );
    const restore = await restoreRegion();
    expect(commandsIn(restore)[0]).toBe(
      `packie --bootstrap ${BOOTSTRAP} backup verify <directory>`,
    );
  });

  it("copies a command, and says so", async () => {
    const { user } = await openBackup(restoreApi());
    const restore = await restoreRegion();
    await user.click(
      within(restore).getByRole("button", { name: "Copy command: stop the engine" }),
    );
    expect(await navigator.clipboard.readText()).toBe(
      `sudo packie --bootstrap ${BOOTSTRAP} service stop`,
    );
    const notices = screen.getByRole("region", { name: "Notifications" });
    expect(within(notices).getByText("Command copied")).toBeTruthy();
  });

  it("has no restore button: restore is never offered in the console", async () => {
    await openBackup(restoreApi());
    await restoreRegion();
    expect(screen.queryByRole("button", { name: /^restore/i })).toBeNull();
  });

  it("has no axe violations", async () => {
    const { container } = await openBackup(restoreApi());
    await restoreRegion();
    await expectNoA11yViolations(container, { wholePage: true });
  });
});
