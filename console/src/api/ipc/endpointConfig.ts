/**
 * Endpoint documents: the engine's endpoint (docs/spec/configuration.md section 5) to the
 * console's, and back.
 *
 * Reading fills the specification's defaults, so a sparse document reads like a complete one.
 * Writing starts from the original object and overwrites only the fields whose console value
 * differs from what reading the original gave; everything the console does not model
 * (timeouts, keep-alive tuning, virtual links, `tags`, directories, ...) survives. A new
 * endpoint is written sparse: required fields, and modeled fields that differ from the default.
 */

import type { SecretRef } from "@/ipc/generated";
import { clone } from "@/lib/clone";
import type {
  EndpointConfig,
  FolderEndpointConfig,
  SerialEndpointConfig,
  SftpEndpointConfig,
  TcpClientEndpointConfig,
  TcpListenerEndpointConfig,
} from "../types";
import { applyPatch, isEqual, type Patch } from "./util";
import type {
  WireEndpoint,
  WireFolderSettings,
  WireSerialSettings,
  WireSftpSettings,
  WireTcpClientSettings,
  WireTcpListenerSettings,
} from "./wire";

interface Base {
  id: string;
  label: string;
  enabled: boolean;
}

/** The name a secret reference holds; a literal in its place is never shown. */
export function secretName(secret: SecretRef | null | undefined): string {
  return typeof secret === "object" && secret !== null ? secret.secret : "";
}

const serialFrom = (base: Base, t: WireSerialSettings): SerialEndpointConfig => ({
  ...base,
  kind: "serial",
  port: t.port_name,
  baudRate: t.baud_rate,
  dataBits: (t.data_bits ?? 8) as SerialEndpointConfig["dataBits"],
  parity: t.parity ?? "none",
  stopBits: t.stop_bits === "two" ? 2 : 1,
  flowControl: t.flow_control ?? "none",
});

const tcpClientFrom = (base: Base, t: WireTcpClientSettings): TcpClientEndpointConfig => ({
  ...base,
  kind: "tcp_client",
  host: t.host,
  port: t.port,
  connectTimeoutMs: t.connect_timeout_ms ?? 5000,
  idleTimeoutSeconds: t.idle_timeout_seconds ?? 0,
  keepalive: t.keep_alive ?? true,
});

const tcpListenerFrom = (base: Base, t: WireTcpListenerSettings): TcpListenerEndpointConfig => ({
  ...base,
  kind: "tcp_listener",
  bindAddress: t.bind_address,
  port: t.port,
  allowlist: [...(t.allowed_peer_cidrs ?? [])],
  allowAnyPeer: t.allow_any_peer ?? false,
  idleTimeoutSeconds: t.idle_timeout_seconds ?? 0,
  keepalive: t.keep_alive ?? true,
});

const folderFrom = (base: Base, t: WireFolderSettings): FolderEndpointConfig => ({
  ...base,
  kind: "folder",
  path: t.path,
  mode: t.mode ?? "watch",
  pollIntervalSec: t.poll_interval_seconds ?? 30,
  filePattern: t.file_pattern ?? "*",
});

const sftpFrom = (base: Base, t: WireSftpSettings): SftpEndpointConfig => ({
  ...base,
  kind: "sftp",
  host: t.host,
  port: t.port ?? 22,
  username: t.username,
  authMethod: t.auth.method === "key" ? "private_key" : "password",
  secretName: secretName(t.auth.method === "key" ? t.auth.private_key : t.auth.password),
  passphraseSecretName: t.auth.method === "key" ? secretName(t.auth.passphrase) : "",
  hostKeyFingerprint: t.host_key_fingerprint,
  remotePath: t.remote_directory,
  pollIntervalSec: t.poll_interval_seconds ?? 60,
  filePattern: t.file_pattern ?? "*",
});

export function endpointFromWire(raw: WireEndpoint): EndpointConfig {
  const base = { id: raw.endpoint_id, label: raw.label, enabled: raw.enabled ?? true };
  switch (raw.kind) {
    case "serial":
      return serialFrom(base, raw.settings);
    case "tcp_client":
      return tcpClientFrom(base, raw.settings);
    case "tcp_listener":
      return tcpListenerFrom(base, raw.settings);
    case "folder":
      return folderFrom(base, raw.settings);
    case "sftp":
      return sftpFrom(base, raw.settings);
  }
}

// ── Writing ────────────────────────────────────────────────────────────────────────────

const SERIAL: Patch<SerialEndpointConfig, WireSerialSettings> = {
  port: (r, u) => {
    r.port_name = u.port;
  },
  baudRate: (r, u) => {
    r.baud_rate = u.baudRate;
  },
  dataBits: (r, u) => {
    r.data_bits = u.dataBits;
  },
  parity: (r, u) => {
    r.parity = u.parity;
  },
  stopBits: (r, u) => {
    r.stop_bits = u.stopBits === 2 ? "two" : "one";
  },
  flowControl: (r, u) => {
    r.flow_control = u.flowControl;
  },
};

const TCP_CLIENT: Patch<TcpClientEndpointConfig, WireTcpClientSettings> = {
  host: (r, u) => {
    r.host = u.host;
  },
  port: (r, u) => {
    r.port = u.port;
  },
  connectTimeoutMs: (r, u) => {
    r.connect_timeout_ms = u.connectTimeoutMs;
  },
  idleTimeoutSeconds: (r, u) => {
    r.idle_timeout_seconds = u.idleTimeoutSeconds;
  },
  keepalive: (r, u) => {
    r.keep_alive = u.keepalive;
  },
};

const TCP_LISTENER: Patch<TcpListenerEndpointConfig, WireTcpListenerSettings> = {
  bindAddress: (r, u) => {
    r.bind_address = u.bindAddress;
  },
  port: (r, u) => {
    r.port = u.port;
  },
  allowlist: (r, u) => {
    r.allowed_peer_cidrs = [...u.allowlist];
  },
  allowAnyPeer: (r, u) => {
    r.allow_any_peer = u.allowAnyPeer;
  },
  idleTimeoutSeconds: (r, u) => {
    r.idle_timeout_seconds = u.idleTimeoutSeconds;
  },
  keepalive: (r, u) => {
    r.keep_alive = u.keepalive;
  },
};

const FOLDER: Patch<FolderEndpointConfig, WireFolderSettings> = {
  path: (r, u) => {
    r.path = u.path;
  },
  mode: (r, u) => {
    r.mode = u.mode;
  },
  pollIntervalSec: (r, u) => {
    r.poll_interval_seconds = u.pollIntervalSec;
  },
  filePattern: (r, u) => {
    r.file_pattern = u.filePattern;
  },
};

function setAuth(r: WireSftpSettings, u: SftpEndpointConfig): void {
  r.auth =
    u.authMethod === "password"
      ? { method: "password", password: { secret: u.secretName } }
      : {
          method: "key",
          private_key: { secret: u.secretName },
          ...(u.passphraseSecretName === ""
            ? {}
            : { passphrase: { secret: u.passphraseSecretName } }),
        };
}

const SFTP: Patch<SftpEndpointConfig, WireSftpSettings> = {
  host: (r, u) => {
    r.host = u.host;
  },
  port: (r, u) => {
    r.port = u.port;
  },
  username: (r, u) => {
    r.username = u.username;
  },
  authMethod: setAuth,
  secretName: setAuth,
  passphraseSecretName: setAuth,
  hostKeyFingerprint: (r, u) => {
    r.host_key_fingerprint = u.hostKeyFingerprint;
  },
  remotePath: (r, u) => {
    r.remote_directory = u.remotePath;
  },
  pollIntervalSec: (r, u) => {
    r.poll_interval_seconds = u.pollIntervalSec;
  },
  filePattern: (r, u) => {
    r.file_pattern = u.filePattern;
  },
};

/** What a new endpoint leaves out because the engine assumes it. */
const DEFAULTS: Record<EndpointConfig["kind"], Record<string, unknown>> = {
  serial: { data_bits: 8, parity: "none", stop_bits: "one", flow_control: "none" },
  tcp_client: { connect_timeout_ms: 5000, idle_timeout_seconds: 0, keep_alive: true },
  tcp_listener: {
    allowed_peer_cidrs: [],
    allow_any_peer: false,
    idle_timeout_seconds: 0,
    keep_alive: true,
  },
  folder: { mode: "watch", poll_interval_seconds: 30, file_pattern: "*" },
  sftp: { port: 22, poll_interval_seconds: 60, file_pattern: "*" },
};

function patchSettings(
  endpoint: WireEndpoint,
  ui: EndpointConfig,
  base: EndpointConfig | null,
): void {
  switch (ui.kind) {
    case "serial":
      applyPatch(endpoint.settings as WireSerialSettings, ui, base as typeof ui | null, SERIAL);
      return;
    case "tcp_client":
      applyPatch(
        endpoint.settings as WireTcpClientSettings,
        ui,
        base as typeof ui | null,
        TCP_CLIENT,
      );
      return;
    case "tcp_listener":
      applyPatch(
        endpoint.settings as WireTcpListenerSettings,
        ui,
        base as typeof ui | null,
        TCP_LISTENER,
      );
      return;
    case "folder":
      applyPatch(endpoint.settings as WireFolderSettings, ui, base as typeof ui | null, FOLDER);
      return;
    case "sftp":
      applyPatch(endpoint.settings as WireSftpSettings, ui, base as typeof ui | null, SFTP);
      return;
  }
}

function newEndpoint(ui: EndpointConfig): WireEndpoint {
  const next = {
    endpoint_id: ui.id,
    label: ui.label,
    ...(ui.enabled ? {} : { enabled: false }),
    kind: ui.kind,
    settings: {},
  } as WireEndpoint;
  patchSettings(next, ui, null);
  const settings = next.settings as unknown as Record<string, unknown>;
  for (const [key, value] of Object.entries(DEFAULTS[ui.kind])) {
    if (isEqual(settings[key], value)) delete settings[key];
  }
  return next;
}

/** The engine endpoint for `ui`; `raw` is the original with the same id, when there is one. */
export function endpointToWire(ui: EndpointConfig, raw: WireEndpoint | undefined): WireEndpoint {
  if (raw === undefined || raw.kind !== ui.kind) return newEndpoint(ui);
  const base = endpointFromWire(raw);
  const next = clone(raw);
  if (ui.label !== base.label) next.label = ui.label;
  if (ui.enabled !== base.enabled) next.enabled = ui.enabled;
  patchSettings(next, ui, base);
  return next;
}
