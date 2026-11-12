import type {
  ConfigDocument,
  EndpointConfig,
  SftpEndpointConfig,
  TcpListenerEndpointConfig,
} from "../types";
import {
  blank,
  checkPort,
  checkPositive,
  error,
  type Issues,
  required,
  warning,
} from "./validateIssues";

const FINGERPRINT = /^SHA256:[A-Za-z0-9+/]{43}={0,1}$/;
const IPV4 = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})(?:\/(\d{1,2}))?$/;
const IPV6 = /^[0-9a-fA-F:]+(?:\/\d{1,3})?$/;

function isLoopback(address: string): boolean {
  const value = address.trim().toLowerCase();
  return value === "localhost" || value === "::1" || value.startsWith("127.");
}

function validCidr(entry: string): boolean {
  const value = entry.trim();
  const v4 = IPV4.exec(value);
  if (v4 !== null) {
    const octets = v4.slice(1, 5).map(Number);
    const prefix = v4[5] === undefined ? 0 : Number(v4[5]);
    return octets.every((octet) => octet <= 255) && prefix <= 32;
  }
  return value.includes(":") && IPV6.test(value);
}

function validateListener(endpoint: TcpListenerEndpointConfig, path: string, issues: Issues): void {
  required(issues, `${path}.bindAddress`, endpoint.bindAddress, "The bind address");
  checkPort(issues, `${path}.port`, endpoint.port);
  checkIdle(endpoint.idleTimeoutSeconds, path, issues);
  endpoint.allowlist.forEach((entry, index) => {
    if (!validCidr(entry)) {
      error(
        issues,
        `${path}.allowlist[${index}]`,
        `"${entry}" is not an IP address or CIDR range such as 10.0.4.0/24.`,
        "invalid_cidr",
      );
    }
  });
  if (endpoint.allowlist.length > 0) return;
  if (endpoint.allowAnyPeer) {
    warning(issues, `${path}.allowAnyPeer`, "Any peer on the network can connect.");
  } else if (!isLoopback(endpoint.bindAddress)) {
    error(
      issues,
      `${path}.allowlist`,
      "List the peers allowed to connect, or allow any peer; a listener that is reachable from the network needs one of them.",
      "allowlist_required",
    );
  }
}

function checkIdle(seconds: number, path: string, issues: Issues): void {
  if (!Number.isInteger(seconds) || seconds < 0) {
    error(issues, `${path}.idleTimeoutSeconds`, "Idle timeout must be 0 (off) or more seconds.");
  }
}

function validateSftp(endpoint: SftpEndpointConfig, path: string, issues: Issues): void {
  required(issues, `${path}.host`, endpoint.host, "The host");
  checkPort(issues, `${path}.port`, endpoint.port);
  required(issues, `${path}.username`, endpoint.username, "The user name");
  required(
    issues,
    `${path}.secretName`,
    endpoint.secretName,
    endpoint.authMethod === "password" ? "The password secret name" : "The private key secret name",
  );
  if (blank(endpoint.hostKeyFingerprint)) {
    error(issues, `${path}.hostKeyFingerprint`, "The server's host key fingerprint is required.");
  } else if (!FINGERPRINT.test(endpoint.hostKeyFingerprint.trim())) {
    error(
      issues,
      `${path}.hostKeyFingerprint`,
      "The fingerprint must be SHA256: followed by 43 base64 characters.",
    );
  }
  required(issues, `${path}.remotePath`, endpoint.remotePath, "The remote path");
  checkFilePattern(endpoint.filePattern, `${path}.filePattern`, issues);
}

/** One glob over file names: `*` and `?` only, nothing that names a folder. */
function checkFilePattern(pattern: string, path: string, issues: Issues): void {
  if (blank(pattern)) error(issues, path, "The file name pattern is required.");
  else if (/[/\\[\]{}]/.test(pattern)) {
    error(
      issues,
      path,
      "Use only * and ? as wildcards, for example *.csv; no folders or brackets.",
    );
  }
}

function validateEndpointKind(endpoint: EndpointConfig, path: string, issues: Issues): void {
  switch (endpoint.kind) {
    case "serial":
      required(issues, `${path}.port`, endpoint.port, "The serial port");
      checkPositive(issues, `${path}.baudRate`, endpoint.baudRate, "Baud rate");
      return;
    case "tcp_client":
      required(issues, `${path}.host`, endpoint.host, "The host");
      checkPort(issues, `${path}.port`, endpoint.port);
      checkIdle(endpoint.idleTimeoutSeconds, path, issues);
      return;
    case "tcp_listener":
      validateListener(endpoint, path, issues);
      return;
    case "folder":
      required(issues, `${path}.path`, endpoint.path, "The folder path");
      checkFilePattern(endpoint.filePattern, `${path}.filePattern`, issues);
      return;
    case "sftp":
      validateSftp(endpoint, path, issues);
  }
}

interface Seen {
  ids: Set<string>;
  listeners: Set<string>;
  ports: Set<string>;
}

function checkIdentity(endpoint: EndpointConfig, path: string, seen: Seen, issues: Issues): void {
  if (blank(endpoint.id)) error(issues, `${path}.id`, "An id is required.");
  else if (seen.ids.has(endpoint.id)) {
    error(issues, `${path}.id`, `Duplicate endpoint id "${endpoint.id}".`, "duplicate_id");
  }
  seen.ids.add(endpoint.id);
  required(issues, `${path}.label`, endpoint.label, "The label");
}

/** Two enabled endpoints must not claim the same listen address or serial port. */
function checkClaims(endpoint: EndpointConfig, path: string, seen: Seen, issues: Issues): void {
  if (!endpoint.enabled) return;
  if (endpoint.kind === "tcp_listener") {
    const key = `${endpoint.bindAddress}:${endpoint.port}`;
    if (seen.listeners.has(key))
      error(issues, `${path}.port`, `Another listener already uses ${key}.`, "port_conflict");
    seen.listeners.add(key);
  }
  if (endpoint.kind === "serial" && !blank(endpoint.port)) {
    if (seen.ports.has(endpoint.port)) {
      warning(issues, `${path}.port`, `${endpoint.port} is also used by another endpoint.`);
    }
    seen.ports.add(endpoint.port);
  }
}

function checkSecrets(
  endpoint: EndpointConfig,
  path: string,
  secrets: ReadonlySet<string>,
  issues: Issues,
): void {
  if (endpoint.kind !== "sftp") return;
  const names: [field: string, name: string][] = [["secretName", endpoint.secretName]];
  if (endpoint.authMethod === "private_key") {
    names.push(["passphraseSecretName", endpoint.passphraseSecretName]);
  }
  for (const [field, name] of names) {
    if (blank(name) || secrets.has(name)) continue;
    warning(
      issues,
      `${path}.${field}`,
      `No secret named "${name}" is stored yet.`,
      "secret_unresolved",
    );
  }
}

export function validateEndpoints(
  document: ConfigDocument,
  secrets: ReadonlySet<string>,
  issues: Issues,
): void {
  const seen: Seen = { ids: new Set(), listeners: new Set(), ports: new Set() };
  document.endpoints.forEach((endpoint, index) => {
    const path = `endpoints[${index}]`;
    checkIdentity(endpoint, path, seen, issues);
    validateEndpointKind(endpoint, path, issues);
    checkSecrets(endpoint, path, secrets, issues);
    checkClaims(endpoint, path, seen, issues);
  });
}
