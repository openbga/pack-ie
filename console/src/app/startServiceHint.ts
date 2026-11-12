/**
 * What to tell an operator about starting the installed service from the console: it needs
 * administrator rights, and the command that works from an elevated terminal. The commands are
 * the packaged locations of `packaging/README.md`; the engine and its service manager decide,
 * the console only explains.
 */
export interface StartServiceHint {
  /** Why it may fail from here. */
  why: string;
  /** The command to run in an administrator terminal. */
  command: string;
}

export function startServiceHint(userAgent: string): StartServiceHint {
  if (/Windows/i.test(userAgent)) {
    return {
      why: "Starting the installed service needs administrator rights.",
      command: "packie service start   (in a PowerShell opened as administrator)",
    };
  }
  if (/Mac/i.test(userAgent)) {
    return {
      why: "Starting the installed service needs administrator rights; without them the command targets a per-user agent instead.",
      command:
        'sudo packie --bootstrap "/Library/Application Support/Positive ACK/bootstrap.json" service start',
    };
  }
  return {
    why: "Starting the installed service needs administrator rights.",
    command: "sudo packie --bootstrap /etc/packie/bootstrap.json service start",
  };
}
