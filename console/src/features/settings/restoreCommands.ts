import type { EngineSettings } from "@/api/types";

/** Where the engine runs, as far as it decides how the service is controlled. */
export type RestorePlatform = "windows" | "macos" | "linux" | null;

/** The platform an engine reports (`windows-x86_64`, `macos-aarch64`, `linux-x86_64`). */
export function restorePlatform(reported: string | null | undefined): RestorePlatform {
  if (reported === null || reported === undefined) return null;
  if (/win/i.test(reported)) return "windows";
  if (/mac|darwin/i.test(reported)) return "macos";
  if (/linux/i.test(reported)) return "linux";
  return null;
}

/** A path as the shell needs it: quoted when it holds a space. */
function shellPath(path: string): string {
  return /\s/.test(path) ? `"${path}"` : path;
}

/** What the bootstrap argument reads when the console does not know the file. */
export const BOOTSTRAP_PLACEHOLDER = "<bootstrap-file>";
const DIRECTORY_PLACEHOLDER = "<directory>";

export interface RestoreInput {
  /** The engine's bootstrap file; null when it did not say. */
  bootstrap: string | null;
  /** The backup to restore; null when none has been taken yet. */
  directory: string | null;
  hosting: EngineSettings["hosting"];
  platform: RestorePlatform;
}

export interface RestoreCommands {
  verify: string;
  /** Null when the engine runs in a terminal (`packie run`) and is stopped there. */
  stop: string | null;
  restore: string;
  start: string;
  /** Finds the bootstrap file of the installed service. */
  status: string;
}

/**
 * The commands of a restore on the lab computer (docs/spec/backup.md §4, operator-console.md §12),
 * each aimed at the engine's own bootstrap file so they act on the state the console is attached
 * to. The service commands and the restore run elevated: `sudo` where there is one, and the
 * administrator terminal of Windows needs nothing in front. On macOS the commands are those of
 * the system service; a per-user agent leaves out `sudo` and adds `--user` (the page says so).
 */
export function restoreCommands({
  bootstrap,
  directory,
  hosting,
  platform,
}: RestoreInput): RestoreCommands {
  const base = `packie --bootstrap ${bootstrap === null ? BOOTSTRAP_PLACEHOLDER : shellPath(bootstrap)}`;
  const backup = directory === null ? DIRECTORY_PLACEHOLDER : shellPath(directory);
  const elevated = platform === "macos" || platform === "linux" ? "sudo " : "";
  const inTerminal = hosting === "console";
  return {
    status: "packie service status",
    verify: `${base} backup verify ${backup}`,
    stop: inTerminal ? null : `${elevated}${base} service stop`,
    restore: `${elevated}${base} backup restore ${backup} --yes`,
    start: inTerminal ? `${base} run` : `${elevated}${base} service start`,
  };
}
