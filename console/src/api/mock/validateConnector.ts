import type {
  AuSettings,
  ChannelConfig,
  ConfigDocument,
  ConnectorConfig,
  EndpointKind,
  Protocol,
} from "../types";
import { blank, error, type Issues, warning } from "./validateIssues";

/** docs/spec/configuration.md §6.4: the endpoint kinds a protocol can run over. */
const PROTOCOL_ENDPOINTS: Record<Protocol, readonly EndpointKind[]> = {
  astm: ["serial", "tcp_client", "tcp_listener"],
  au: ["serial", "tcp_client", "tcp_listener"],
  hl7_mllp: ["tcp_client", "tcp_listener"],
  delimited_file: ["folder", "sftp"],
  raw: [],
};

const words = (text: string) => text.replaceAll("_", " ");

export function checkEndpointRef(
  document: ConfigDocument,
  channel: ChannelConfig,
  endpointId: string,
  path: string,
  issues: Issues,
): void {
  const endpoint = document.endpoints.find((e) => e.id === endpointId);
  if (blank(endpointId)) error(issues, path, "Choose an endpoint.");
  else if (endpoint === undefined)
    error(issues, path, `Endpoint "${endpointId}" does not exist.`, "unknown_endpoint");
  else if (!endpoint.enabled && channel.enabled) {
    warning(
      issues,
      path,
      `Endpoint "${endpoint.label}" is disabled, so this channel cannot run.`,
      "endpoint_disabled",
    );
  }
}

/** A protocol must suit the kind of its endpoint (`endpoint_kind_not_allowed`). */
function checkProtocolSuits(
  document: ConfigDocument,
  connector: ConnectorConfig,
  path: string,
  issues: Issues,
): void {
  const at = `${path}.protocol`;
  if (connector.protocol === "raw") {
    error(issues, at, "Raw is only for bridge taps; a message channel speaks a message protocol.");
    return;
  }
  const endpoint = document.endpoints.find((e) => e.id === connector.endpointId);
  if (endpoint === undefined) return;
  const kinds = PROTOCOL_ENDPOINTS[connector.protocol];
  if (!kinds.includes(endpoint.kind)) {
    error(
      issues,
      at,
      `${connector.protocol} does not run over a ${words(endpoint.kind)} endpoint; it needs ${kinds
        .map(words)
        .join(", ")}.`,
      "endpoint_kind_not_allowed",
    );
  }
}

/** The column template of a delimited connector must be a JSON object. */
function checkTemplate(connector: ConnectorConfig, path: string, issues: Issues): void {
  const at = `${path}.settings.templateJson`;
  const text = connector.settings.templateJson ?? "";
  if (blank(text)) {
    error(issues, at, "The column template is required for delimited files.");
    return;
  }
  try {
    const parsed: unknown = JSON.parse(text);
    if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
      error(issues, at, "The column template must be a JSON object, written with { }.");
    }
  } catch (cause) {
    const reason = cause instanceof Error ? cause.message : "invalid JSON";
    error(issues, at, `The column template is not valid JSON: ${reason}`);
  }
}

const isInt = (value: unknown): value is number => Number.isInteger(value);
const inSet = (value: unknown, allowed: readonly unknown[]) => allowed.includes(value);
const timeoutOk = (value: unknown) =>
  isInt(value) && value >= 100 && value <= 9900 && value % 100 === 0;

function controlBytesOk(bytes: unknown): bytes is number[] {
  return (
    Array.isArray(bytes) &&
    bytes.length >= 1 &&
    bytes.length <= 2 &&
    bytes.every((byte) => isInt(byte) && byte >= 1 && byte <= 31 && byte !== 6 && byte !== 21)
  );
}

function checkControlBytes(au: AuSettings, path: string, issues: Issues): void {
  const rule = "Use one or two control bytes from 1 to 31, never 6 (ACK) or 21 (NAK).";
  if (!controlBytesOk(au.framingStart)) error(issues, `${path}.framingStart`, rule);
  if (!controlBytesOk(au.framingEnd)) error(issues, `${path}.framingEnd`, rule);
  const endIsEtx = au.framingEnd.length === 1 && au.framingEnd[0] === 3;
  if (au.useEtb && !endIsEtx) {
    error(issues, `${path}.useEtb`, "ETB can only be used when the end bytes are 3 (ETX).");
  }
}

function checkPatientInfo(digits: unknown, path: string, issues: Issues): void {
  const at = `${path}.patientInfoDigits`;
  if (!Array.isArray(digits) || digits.length !== 6) {
    error(issues, at, "Give six digit counts, one for each patient information field.");
    return;
  }
  digits.forEach((count, index) => {
    if (!isInt(count) || count < 0 || count > 20) {
      error(issues, `${at}[${index}]`, "Each count must be a whole number from 0 to 20.");
    }
  });
}

/** docs/spec/configuration.md §7.2: every field is required and checked. */
function checkAu(au: AuSettings, path: string, issues: Issues): void {
  const choices: [field: keyof AuSettings, allowed: readonly unknown[], message: string][] = [
    ["profile", ["au400", "au640"], "Choose the instrument profile, AU400 or AU640."],
    ["maxWireLen", [256, 512, 1024], "Maximum wire length must be 256, 512 or 1024."],
    ["framingClass", ["a", "b"], "Choose framing class a or b."],
    ["rackDigits", [0, 4, 5], "Rack digits must be 0, 4 or 5."],
    ["resultDigits", [6, 9], "Result digits must be 6 or 9."],
  ];
  for (const [field, allowed, message] of choices) {
    if (!inSet(au[field], allowed)) error(issues, `${path}.${field}`, message);
  }
  checkControlBytes(au, path, issues);
  if (!isInt(au.sampleIdDigits) || au.sampleIdDigits < 1 || au.sampleIdDigits > 20) {
    error(
      issues,
      `${path}.sampleIdDigits`,
      "Sample ID digits must be a whole number from 1 to 20.",
    );
  }
  checkPatientInfo(au.patientInfoDigits, path, issues);
  for (const field of ["frameTimeoutMs", "blockTimeoutMs"] as const) {
    // The block timeout is optional: absent is the engine's default.
    if (field === "blockTimeoutMs" && au[field] === undefined) continue;
    if (!timeoutOk(au[field])) {
      error(issues, `${path}.${field}`, "Use 100 to 9900 milliseconds, in steps of 100.");
    }
  }
}

/** The engine's syntax: `O.<field>[.<component>]`, field 3 to 99, component 1 to 99, no leading zeros. */
const SAMPLE_ID_PATH = /^O\.(?:[3-9]|[1-9]\d)(?:\.(?:[1-9]|[1-9]\d))?$/;

function checkSampleIdPath(
  value: string | undefined,
  role: "source" | "destination",
  path: string,
  issues: Issues,
): void {
  if (role !== "source" || value === undefined || SAMPLE_ID_PATH.test(value)) return;
  error(
    issues,
    `${path}.settings.sampleIdPath`,
    "The sample id path is O. and a field from 3 to 99, then optionally a component from 1 to 99, such as O.4.3.",
  );
}

function checkSettings(
  connector: ConnectorConfig,
  role: "source" | "destination",
  path: string,
  issues: Issues,
) {
  const { settings } = connector;
  switch (connector.protocol) {
    case "delimited_file":
      checkTemplate(connector, path, issues);
      return;
    case "au":
      if (settings.au === undefined) {
        error(issues, `${path}.settings.au`, "The AU instrument settings are required.");
      } else checkAu(settings.au, `${path}.settings.au`, issues);
      return;
    case "astm":
      if (
        settings.astmRole !== undefined &&
        !inSet(settings.astmRole, ["computer", "instrument"])
      ) {
        error(issues, `${path}.settings.astmRole`, "The link role is computer or instrument.");
      }
      checkSampleIdPath(settings.sampleIdPath, role, path, issues);
      return;
    case "hl7_mllp":
      if (role === "source" && settings.ackMode === "none") {
        error(
          issues,
          `${path}.settings.ackMode`,
          "A source must acknowledge: choose original or enhanced. None is for destinations.",
        );
      }
      return;
    case "raw":
      return;
  }
}

/** The endpoint, its protocol and the protocol's settings. */
export function validateConnector(
  document: ConfigDocument,
  channel: ChannelConfig,
  connector: ConnectorConfig,
  role: "source" | "destination",
  path: string,
  issues: Issues,
): void {
  checkEndpointRef(document, channel, connector.endpointId, `${path}.endpointId`, issues);
  checkProtocolSuits(document, connector, path, issues);
  checkSettings(connector, role, path, issues);
}
