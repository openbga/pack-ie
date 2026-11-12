// How a probe's steps and failures read to an operator. The engine names the steps
// (docs/spec/diagnostics.md section 10); the words and the hints are the console's.
import type { EndpointKind, ProbeStep } from "@/api/types";

const STEP_WORDS: Record<string, string> = {
  credentials: "Look up credentials",
  resolve: "Resolve host",
  connect: "Connect",
  authenticate: "Sign in",
  "open sftp": "Open SFTP",
  list: "List remote directory",
  disconnect: "Disconnect",
  bind: "Bind address and port",
  open: "Open port",
  "virtual link": "Check virtual link",
  "find folder": "Find folder",
  "read directory": "Read folder",
  "write test": "Write test",
};

const NOTHING_LEFT =
  "No message or instrument data is sent, the configuration is not changed and no test file is left behind. Each test is recorded in the audit log with your account.";

/**
 * What a test of this kind of endpoint does and does not do (docs/spec/diagnostics.md §10),
 * said before it runs. A folder is the one kind that writes: to prove the folder can be
 * written it creates one empty hidden file and deletes it again.
 */
export function explanationFor(kind: EndpointKind): string {
  return kind === "folder"
    ? `The test reads the folder and counts the files that match, never their names. To prove the folder can be written it also creates one empty hidden file there and deletes it again. ${NOTHING_LEFT}`
    : `The test only opens, connects or reads. ${NOTHING_LEFT}`;
}

export function stepWords(name: string): string {
  return STEP_WORDS[name] ?? `${name.charAt(0).toUpperCase()}${name.slice(1)}`;
}

/** A step's duration: whole milliseconds, then seconds with one decimal. */
export function formatMillis(millis: number): string {
  if (millis <= 0) return "under 1 ms";
  return millis < 1000 ? `${millis} ms` : `${(millis / 1000).toFixed(1)} s`;
}

export const HELD_PORT_HINT =
  "A port this engine already holds is reported as in use: the endpoint is working if a channel is running it.";

/** What to check next for a failed step, from the step and the cause the engine gave. */
export function hintFor(step: ProbeStep): string | null {
  const cause = step.detail.toLowerCase();
  switch (step.name) {
    case "open":
      if (cause.startsWith("in use by this engine")) return HELD_PORT_HINT;
      return "Check the port name against the ports this computer reports, and that no other program has it open.";
    case "bind":
      if (cause.includes("already in use")) {
        return "A running listener, or another program, holds this port. If a channel is running this endpoint, it is working; otherwise choose another port.";
      }
      return "Use an address this computer has, or 0.0.0.0 for all of them.";
    case "resolve":
      return "Check the spelling of the host name, or use the IP address.";
    case "connect":
      if (cause.includes("host key")) {
        return "The server presented a different host key than the pinned fingerprint. Confirm the fingerprint with the server's administrator before changing it.";
      }
      if (cause.includes("refused")) {
        return "The host answered but nothing listens on that port. Check the port and that the system is running.";
      }
      return "Nothing answered in time. Check the host and port, that the system is running and that the network allows the connection.";
    case "credentials":
      return "Store the secret under the name this endpoint uses in Settings, then test again.";
    case "authenticate":
      return "The server refused the credentials. Check the user name and the value of the secret.";
    case "list":
      return "Check the remote path and that the account may list it.";
    case "find folder":
      return "Check the path, and that the account the engine runs as can reach it.";
    case "write test":
      return "Sources move or delete the files they consume and destinations write, so the account the engine runs as must be able to create files here.";
    default:
      return null;
  }
}
