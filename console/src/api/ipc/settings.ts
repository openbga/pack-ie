import type { ConnectionStatus, EngineSettings, SecretInfo } from "../types";
import type { ConnectionPayload } from "./bridge";
import { secretName } from "./endpointConfig";
import { type BootstrapNotifications, notificationSettingsFrom } from "./setup";
import type { WireConfig } from "./wire";

/** The parts of the engine's bootstrap document (`bootstrap.json`, configuration.md section 3) the console shows. */
export interface BootstrapDocument {
  node: { node_id: string; display_name: string };
  paths: { state_directory: string; log_directory?: string };
  logging?: { retention_days?: number; ring_buffer_events?: number };
  notifications?: BootstrapNotifications | null;
}

const DEFAULT_LOG_RETENTION_DAYS = 30;
const DEFAULT_LOG_RING_SIZE = 20_000;

export function connectionStatusFrom(payload: ConnectionPayload): ConnectionStatus {
  return {
    state: payload.state,
    endpoint: payload.endpoint,
    detail: payload.error,
    since: payload.since,
    actor: payload.engine?.actor?.name ?? null,
  };
}

/** `paths.log_directory`, which is `<state_directory>/logs` when the document leaves it out. */
function logDirectory({ paths }: BootstrapDocument): string {
  return paths.log_directory ?? `${paths.state_directory.replace(/[\\/]+$/, "")}/logs`;
}

/**
 * Read-only engine settings. Without a bootstrap document everything the document holds is
 * null (the node's identity falls back to what the engine said when connecting). The IPC
 * path is the one the console connected to, shown only with the document.
 */
export function engineSettingsFrom(
  bootstrap: BootstrapDocument | null,
  connection: ConnectionPayload | null,
  hosting: EngineSettings["hosting"] = null,
  bootstrapPath: string | null = null,
): EngineSettings {
  return {
    ...(bootstrapPath === null ? {} : { bootstrapPath }),
    ...(bootstrap === null
      ? {}
      : { notifications: notificationSettingsFrom(bootstrap.notifications) }),
    nodeId: bootstrap?.node.node_id ?? connection?.engine?.node_id ?? "",
    nodeName: bootstrap?.node.display_name ?? connection?.engine?.node_name ?? "",
    stateDir: bootstrap?.paths.state_directory ?? null,
    logDir: bootstrap === null ? null : logDirectory(bootstrap),
    ipcPath: bootstrap === null ? null : (connection?.endpoint ?? null),
    hosting,
    logRetentionDays:
      bootstrap === null ? null : (bootstrap.logging?.retention_days ?? DEFAULT_LOG_RETENTION_DAYS),
    logRingSize:
      bootstrap === null ? null : (bootstrap.logging?.ring_buffer_events ?? DEFAULT_LOG_RING_SIZE),
  };
}

/**
 * The secrets of the settings page: every name the engine holds a value for (`list_secrets`)
 * and every name an SFTP endpoint of `config` refers to, so a reference without a value shows
 * as missing. Values are never readable; where a value lives (key store or file) is not
 * reported by the engine, so `storage` is null.
 */
export function secretsFrom(config: WireConfig, stored: readonly string[]): SecretInfo[] {
  const users = new Map<string, string[]>(stored.map((name) => [name, []]));
  for (const endpoint of config.endpoints ?? []) {
    if (endpoint.kind !== "sftp") continue;
    const auth = endpoint.settings.auth;
    const names =
      auth.method === "password"
        ? [secretName(auth.password)]
        : [secretName(auth.private_key), secretName(auth.passphrase)];
    for (const name of names.filter((n) => n !== "")) {
      const ids = users.get(name) ?? [];
      if (!ids.includes(endpoint.endpoint_id)) ids.push(endpoint.endpoint_id);
      users.set(name, ids);
    }
  }
  const held = new Set(stored);
  return [...users.entries()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([name, usedByEndpointIds]) => ({
      name,
      storage: null,
      hasValue: held.has(name),
      usedByEndpointIds,
    }));
}

export function platformName(userAgent: string): string {
  if (/Windows/.test(userAgent)) return "Windows";
  if (/Mac OS X|Macintosh/.test(userAgent)) return "macOS";
  if (/Linux|X11/.test(userAgent)) return "Linux";
  return "Unknown";
}
