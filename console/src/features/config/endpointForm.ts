// The endpoint form's values and how they become an `EndpointConfig`. Numbers are held as
// text while typed; the form holds every kind's fields, and `kind` decides which are used.
import type { EndpointConfig, EndpointKind, SerialEndpointConfig } from "@/api/types";

export interface EndpointFormValues {
  kind: EndpointKind | null;
  id: string;
  label: string;
  enabled: boolean;
  /** A serial port name, or a TCP port number. */
  port: string;
  baudRate: string;
  dataBits: string;
  parity: string;
  stopBits: string;
  flowControl: string;
  host: string;
  connectTimeoutMs: string;
  /** Seconds; 0 turns the idle timeout off. */
  idleTimeoutSeconds: string;
  keepalive: boolean;
  bindAddress: string;
  allowlist: string[];
  allowAnyPeer: boolean;
  path: string;
  mode: string;
  pollIntervalSec: string;
  filePattern: string;
  username: string;
  authMethod: "password" | "private_key";
  secretName: string;
  passphraseSecretName: string;
  hostKeyFingerprint: string;
  remotePath: string;
}

export const EMPTY_ENDPOINT_VALUES: EndpointFormValues = {
  kind: null,
  id: "",
  label: "",
  enabled: true,
  port: "",
  baudRate: "9600",
  dataBits: "8",
  parity: "none",
  stopBits: "1",
  flowControl: "none",
  host: "",
  connectTimeoutMs: "5000",
  idleTimeoutSeconds: "0",
  keepalive: true,
  bindAddress: "0.0.0.0",
  allowlist: [],
  allowAnyPeer: false,
  path: "",
  mode: "watch",
  pollIntervalSec: "30",
  filePattern: "*",
  username: "",
  authMethod: "password",
  secretName: "",
  passphraseSecretName: "",
  hostKeyFingerprint: "",
  remotePath: "",
};

/** The fields a kind starts with when it is chosen on create. */
export function kindDefaults(
  kind: EndpointKind,
): Pick<EndpointFormValues, "port" | "pollIntervalSec"> {
  return kind === "sftp"
    ? { port: "22", pollIntervalSec: "60" }
    : { port: "", pollIntervalSec: EMPTY_ENDPOINT_VALUES.pollIntervalSec };
}

export function endpointToValues(config: EndpointConfig): EndpointFormValues {
  const base = {
    ...EMPTY_ENDPOINT_VALUES,
    kind: config.kind,
    id: config.id,
    label: config.label,
    enabled: config.enabled,
  };
  switch (config.kind) {
    case "serial":
      return {
        ...base,
        port: config.port,
        baudRate: String(config.baudRate),
        dataBits: String(config.dataBits),
        parity: config.parity,
        stopBits: String(config.stopBits),
        flowControl: config.flowControl,
      };
    case "tcp_client":
      return {
        ...base,
        host: config.host,
        port: String(config.port),
        connectTimeoutMs: String(config.connectTimeoutMs),
        idleTimeoutSeconds: String(config.idleTimeoutSeconds),
        keepalive: config.keepalive,
      };
    case "tcp_listener":
      return {
        ...base,
        bindAddress: config.bindAddress,
        port: String(config.port),
        allowlist: [...config.allowlist],
        allowAnyPeer: config.allowAnyPeer,
        idleTimeoutSeconds: String(config.idleTimeoutSeconds),
        keepalive: config.keepalive,
      };
    case "folder":
      return {
        ...base,
        path: config.path,
        mode: config.mode,
        pollIntervalSec: String(config.pollIntervalSec),
        filePattern: config.filePattern,
      };
    case "sftp":
      return {
        ...base,
        host: config.host,
        port: String(config.port),
        username: config.username,
        authMethod: config.authMethod,
        secretName: config.secretName,
        passphraseSecretName: config.passphraseSecretName,
        hostKeyFingerprint: config.hostKeyFingerprint,
        remotePath: config.remotePath,
        pollIntervalSec: String(config.pollIntervalSec),
        filePattern: config.filePattern,
      };
  }
}

/** The endpoint the form describes; null until a kind is chosen. */
export function valuesToEndpoint(values: EndpointFormValues): EndpointConfig | null {
  const base = {
    id: values.id.trim(),
    label: values.label.trim(),
    enabled: values.enabled,
  };
  const text = (value: string) => value.trim();
  const num = (value: string) => Number(value.trim());
  switch (values.kind) {
    case null:
      return null;
    case "serial":
      return {
        ...base,
        kind: "serial",
        port: text(values.port),
        baudRate: num(values.baudRate),
        dataBits: num(values.dataBits) as 5 | 6 | 7 | 8,
        parity: values.parity as SerialEndpointConfig["parity"],
        stopBits: num(values.stopBits) as 1 | 2,
        flowControl: values.flowControl as SerialEndpointConfig["flowControl"],
      };
    case "tcp_client":
      return {
        ...base,
        kind: "tcp_client",
        host: text(values.host),
        port: num(values.port),
        connectTimeoutMs: num(values.connectTimeoutMs),
        idleTimeoutSeconds: num(values.idleTimeoutSeconds),
        keepalive: values.keepalive,
      };
    case "tcp_listener":
      return {
        ...base,
        kind: "tcp_listener",
        bindAddress: text(values.bindAddress),
        port: num(values.port),
        allowlist: values.allowlist.map(text).filter((entry) => entry !== ""),
        allowAnyPeer: values.allowAnyPeer,
        idleTimeoutSeconds: num(values.idleTimeoutSeconds),
        keepalive: values.keepalive,
      };
    case "folder":
      return {
        ...base,
        kind: "folder",
        path: text(values.path),
        mode: values.mode === "poll" ? "poll" : "watch",
        pollIntervalSec: num(values.pollIntervalSec),
        filePattern: text(values.filePattern),
      };
    case "sftp":
      return {
        ...base,
        kind: "sftp",
        host: text(values.host),
        port: num(values.port),
        username: text(values.username),
        authMethod: values.authMethod,
        secretName: text(values.secretName),
        passphraseSecretName:
          values.authMethod === "private_key" ? text(values.passphraseSecretName) : "",
        hostKeyFingerprint: text(values.hostKeyFingerprint),
        remotePath: text(values.remotePath),
        pollIntervalSec: num(values.pollIntervalSec),
        filePattern: text(values.filePattern),
      };
  }
}
