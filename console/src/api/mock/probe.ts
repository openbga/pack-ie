// What a read-only probe of an endpoint definition finds (docs/spec/diagnostics.md section 10),
// worded as the engine words it. Nothing is sent or left behind. Healthy definitions pass;
// the failures a journey can drive from the form are decided by the definition itself:
//   serial        COM99 and /dev/ttyMISSING are not on the computer; a port a running channel holds
//   tcp_client    host `nosuchhost.invalid` or any `.invalid` name does not resolve,
//                 `unreachable.example` times out, port 9 is refused
//   tcp_listener  a port a running listener holds; an address that is not this computer's
//   folder        a path with `missing` is not found, one with `readonly` cannot be written
//   sftp          an unstored secret, a fingerprint starting `SHA256:BAD` (another host key)
import type {
  EndpointConfig,
  EndpointTestRequest,
  FolderEndpointConfig,
  ProbeReport,
  ProbeStep,
  SerialEndpointConfig,
  SftpEndpointConfig,
  TcpClientEndpointConfig,
  TcpListenerEndpointConfig,
} from "../types";
import { endpointTarget } from "./endpointInfo";
import { hashString } from "./prng";
import { validateEndpoints } from "./validateEndpoints";
import type { Issues } from "./validateIssues";
import type { MockWorld } from "./world";

const DEFAULT_LIMIT_SECONDS = 10;
const LOCAL_ADDRESSES = new Set(["0.0.0.0", "::", "::1", "localhost", "10.0.4.5"]);

/** A repeatable duration in `[low, high)` for one step of one target. */
function took(step: string, target: string, low: number, high: number): number {
  return low + (hashString(`${step}|${target}`) % (high - low));
}

/** What the steps so far found: their list and the sentence for a probe that got through. */
class Steps {
  readonly list: ProbeStep[] = [];

  /** Records a step; false when it failed, which ends the probe. */
  add(name: string, ok: boolean, detail: string, millis: number): boolean {
    this.list.push({ name, ok, detail, millis });
    return ok;
  }

  report(success: string): ProbeReport {
    const failed = this.list.find((step) => !step.ok);
    return failed
      ? {
          ok: false,
          steps: this.list,
          summary: `The ${failed.name} step failed: ${failed.detail.replace(/\.$/, "")}.`,
        }
      : { ok: true, steps: this.list, summary: success };
  }
}

function plural(count: number, one: string, many: string): string {
  return `${count} ${count === 1 ? one : many}`;
}

function address(host: string): string {
  if (/^[\d.]+$/.test(host) || host.includes(":")) return host;
  const hash = hashString(host);
  return `10.0.${hash % 200}.${(hash >>> 8) % 250}`;
}

/** How connecting to `host:port` ends: the cause when it fails, and the time it took. */
function connectOutcome(endpoint: TcpClientEndpointConfig, limitMs: number) {
  const target = `${endpoint.host}:${endpoint.port}`;
  if (endpoint.host.toLowerCase() === "unreachable.example") {
    return { cause: "connection timed out", millis: Math.min(endpoint.connectTimeoutMs, limitMs) };
  }
  if (endpoint.port === 9) {
    return { cause: "connection refused", millis: took("connect", target, 1, 4) };
  }
  return { cause: null, millis: took("connect", target, 4, 60) };
}

function probeTcpClient(endpoint: TcpClientEndpointConfig, limitMs: number): ProbeReport {
  const steps = new Steps();
  const { host, port } = endpoint;
  const ip = address(host);
  const unresolvable = host.toLowerCase().endsWith(".invalid");
  const resolveDetail = unresolvable ? "host name did not resolve" : `resolved to ${ip}`;
  const resolved = steps.add("resolve", !unresolvable, resolveDetail, took("resolve", host, 2, 40));
  const { cause, millis } = connectOutcome(endpoint, limitMs);
  if (resolved) {
    steps.add(
      "connect",
      cause === null,
      cause ?? `connected to ${ip}:${port} in ${millis} ms, then closed`,
      millis,
    );
  }
  return steps.report(`${host}:${port} accepted a connection in ${millis} ms.`);
}

/** The channel running the listener on `port`, from the live snapshot. */
function listenerHolder(world: MockWorld, endpoint: TcpListenerEndpointConfig): string | null {
  const snapshot = world.snapshot();
  for (const held of snapshot.endpoints) {
    if (held.heldBy === null || held.kind !== "tcp_listener") continue;
    const config = world.config.live.endpoints.find((each) => each.id === held.id);
    if (config?.kind !== "tcp_listener" || config.port !== endpoint.port) continue;
    const wildcard = (value: string) => value === "0.0.0.0" || value === "::";
    if (
      config.bindAddress === endpoint.bindAddress ||
      wildcard(config.bindAddress) ||
      wildcard(endpoint.bindAddress)
    ) {
      return held.heldBy;
    }
  }
  return null;
}

function probeTcpListener(world: MockWorld, endpoint: TcpListenerEndpointConfig): ProbeReport {
  const steps = new Steps();
  const where = `${endpoint.bindAddress}:${endpoint.port}`;
  const local = LOCAL_ADDRESSES.has(endpoint.bindAddress.toLowerCase());
  const inUse = local && listenerHolder(world, endpoint) !== null;
  const detail = !local
    ? `${endpoint.bindAddress} is not an address of this computer`
    : inUse
      ? `port ${endpoint.port} on ${endpoint.bindAddress} is already in use (by this engine or another program)`
      : `${where} can be bound, then released`;
  steps.add("bind", local && !inUse, detail, took("bind", where, 1, 6));
  return steps.report(`This computer can listen on ${where}.`);
}

/** The label of the channel holding the serial port `name` open, from the live snapshot. */
function portHolder(world: MockWorld, name: string): string | null {
  const snapshot = world.snapshot();
  for (const held of snapshot.endpoints) {
    if (held.heldBy === null || held.kind !== "serial") continue;
    const config = world.config.live.endpoints.find((each) => each.id === held.id);
    if (config?.kind !== "serial" || config.port.toLowerCase() !== name.toLowerCase()) continue;
    return snapshot.channels.find((channel) => channel.id === held.heldBy)?.label ?? held.heldBy;
  }
  return null;
}

const FLOW_WORDS = { none: "no flow control", rts_cts: "RTS/CTS", xon_xoff: "XON/XOFF" } as const;

function probeSerial(world: MockWorld, endpoint: SerialEndpointConfig): ProbeReport {
  const steps = new Steps();
  const name = endpoint.port;
  const holder = portHolder(world, name);
  const missing = /^(COM99|\/dev\/ttyMISSING)$/i.test(name);
  const framing = `${endpoint.dataBits}${endpoint.parity[0]?.toUpperCase() ?? "N"}${endpoint.stopBits}`;
  if (holder !== null) {
    steps.add("open", false, `in use by this engine (channel ${holder})`, 0);
  } else {
    steps.add(
      "open",
      !missing,
      missing
        ? "no such port"
        : `${name} opened at ${endpoint.baudRate} baud, ${framing}, ${FLOW_WORDS[endpoint.flowControl]}, and closed again; nothing was sent`,
      took("open", name, 3, 25),
    );
  }
  return steps.report(`${name} can be used with these settings.`);
}

function probeFolder(endpoint: FolderEndpointConfig): ProbeReport {
  const steps = new Steps();
  const path = endpoint.path;
  const missing = /missing/i.test(path);
  const found = steps.add(
    "find folder",
    !missing,
    missing ? "not found" : `${path} exists and is a directory`,
    took("find", path, 1, 5),
  );
  if (found) {
    const files = hashString(`files|${path}|${endpoint.filePattern}`) % 40;
    steps.add(
      "read directory",
      true,
      `readable; ${plural(files, "file", "files")} match ${endpoint.filePattern}`,
      took("read", path, 2, 30),
    );
    const readOnly = /readonly/i.test(path);
    steps.add(
      "write test",
      !readOnly,
      readOnly
        ? "permission denied"
        : "writable (an empty hidden test file was created and deleted; the endpoint has no read-only setting, so writing is always checked)",
      took("write", path, 1, 8),
    );
  }
  return steps.report(`The folder ${path} can be read and written.`);
}

/** The first secret the endpoint names that the mock's store does not hold. */
function missingSecret(world: MockWorld, endpoint: SftpEndpointConfig): string | undefined {
  const names =
    endpoint.authMethod === "private_key"
      ? [endpoint.secretName, endpoint.passphraseSecretName]
      : [endpoint.secretName];
  const stored = world.secrets.names();
  return names.find((name) => name !== "" && !stored.has(name));
}

function probeSftp(world: MockWorld, endpoint: SftpEndpointConfig): ProbeReport {
  const steps = new Steps();
  const target = `${endpoint.host}:${endpoint.port}`;
  const done = `${target} accepted the host key and credentials and lists ${endpoint.remotePath}.`;
  const step = (name: string, ok: boolean, detail: string, low: number, high: number) =>
    steps.add(name, ok, detail, took(name, target, low, high));
  const missing = missingSecret(world, endpoint);
  const found =
    endpoint.authMethod === "private_key" ? "private key found and readable" : "password found";
  if (
    !step(
      "credentials",
      missing === undefined,
      missing === undefined ? found : `no secret named "${missing}" is stored`,
      1,
      4,
    )
  ) {
    return steps.report(done);
  }
  const unresolvable = endpoint.host.toLowerCase().endsWith(".invalid");
  const resolveDetail = unresolvable
    ? "host name did not resolve"
    : `resolved to ${address(endpoint.host)}`;
  if (!step("resolve", !unresolvable, resolveDetail, 2, 40)) return steps.report(done);
  const pinned = endpoint.hostKeyFingerprint.trim();
  const mismatch = pinned.startsWith("SHA256:BAD");
  const connectDetail = mismatch
    ? `host key does not match the pinned fingerprint ${pinned}`
    : `connected; host key matches the pinned fingerprint ${pinned}`;
  if (!step("connect", !mismatch, connectDetail, 40, 180)) return steps.report(done);
  step("authenticate", true, `signed in as ${endpoint.username}`, 30, 120);
  step("open sftp", true, "SFTP subsystem started", 8, 40);
  const files = hashString(`files|${target}|${endpoint.remotePath}`) % 25;
  const directories = hashString(`dirs|${target}|${endpoint.remotePath}`) % 3;
  step(
    "list",
    true,
    `${endpoint.remotePath} is readable; ${plural(files, "file", "files")} match ${endpoint.filePattern}, ${plural(directories, "sub-directory", "sub-directories")}`,
    6,
    60,
  );
  step("disconnect", true, "closed", 1, 5);
  return steps.report(done);
}

/** The endpoint rules the engine applies before probing; an id and label are not part of what is probed. */
function ruleErrors(world: MockWorld, endpoint: EndpointConfig): Issues {
  const issues: Issues = [];
  validateEndpoints(
    {
      version: world.config.version,
      endpoints: [endpoint],
      channels: [],
      alertThresholds: world.config.live.alertThresholds,
    },
    world.secrets.names(),
    issues,
  );
  return issues.filter((issue) => issue.severity === "error" && !/\.(id|label)$/.test(issue.path));
}

function probeOf(world: MockWorld, endpoint: EndpointConfig, limitMs: number): ProbeReport {
  switch (endpoint.kind) {
    case "serial":
      return probeSerial(world, endpoint);
    case "tcp_client":
      return probeTcpClient(endpoint, limitMs);
    case "tcp_listener":
      return probeTcpListener(world, endpoint);
    case "folder":
      return probeFolder(endpoint);
    case "sftp":
      return probeSftp(world, endpoint);
  }
}

/**
 * Probes the definition without saving it. A definition the endpoint rules reject comes back
 * as `validation` at `endpoints[0]` before anything runs. Every test is audited as
 * `endpoint_test`: `rejected` for that refusal, `succeeded` with `ok` when a report is returned.
 */
export function probeEndpoint(world: MockWorld, request: EndpointTestRequest): ProbeReport {
  const { endpoint } = request;
  const details = { kind: endpoint.kind, target: endpointTarget(endpoint) };
  world.audit.record({ action: "endpoint_test", outcome: "requested", details });
  const errors = ruleErrors(world, endpoint);
  if (errors.length > 0) {
    world.audit.record({ action: "endpoint_test", outcome: "rejected", details });
    return world.fail(
      "validation",
      "The endpoint's settings were rejected before any test ran.",
      errors.map((issue) => ({ field: issue.path, message: issue.message })),
    );
  }
  const limitMs = (request.timeoutSeconds ?? DEFAULT_LIMIT_SECONDS) * 1000;
  const report = probeOf(world, endpoint, limitMs);
  world.audit.record({
    action: "endpoint_test",
    outcome: "succeeded",
    details: { ...details, ok: report.ok },
  });
  return report;
}
