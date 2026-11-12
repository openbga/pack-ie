/**
 * Engine field path to console field path, for field errors and validation issues. The
 * engine names `endpoints[0].settings.port_name` and `channels[1].destinations[0].mapping.rules[3].to`
 * (docs/spec/configuration.md section 9); the console's form fields are `endpoints[0].port` and
 * `channels[1].destinations[0].mappings[3].to`. One table per scope; a path nothing matches
 * passes through unchanged, and so do fields of the engine the console does not model.
 *
 * Some paths depend on the document the engine checked: filters the console cannot show (such
 * as `all`/`any`) do not count in its filter lists, and a mapping it cannot show has no rules
 * there; settings keys mean different things in different protocols. When `document` is given
 * those are resolved; without it paths are translated as if everything were shown.
 */

import { visibleFilterIndex } from "./filters";
import { mappingFromWire } from "./mappings";
import type { WireChannel, WireConfig, WireDestination, WireExpression } from "./wire";

type Translated = string | null;

/** `endpoints[i].settings.<key>`; keys have list indexes removed. */
const ENDPOINT_FIELDS: Record<string, string> = {
  port_name: "port",
  baud_rate: "baudRate",
  data_bits: "dataBits",
  parity: "parity",
  stop_bits: "stopBits",
  flow_control: "flowControl",
  host: "host",
  port: "port",
  connect_timeout_ms: "connectTimeoutMs",
  idle_timeout_seconds: "idleTimeoutSeconds",
  keep_alive: "keepalive",
  keep_alive_time_sec: "keepalive",
  keep_alive_interval_sec: "keepalive",
  keep_alive_retry_count: "keepalive",
  bind_address: "bindAddress",
  allowed_peer_cidrs: "allowlist",
  allow_any_peer: "allowAnyPeer",
  path: "path",
  mode: "mode",
  poll_interval_seconds: "pollIntervalSec",
  file_pattern: "filePattern",
  username: "username",
  auth: "authMethod",
  "auth.method": "authMethod",
  "auth.password": "secretName",
  "auth.password.secret": "secretName",
  "auth.private_key": "secretName",
  "auth.private_key.secret": "secretName",
  "auth.passphrase": "passphraseSecretName",
  "auth.passphrase.secret": "passphraseSecretName",
  host_key_fingerprint: "hostKeyFingerprint",
  remote_directory: "remotePath",
};

/** Fields whose console value is a list, so an index in the engine path stays. */
const LIST_FIELDS = new Set(["allowlist"]);

const ENDPOINT_COMMON: Record<string, string> = {
  endpoint_id: "id",
  label: "label",
  enabled: "enabled",
  kind: "kind",
};

/** `channels[i].<key>` for the fields of the channel itself. */
const CHANNEL_FIELDS: Record<string, string> = {
  channel_id: "id",
  label: "label",
  enabled: "enabled",
  kind: "kind",
  endpoint_a: "endpointAId",
  endpoint_b: "endpointBId",
  "connection.connect_strategy": "pairing",
  "connection.receiver": "pairingReceiver",
  "buffer.max_bytes": "bufferBytes",
  tap: "tapFramer",
  "tap.framer": "tapFramer",
  "tap.record_messages": "tapFramer",
  "tap.retention": "tapRetention",
  "tap.retention.max_age_days": "tapRetention.days",
  "tap.retention.max_messages": "tapRetention.maxMessages",
  "retry.initial_delay_ms": "retry.initialMs",
  "retry.max_delay_ms": "retry.maxMs",
  "retention.max_age_days": "retention.days",
  "retention.max_messages": "retention.maxMessages",
};

/** `destinations[j].<key>` for the fields of the destination itself. */
const DESTINATION_FIELDS: Record<string, string> = {
  destination_id: "id",
  label: "label",
  enabled: "enabled",
  "delivery.max_attempts": "retry.maxAttempts",
  "delivery.retry.initial_delay_ms": "retry.initialMs",
  "delivery.retry.max_delay_ms": "retry.maxMs",
};

const ALERT_FIELDS: Record<string, string> = {
  endpoint_down_seconds: "endpointDownSeconds",
  delivery_backlog_count: "queueDepthWarning",
  buffer_high_percent: "bufferPressurePercent",
  disk_low_megabytes: "diskFreeMinMib",
};

/** AU settings: engine key (indexes removed) to `settings.au.<field>`. */
const AU_FIELDS: Record<string, string> = {
  profile: "profile",
  framing: "",
  "framing.start": "framingStart",
  "framing.end": "framingEnd",
  "framing.bcc": "bcc",
  "framing.use_etb": "useEtb",
  "framing.max_wire_len": "maxWireLen",
  "framing.class": "framingClass",
  format: "",
  "format.unit_no": "unitNo",
  "format.rack_digits": "rackDigits",
  "format.sample_id_digits": "sampleIdDigits",
  "format.sex": "sex",
  "format.age": "age",
  "format.month": "month",
  "format.patient_info_digits": "patientInfoDigits",
  "format.result_digits": "resultDigits",
  frame_timeout_ms: "frameTimeoutMs",
  block_timeout_ms: "blockTimeoutMs",
};

/** Settings only delimited files have: the console edits them as one JSON text. */
const DELIMITED_KEYS = new Set([
  "delimiter",
  "quote",
  "header_row",
  "skip_lines",
  "line_ending",
  "write_bom",
  "columns",
  "record_mode",
  "group_by",
  "max_file_bytes",
  "file_name",
  "on_exists",
]);

const FILTER_FIELDS: Record<string, string> = {
  path: "path",
  op: "operator",
  value: "value",
  ignore_case: "ignoreCase",
};

/** Rule fields that keep their name; the others are in `RULE_RENAMES`. */
const RULE_FIELDS = new Set([
  "op",
  "from",
  "to",
  "default",
  "required",
  "value",
  "table",
  "input",
  "output",
  "over",
  "into",
  "start",
  "scale",
  "rounding",
  "comparator",
]);
const RULE_RENAMES: Record<string, string> = {
  on_missing: "onMissing",
  on_invalid: "onInvalid",
  ignore_case: "ignoreCase",
  from_timezone: "fromTimezone",
  to_timezone: "toTimezone",
};
/** A `require` entry's keys as the form names them. */
const REQUIRE_FIELDS: Record<string, string> = { count: "record", same: "path" };
const RULE_LISTS = new Set(["then", "else", "rules"]);

const withoutIndexes = (rest: string) => rest.replace(/\[\d+\]/g, "");
const firstIndex = (rest: string) => /\[(\d+)\]/.exec(rest)?.[0] ?? "";

// ── Endpoints ──────────────────────────────────────────────────────────────────────────

function endpointPath(index: string, rest: string): Translated {
  const prefix = `endpoints[${index}]`;
  if (rest === "" || rest === "settings") return prefix;
  const common = ENDPOINT_COMMON[rest];
  if (common !== undefined) return `${prefix}.${common}`;
  if (!rest.startsWith("settings.")) return null;
  const key = rest.slice("settings.".length);
  const field = ENDPOINT_FIELDS[withoutIndexes(key)];
  if (field === undefined) return null;
  return `${prefix}.${field}${LIST_FIELDS.has(field) ? firstIndex(key) : ""}`;
}

// ── Filters and mappings ───────────────────────────────────────────────────────────────

/** `filters[k]<rest>` of a list of expressions, as a path under `prefix`. */
function filterPath(
  prefix: string,
  index: string,
  rest: string,
  expressions: readonly WireExpression[] | undefined,
): Translated {
  const shown =
    expressions === undefined ? Number(index) : visibleFilterIndex(expressions, Number(index));
  if (shown === null) return null;
  const at = `${prefix === "" ? "" : `${prefix}.`}filters[${shown}]`;
  const inner = conditionTail(rest);
  return inner === null ? null : `${at}${inner}`;
}

/**
 * The rest of a path below a condition: a comparison's field (`.path`), or inside a combination
 * (`.all[1].path`, `.not.value`), which the form holds as `.items[1].path` and `.items[0].value`.
 */
function conditionTail(tail: string): Translated {
  if (tail === "") return "";
  const combination = /^\.(all|any|not)(?:\[(\d+)\])?(.*)$/.exec(tail);
  if (combination !== null) {
    const [, kind = "", index, rest = ""] = combination;
    return combinationTail(kind, index, rest);
  }
  const field = FILTER_FIELDS[withoutIndexes(tail).replace(/^\./, "")];
  return field === undefined ? null : `.${field}`;
}

/** Inside `all`/`any` (an item, or the list itself) or `not` (its one condition). */
function combinationTail(kind: string, index: string | undefined, rest: string): Translated {
  if (kind === "not") {
    const inner = index === undefined ? conditionTail(rest) : null;
    return inner === null ? null : `.items[0]${inner}`;
  }
  if (index === undefined) return rest === "" ? ".items" : null;
  const inner = conditionTail(rest);
  return inner === null ? null : `.items[${index}]${inner}`;
}

const ruleField = (name: string): string | null =>
  RULE_FIELDS.has(name) ? name : (RULE_RENAMES[name] ?? null);

/** `.steps[n]` and its field below a rule or a concat part; the step's `step` is `kind` here. */
function stepsPath(rest: string): Translated {
  const match = /^\.steps\[(\d+)\](?:\.(\w+))?$/.exec(rest);
  if (match === null) return null;
  const [, index = "", field] = match;
  if (field === undefined) return `.steps[${index}]`;
  return `.steps[${index}].${field === "step" ? "kind" : field}`;
}

/** `parts[n]`, `then[n]`, `else[n]`, `rules[n]` and `steps[n]` below a rule. */
function indexedRulePath(name: string, index: string, tail: string): Translated {
  if (name === "steps") return stepsPath(`.steps[${index}]${tail}`);
  if (name === "parts") {
    if (tail === "" || tail === ".from" || tail === ".text") return `.parts[${index}]${tail}`;
    const steps = stepsPath(tail);
    return steps === null ? null : `.parts[${index}]${steps}`;
  }
  if (!RULE_LISTS.has(name)) return null;
  const inner = rulePath(tail);
  return inner === null ? null : `.${name}[${index}]${inner}`;
}

/** The rest of a path below one rule (`.to`, `.then[0].to`, `.condition.op`, ...). */
function rulePath(rest: string): Translated {
  if (rest === "") return "";
  const match = /^\.(\w+)(?:\[(\d+)\])?(.*)$/.exec(rest);
  if (match === null) return null;
  const [, name = "", index, tail = ""] = match;
  if (index !== undefined) return indexedRulePath(name, index, tail);
  if (name === "condition") return conditionPath(tail);
  // A problem inside the formula (`.formula.mul[1]`) shows at the formula's one field.
  if (name === "formula") return ".formula";
  const field = ruleField(name);
  return field !== null && tail === "" ? `.${field}` : null;
}

function conditionPath(tail: string): Translated {
  const inner = conditionTail(tail);
  return inner === null ? null : `.condition${inner}`;
}

/** `.require`, `.require[1]` and `.require[1].count` below a mapping, as the form names them. */
function requirePath(prefix: string, rest: string): Translated {
  const match = /^\.require(?:\[(\d+)\](?:\.(\w+))?)?$/.exec(rest);
  if (match === null) return null;
  const [, index, field] = match;
  if (index === undefined) return `${prefix}.require`;
  const form = field === undefined ? "" : `.${REQUIRE_FIELDS[field] ?? field}`;
  return `${prefix}.require[${index}]${form}`;
}

function mappingPath(prefix: string, rest: string, hidden: boolean): Translated {
  if (hidden) return null;
  if (rest === "") return `${prefix}.mappings`;
  const tables = /^\.tables(?:\.([^.[]+))?/.exec(rest);
  if (tables !== null)
    return `${prefix}.codeTables${tables[1] === undefined ? "" : `.${tables[1]}`}`;
  if (rest === ".require_single_group") return `${prefix}.requireSingleGroup`;
  const requirement = requirePath(prefix, rest);
  if (requirement !== null) return requirement;
  const rules = /^\.rules(?:\[(\d+)\](.*))?$/.exec(rest);
  if (rules === null) return null;
  if (rules[1] === undefined) return `${prefix}.mappings`;
  const inner = rulePath(rules[2] ?? "");
  return inner === null ? null : `${prefix}.mappings[${rules[1]}]${inner}`;
}

// ── Connectors ─────────────────────────────────────────────────────────────────────────

/** Settings key (below `settings.`) to the console path below the connector's `settings`. */
/** The HL7 and ASTM settings the console shows, by the engine's key (`ack_mode` and `role` are handled below). */
const VARIANT_KEYS: Record<string, string> = {
  version: "hl7Version",
  require_end_cr: "requireEndCr",
  ack_message_types: "ackMessageTypes",
  reply_type: "replyType",
  max_frame_chars: "maxFrameChars",
  enq_attempts: "enqAttempts",
  busy_retry_ms: "busyRetryMs",
  sample_id_path: "sampleIdPath",
};

function settingsKey(key: string, protocol: string | undefined): Translated {
  const plain = withoutIndexes(key);
  const known = VARIANT_KEYS[plain];
  if (known !== undefined && protocol !== "au" && protocol !== "delimited_file") return known;
  if (plain === "ack_mode") return "ackMode";
  if (plain === "role") return "astmRole";
  const first = plain.split(".")[0] ?? "";
  if (DELIMITED_KEYS.has(first) || (protocol === "delimited_file" && first === "charset")) {
    return "templateJson";
  }
  if (protocol === "hl7_mllp" || protocol === "astm") return null;
  const au = AU_FIELDS[plain];
  if (au === undefined) return null;
  const indexed = au === "patientInfoDigits" ? firstIndex(key) : "";
  return au === "" ? "au" : `au.${au}${indexed}`;
}

function connectorPath(prefix: string, rest: string, protocol: string | undefined): Translated {
  if (rest === "" || rest === "settings") return rest === "" ? prefix : `${prefix}.settings`;
  if (rest === "endpoint") return `${prefix}.endpointId`;
  if (rest === "protocol") return `${prefix}.protocol`;
  if (rest === "duplicate_detection") return `${prefix}.duplicateDetection`;
  if (!rest.startsWith("settings.")) return null;
  const key = settingsKey(rest.slice("settings.".length), protocol);
  return key === null ? null : `${prefix}.settings.${key}`;
}

function destinationPath(
  index: string,
  rest: string,
  channel: WireChannel | undefined,
): Translated {
  const prefix = `destinations[${index}]`;
  const destination: WireDestination | undefined =
    channel?.kind === "message" ? channel.destinations[Number(index)] : undefined;
  if (rest === "") return prefix;
  const own = DESTINATION_FIELDS[rest];
  if (own !== undefined) return `${prefix}.${own}`;
  const filter = /^filters\[(\d+)\](.*)$/.exec(rest);
  if (filter !== null) {
    return filterPath(prefix, filter[1] ?? "", filter[2] ?? "", destination?.filters);
  }
  if (rest === "mapping" || rest.startsWith("mapping.")) {
    const hidden = destination?.mapping != null && mappingFromWire(destination.mapping) === null;
    return mappingPath(prefix, rest.slice("mapping".length), hidden);
  }
  return connectorPath(prefix, rest, destination?.protocol);
}

// ── Channels ───────────────────────────────────────────────────────────────────────────

function messageRest(rest: string, channel: WireChannel | undefined): Translated {
  const message = channel?.kind === "message" ? channel : undefined;
  if (rest === "source" || rest.startsWith("source.")) {
    return connectorPath(
      "source",
      rest.slice("source".length).replace(/^\./, ""),
      message?.source.protocol,
    );
  }
  const destination = /^destinations\[(\d+)\](?:\.(.*))?$/.exec(rest);
  if (destination !== null) {
    return destinationPath(destination[1] ?? "", destination[2] ?? "", channel);
  }
  const filter = /^filters\[(\d+)\](.*)$/.exec(rest);
  if (filter !== null) return filterPath("", filter[1] ?? "", filter[2] ?? "", message?.filters);
  return null;
}

function channelRest(rest: string, channel: WireChannel | undefined): Translated {
  const field = CHANNEL_FIELDS[rest];
  if (field !== undefined) return field;
  return messageRest(rest, channel);
}

// ── Entry point ────────────────────────────────────────────────────────────────────────

/** `channels[i]<rest>`; the channel's own document is `raw`, when known. */
function channelPath(index: string, rest: string, raw: WireChannel | undefined): Translated {
  if (rest === "") return `channels[${index}]`;
  const translated = channelRest(rest, raw);
  return translated === null ? null : `channels[${index}].${translated}`;
}

/** The console path of an engine field path. */
export function toUiPath(path: string, document?: WireConfig): string {
  const endpoint = /^endpoints\[(\d+)\](?:\.(.*))?$/.exec(path);
  if (endpoint !== null) return endpointPath(endpoint[1] ?? "", endpoint[2] ?? "") ?? path;
  const channel = /^channels\[(\d+)\](?:\.(.*))?$/.exec(path);
  if (channel !== null) {
    const index = channel[1] ?? "";
    const raw = document?.channels?.[Number(index)];
    return channelPath(index, channel[2] ?? "", raw) ?? path;
  }
  const alert = /^alerts\.(\w+)$/.exec(path);
  if (alert !== null) {
    const field = ALERT_FIELDS[alert[1] ?? ""];
    return field === undefined ? path : `alertThresholds.${field}`;
  }
  return path === "alerts" ? "alertThresholds" : path;
}
