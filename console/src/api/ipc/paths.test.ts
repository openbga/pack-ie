import { describe, expect, it } from "vitest";
import { toUiPath } from "./paths";
import type { WireConfig } from "./wire";

type Row = [engine: string, ui: string];

/** Rows that share a prefix: `rows` are the rest of each path after `engine` and `ui`. */
const under = (engine: string, ui: string, rows: Row[]): Row[] =>
  rows.map(([e, u]) => [`${engine}${e}`, `${ui}${u}`]);

/** Engine paths as docs/spec/configuration.md section 9 names them, and the console's fields. */
const TABLE: Row[] = [
  // endpoints
  ["endpoints[0].endpoint_id", "endpoints[0].id"],
  ["endpoints[2].label", "endpoints[2].label"],
  ["endpoints[2].enabled", "endpoints[2].enabled"],
  ["endpoints[2].kind", "endpoints[2].kind"],
  ["endpoints[1]", "endpoints[1]"],
  ["endpoints[1].settings", "endpoints[1]"],
  ["endpoints[1].settings.port_name", "endpoints[1].port"],
  ["endpoints[1].settings.baud_rate", "endpoints[1].baudRate"],
  ["endpoints[1].settings.data_bits", "endpoints[1].dataBits"],
  ["endpoints[1].settings.parity", "endpoints[1].parity"],
  ["endpoints[1].settings.stop_bits", "endpoints[1].stopBits"],
  ["endpoints[1].settings.flow_control", "endpoints[1].flowControl"],
  ["endpoints[3].settings.host", "endpoints[3].host"],
  ["endpoints[3].settings.port", "endpoints[3].port"],
  ["endpoints[3].settings.connect_timeout_ms", "endpoints[3].connectTimeoutMs"],
  ["endpoints[3].settings.idle_timeout_seconds", "endpoints[3].idleTimeoutSeconds"],
  ["endpoints[3].settings.keep_alive", "endpoints[3].keepalive"],
  ["endpoints[3].settings.keep_alive_time_sec", "endpoints[3].keepalive"],
  ["endpoints[3].settings.keep_alive_interval_sec", "endpoints[3].keepalive"],
  ["endpoints[3].settings.keep_alive_retry_count", "endpoints[3].keepalive"],
  ["endpoints[4].settings.bind_address", "endpoints[4].bindAddress"],
  ["endpoints[4].settings.allowed_peer_cidrs", "endpoints[4].allowlist"],
  ["endpoints[4].settings.allowed_peer_cidrs[2]", "endpoints[4].allowlist[2]"],
  ["endpoints[4].settings.allow_any_peer", "endpoints[4].allowAnyPeer"],
  ["endpoints[5].settings.path", "endpoints[5].path"],
  ["endpoints[5].settings.file_pattern", "endpoints[5].filePattern"],
  ["endpoints[5].settings.poll_interval_seconds", "endpoints[5].pollIntervalSec"],
  ["endpoints[5].settings.mode", "endpoints[5].mode"],
  ["endpoints[6].settings.username", "endpoints[6].username"],
  ["endpoints[6].settings.auth", "endpoints[6].authMethod"],
  ["endpoints[6].settings.auth.method", "endpoints[6].authMethod"],
  ["endpoints[6].settings.auth.password", "endpoints[6].secretName"],
  ["endpoints[6].settings.auth.password.secret", "endpoints[6].secretName"],
  ["endpoints[6].settings.auth.private_key", "endpoints[6].secretName"],
  ["endpoints[6].settings.auth.passphrase", "endpoints[6].passphraseSecretName"],
  ["endpoints[6].settings.host_key_fingerprint", "endpoints[6].hostKeyFingerprint"],
  ["endpoints[6].settings.remote_directory", "endpoints[6].remotePath"],
  // endpoint settings the console does not model stay as the engine names them
  ["endpoints[1].settings.read_timeout_ms", "endpoints[1].settings.read_timeout_ms"],
  ["endpoints[4].settings.accept_backlog", "endpoints[4].settings.accept_backlog"],
  ["endpoints[5].settings.processed_directory", "endpoints[5].settings.processed_directory"],
  ["endpoints[6].settings.operation_timeout_ms", "endpoints[6].settings.operation_timeout_ms"],
  ["endpoints[1].settings.virtual_link", "endpoints[1].settings.virtual_link"],
  ["endpoints[1].tags", "endpoints[1].tags"],
  // channels: common
  ["channels[0]", "channels[0]"],
  ["channels[0].channel_id", "channels[0].id"],
  ["channels[0].label", "channels[0].label"],
  ["channels[0].enabled", "channels[0].enabled"],
  ["channels[0].kind", "channels[0].kind"],
  // bridge
  ["channels[0].endpoint_a", "channels[0].endpointAId"],
  ["channels[0].endpoint_b", "channels[0].endpointBId"],
  ["channels[2].connection.receiver", "channels[2].pairingReceiver"],
  ["channels[0].connection.connect_strategy", "channels[0].pairing"],
  ["channels[0].connection.pairing_timeout_ms", "channels[0].connection.pairing_timeout_ms"],
  ["channels[0].buffer.max_bytes", "channels[0].bufferBytes"],
  ["channels[0].buffer.max_buffered_segments", "channels[0].buffer.max_buffered_segments"],
  ["channels[0].retry.initial_delay_ms", "channels[0].retry.initialMs"],
  ["channels[0].retry.max_delay_ms", "channels[0].retry.maxMs"],
  ["channels[0].retry.multiplier", "channels[0].retry.multiplier"],
  ["channels[0].tap", "channels[0].tapFramer"],
  ["channels[0].tap.framer", "channels[0].tapFramer"],
  ["channels[0].tap.record_messages", "channels[0].tapFramer"],
  ["channels[0].tap.max_message_bytes", "channels[0].tap.max_message_bytes"],
  ["channels[0].tap.retention", "channels[0].tapRetention"],
  ["channels[0].tap.retention.max_age_days", "channels[0].tapRetention.days"],
  ["channels[0].tap.retention.max_messages", "channels[0].tapRetention.maxMessages"],
  // message channels: source
  ["channels[1].source", "channels[1].source"],
  ["channels[1].source.endpoint", "channels[1].source.endpointId"],
  ["channels[1].source.protocol", "channels[1].source.protocol"],
  ["channels[1].source.settings", "channels[1].source.settings"],
  ["channels[1].source.duplicate_detection", "channels[1].source.duplicateDetection"],
  ["channels[1].source.settings.ack_mode", "channels[1].source.settings.ackMode"],
  ["channels[1].source.settings.role", "channels[1].source.settings.astmRole"],
  ["channels[1].source.settings.sample_id_path", "channels[1].source.settings.sampleIdPath"],
  ["channels[1].source.settings.max_frame_chars", "channels[1].source.settings.maxFrameChars"],
  [
    "channels[1].destinations[0].settings.enq_attempts",
    "channels[1].destinations[0].settings.enqAttempts",
  ],
  [
    "channels[1].destinations[0].settings.version",
    "channels[1].destinations[0].settings.hl7Version",
  ],
  ["channels[1].source.settings.require_end_cr", "channels[1].source.settings.requireEndCr"],
  // AU settings
  ...under("channels[1].source.settings", "channels[1].source.settings.au", [
    [".profile", ".profile"],
    [".framing", ""],
    [".framing.start", ".framingStart"],
    [".framing.start[1]", ".framingStart"],
    [".framing.end", ".framingEnd"],
    [".framing.bcc", ".bcc"],
    [".framing.use_etb", ".useEtb"],
    [".framing.max_wire_len", ".maxWireLen"],
    [".framing.class", ".framingClass"],
    [".format.unit_no", ".unitNo"],
    [".format.rack_digits", ".rackDigits"],
    [".format.sample_id_digits", ".sampleIdDigits"],
    [".format.sex", ".sex"],
    [".format.age", ".age"],
    [".format.month", ".month"],
    [".format.patient_info_digits[2]", ".patientInfoDigits[2]"],
    [".format.result_digits", ".resultDigits"],
    [".frame_timeout_ms", ".frameTimeoutMs"],
    [".block_timeout_ms", ".blockTimeoutMs"],
  ]),
  // delimited file settings are one JSON text in the console
  ["channels[1].source.settings.columns", "channels[1].source.settings.templateJson"],
  ["channels[1].source.settings.columns[0].name", "channels[1].source.settings.templateJson"],
  ["channels[1].source.settings.delimiter", "channels[1].source.settings.templateJson"],
  ["channels[1].source.settings.group_by", "channels[1].source.settings.templateJson"],
  // protocol settings the console does not model
  ["channels[1].source.settings.ack_timeout_ms", "channels[1].source.settings.ack_timeout_ms"],
  [
    "channels[1].source.settings.max_message_bytes",
    "channels[1].source.settings.max_message_bytes",
  ],
  // message channels: channel-level filters and retention
  ["channels[1].filters[0].path", "channels[1].filters[0].path"],
  ["channels[1].filters[2].op", "channels[1].filters[2].operator"],
  ["channels[1].filters[2].value", "channels[1].filters[2].value"],
  ["channels[1].filters[2].value[1]", "channels[1].filters[2].value"],
  ["channels[1].filters[2].ignore_case", "channels[1].filters[2].ignoreCase"],
  ["channels[1].filters[2]", "channels[1].filters[2]"],
  // Combinations are held as `items` of the condition.
  ["channels[1].filters[2].all", "channels[1].filters[2].items"],
  ["channels[1].filters[2].all[1].path", "channels[1].filters[2].items[1].path"],
  ["channels[1].filters[2].any[0].all[2].value", "channels[1].filters[2].items[0].items[2].value"],
  ["channels[1].filters[2].not", "channels[1].filters[2].items[0]"],
  ["channels[1].filters[2].not.any[1].op", "channels[1].filters[2].items[0].items[1].operator"],
  ["channels[1].retention.max_age_days", "channels[1].retention.days"],
  ["channels[1].retention.max_messages", "channels[1].retention.maxMessages"],
  ["channels[1].destinations", "channels[1].destinations"],
  ["channels[1].delivery.max_attempts", "channels[1].delivery.max_attempts"],
  // destinations
  ["channels[1].destinations[0]", "channels[1].destinations[0]"],
  ...under("channels[1].destinations[0]", "channels[1].destinations[0]", [
    [".destination_id", ".id"],
    [".label", ".label"],
    [".endpoint", ".endpointId"],
    [".protocol", ".protocol"],
    [".settings.ack_mode", ".settings.ackMode"],
    [".filters[1].path", ".filters[1].path"],
    [".filters[1].op", ".filters[1].operator"],
    [".delivery.max_attempts", ".retry.maxAttempts"],
    [".delivery.retry.initial_delay_ms", ".retry.initialMs"],
    [".delivery.retry.max_delay_ms", ".retry.maxMs"],
    [".delivery.ack_required", ".delivery.ack_required"],
    // mappings
    [".mapping", ".mappings"],
    [".mapping.rules", ".mappings"],
    [".mapping.require", ".require"],
    [".mapping.require[1]", ".require[1]"],
    [".mapping.require[1].count", ".require[1].record"],
    [".mapping.require[2].same", ".require[2].path"],
    [".mapping.require[0].max", ".require[0].max"],
    [".mapping.require_single_group", ".requireSingleGroup"],
    [".mapping.rules[3]", ".mappings[3]"],
    [".mapping.rules[3].condition.all[1].value", ".mappings[3].condition.items[1].value"],
    [".mapping.rules[3].condition.not.path", ".mappings[3].condition.items[0].path"],
    [".mapping.rules[3].to", ".mappings[3].to"],
    [".mapping.rules[3].from", ".mappings[3].from"],
    [".mapping.rules[0].default", ".mappings[0].default"],
    [".mapping.rules[0].op", ".mappings[0].op"],
    [".mapping.rules[2].table", ".mappings[2].table"],
    [".mapping.rules[2].on_missing", ".mappings[2].onMissing"],
    [".mapping.rules[2].ignore_case", ".mappings[2].ignoreCase"],
    [".mapping.rules[0].steps[2]", ".mappings[0].steps[2]"],
    [".mapping.rules[0].steps[2].separator", ".mappings[0].steps[2].separator"],
    [".mapping.rules[0].steps[1].step", ".mappings[0].steps[1].kind"],
    [".mapping.rules[4].parts[1].steps[0].chars", ".mappings[4].parts[1].steps[0].chars"],
    [".mapping.rules[3].rules[1].to", ".mappings[3].rules[1].to"],
    [".mapping.rules[3].into", ".mappings[3].into"],
    [".mapping.rules[6].formula.sub[1].where.path", ".mappings[6].formula"],
    [".mapping.rules[6].formula.sub[0].steps[1].take", ".mappings[6].formula"],
    [".mapping.rules[6].formula", ".mappings[6].formula"],
    [".mapping.rules[6].formula.mul[1]", ".mappings[6].formula"],
    [".mapping.rules[6].formula.div[0].from", ".mappings[6].formula"],
    [".mapping.rules[6].scale", ".mappings[6].scale"],
    [".mapping.rules[6].rounding", ".mappings[6].rounding"],
    [".mapping.rules[6].comparator", ".mappings[6].comparator"],
    [".mapping.rules[6].on_invalid", ".mappings[6].onInvalid"],
    [".mapping.rules[4].parts[1].text", ".mappings[4].parts[1].text"],
    [".mapping.rules[4].parts[0].from", ".mappings[4].parts[0].from"],
    [".mapping.rules[5].from_timezone", ".mappings[5].fromTimezone"],
    [".mapping.rules[5].to_timezone", ".mappings[5].toTimezone"],
    [".mapping.rules[5].input", ".mappings[5].input"],
    [".mapping.rules[6].condition.path", ".mappings[6].condition.path"],
    [".mapping.rules[6].condition.op", ".mappings[6].condition.operator"],
    [".mapping.rules[6].then[0].to", ".mappings[6].then[0].to"],
    [".mapping.rules[6].else[1].value", ".mappings[6].else[1].value"],
    [".mapping.rules[7].over", ".mappings[7].over"],
    [".mapping.rules[7].rules[2].to", ".mappings[7].rules[2].to"],
    [".mapping.rules[7].rules[0].start", ".mappings[7].rules[0].start"],
    [".mapping.tables", ".codeTables"],
    [".mapping.tables.flags", ".codeTables.flags"],
    [".mapping.tables.flags.H", ".codeTables.flags"],
  ]),
  ["channels[1].destinations[1].enabled", "channels[1].destinations[1].enabled"],
  // alerts
  ["alerts", "alertThresholds"],
  ["alerts.endpoint_down_seconds", "alertThresholds.endpointDownSeconds"],
  ["alerts.delivery_backlog_count", "alertThresholds.queueDepthWarning"],
  ["alerts.buffer_high_percent", "alertThresholds.bufferPressurePercent"],
  ["alerts.disk_low_megabytes", "alertThresholds.diskFreeMinMib"],
  ["alerts.no_peer_seconds", "alerts.no_peer_seconds"],
  // the document as a whole, and what is not the console's
  ["", ""],
  ["schema_version", "schema_version"],
  ["diagnostics.retain_recent_errors", "diagnostics.retain_recent_errors"],
  ["something[0].else", "something[0].else"],
];

describe("toUiPath", () => {
  it.each(TABLE)("%s is %s", (engine, ui) => {
    expect(toUiPath(engine)).toBe(ui);
  });

  it("passes unknown paths of known scopes through", () => {
    expect(toUiPath("channels[1].unheard_of")).toBe("channels[1].unheard_of");
    expect(toUiPath("endpoints[0].settings.unheard_of")).toBe("endpoints[0].settings.unheard_of");
    expect(toUiPath("channels[1].destinations[0].unheard_of")).toBe(
      "channels[1].destinations[0].unheard_of",
    );
    expect(toUiPath("channels[1].destinations[0].mapping.rules[0].unheard_of")).toBe(
      "channels[1].destinations[0].mapping.rules[0].unheard_of",
    );
  });
});

describe("toUiPath with the document the engine checked", () => {
  const document: WireConfig = {
    channels: [
      {
        channel_id: "m",
        label: "M",
        kind: "message",
        source: { endpoint: "a", protocol: "au" },
        filters: [
          { path: "X", op: "near", value: "x" },
          { path: "A", op: "exists" },
          { path: "B", op: "equals", value: "1" },
        ],
        destinations: [
          {
            destination_id: "d0",
            label: "D0",
            endpoint: "b",
            protocol: "hl7_mllp",
            settings: {},
            filters: [
              { path: "A", op: "exists" },
              { path: "B", op: "near" },
            ],
            mapping: { rules: [{ op: "copy", from: "A", to: "B" }] },
          },
          {
            destination_id: "d1",
            label: "D1",
            endpoint: "b",
            protocol: "delimited_file",
            // A mapping the console cannot show: its rules are not the console's.
            mapping: { rules: [{ op: "explode", to: "A" }] },
          },
          { destination_id: "d2", label: "D2", endpoint: "b", protocol: "astm" },
        ],
      },
      {
        channel_id: "b",
        label: "B",
        kind: "bridge",
        endpoint_a: "x",
        endpoint_b: "y",
      },
    ],
  };

  it("counts only the filters the console shows", () => {
    expect(toUiPath("channels[0].filters[1].path", document)).toBe("channels[0].filters[0].path");
    expect(toUiPath("channels[0].filters[2].op", document)).toBe("channels[0].filters[1].operator");
    expect(toUiPath("channels[0].destinations[0].filters[0].path", document)).toBe(
      "channels[0].destinations[0].filters[0].path",
    );
  });

  it("keeps the engine path of a filter the console does not show", () => {
    expect(toUiPath("channels[0].filters[0].path", document)).toBe("channels[0].filters[0].path");
    expect(toUiPath("channels[0].destinations[0].filters[1].op", document)).toBe(
      "channels[0].destinations[0].filters[1].op",
    );
  });

  it("keeps the engine path below a mapping the console does not show", () => {
    expect(toUiPath("channels[0].destinations[1].mapping.rules[0].to", document)).toBe(
      "channels[0].destinations[1].mapping.rules[0].to",
    );
    expect(toUiPath("channels[0].destinations[0].mapping.rules[0].to", document)).toBe(
      "channels[0].destinations[0].mappings[0].to",
    );
  });

  it("reads settings keys in the protocol of their connector", () => {
    // `frame_timeout_ms` is an AU field here, and an HL7 field the console does not model there.
    expect(toUiPath("channels[0].source.settings.frame_timeout_ms", document)).toBe(
      "channels[0].source.settings.au.frameTimeoutMs",
    );
    expect(toUiPath("channels[0].destinations[0].settings.frame_timeout_ms", document)).toBe(
      "channels[0].destinations[0].settings.frame_timeout_ms",
    );
    expect(toUiPath("channels[0].destinations[1].settings.charset", document)).toBe(
      "channels[0].destinations[1].settings.templateJson",
    );
    expect(toUiPath("channels[0].destinations[0].settings.charset", document)).toBe(
      "channels[0].destinations[0].settings.charset",
    );
  });

  it("translates bridge fields", () => {
    expect(toUiPath("channels[1].connection.receiver", document)).toBe(
      "channels[1].pairingReceiver",
    );
  });
});

/**
 * Problems a running engine reported for deliberately broken documents (validate_config of a
 * bridge between a serial port, listeners and clients), as `[code, engine path, console path]`.
 */
const REAL_PROBLEMS: [string, string, string][] = [
  ["schema", "endpoints[0].settings.baud_rate", "endpoints[0].baudRate"],
  ["schema", "endpoints[0].settings.data_bits", "endpoints[0].dataBits"],
  ["schema", "endpoints[2].settings.host", "endpoints[2].host"],
  ["schema", "endpoints[2].settings.port", "endpoints[2].port"],
  ["schema", "endpoints[0].label", "endpoints[0].label"],
  ["schema", "endpoints[0].endpoint_id", "endpoints[0].id"],
  ["schema", "alerts.endpoint_down_seconds", "alertThresholds.endpointDownSeconds"],
  ["schema", "channels[0].buffer.max_bytes", "channels[0].bufferBytes"],
  ["schema", "channels[0].retention.max_age_days", "channels[0].retention.days"],
  ["schema", "channels[0].source.settings.ack_mode", "channels[0].source.settings.ackMode"],
  [
    "schema",
    "channels[0].destinations[0].delivery.max_attempts",
    "channels[0].destinations[0].retry.maxAttempts",
  ],
  ["same_endpoint", "channels[0].endpoint_b", "channels[0].endpointBId"],
  ["unknown_endpoint", "channels[0].endpoint_b", "channels[0].endpointBId"],
  ["unknown_endpoint", "channels[0].source.endpoint", "channels[0].source.endpointId"],
  ["endpoint_kind_not_allowed", "channels[0].endpoint_b", "channels[0].endpointBId"],
  ["endpoint_disabled", "channels[0].endpoint_a", "channels[0].endpointAId"],
  ["receiver_required", "channels[0].connection.receiver", "channels[0].pairingReceiver"],
  ["allowlist_required", "endpoints[1].settings.allowed_peer_cidrs", "endpoints[1].allowlist"],
  ["invalid_cidr", "endpoints[1].settings.allowed_peer_cidrs[0]", "endpoints[1].allowlist[0]"],
  ["retry_invalid", "channels[0].retry.initial_delay_ms", "channels[0].retry.initialMs"],
  ["tap_invalid", "channels[0].tap.record_messages", "channels[0].tapFramer"],
  ["duplicate_id", "endpoints[4].endpoint_id", "endpoints[4].id"],
  ["duplicate_id", "channels[0].destinations[1].destination_id", "channels[0].destinations[1].id"],
  ["port_conflict", "endpoints[4].settings.port", "endpoints[4].port"],
  ["port_conflict", "endpoints[4].settings.port_name", "endpoints[4].port"],
  [
    "passthrough_protocol",
    "channels[0].destinations[0].mapping",
    "channels[0].destinations[0].mappings",
  ],
  [
    "ack_level_unreachable",
    "channels[0].destinations[0].delivery.ack_required",
    "channels[0].destinations[0].delivery.ack_required",
  ],
  ["secret_inline", "endpoints[4].settings.auth.password", "endpoints[4].secretName"],
  // A warning about the document as a whole, and one about a whole channel.
  ["shared_exclusive_endpoint", "channels", "channels"],
  ["retention_unbounded", "channels[0].retention", "channels[0].retention"],
  ["retention_unbounded", "channels[0].tap.retention", "channels[0].tapRetention"],
];

describe("toUiPath for the paths a real engine reports", () => {
  it.each(REAL_PROBLEMS)("%s at %s", (_code, enginePath, uiPath) => {
    expect(toUiPath(enginePath)).toBe(uiPath);
  });
});
