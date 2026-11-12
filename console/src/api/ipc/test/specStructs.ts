/**
 * The fields of every struct of the runtime document and its protocol settings (docs/spec/
 * configuration.md sections 4 to 8): the names the engine knows, and those without a default.
 */

interface Struct {
  known: string[];
  required?: string[];
}

/** A struct whose every field is required. */
const allRequired = (known: string[]): Struct => ({ known, required: known });

const RETRY = [
  "initial_delay_ms",
  "max_delay_ms",
  "multiplier",
  "jitter_percent",
  "max_consecutive_failures_before_alert",
];
const RETENTION = ["max_age_days", "max_messages"];
const KEEP_ALIVE = [
  "keep_alive",
  "keep_alive_time_sec",
  "keep_alive_interval_sec",
  "keep_alive_retry_count",
];
const FILES = [
  "poll_interval_seconds",
  "file_pattern",
  "recursive",
  "ignore_suffixes",
  "stable_seconds",
  "on_consumed",
  "processed_directory",
  "write_mode",
];

export const STRUCTS: Record<string, Struct> = {
  document: {
    known: [
      "schema_version",
      "config_version",
      "generated_at",
      "diagnostics",
      "alerts",
      "endpoints",
      "channels",
    ],
  },
  endpoint: {
    known: ["endpoint_id", "label", "enabled", "kind", "settings", "tags"],
    required: ["endpoint_id", "label", "kind", "settings"],
  },
  serial: {
    known: [
      "port_name",
      "baud_rate",
      "data_bits",
      "parity",
      "stop_bits",
      "flow_control",
      "read_timeout_ms",
      "write_timeout_ms",
      "virtual_link",
    ],
    required: ["port_name", "baud_rate"],
  },
  tcp_client: {
    known: ["host", "port", "connect_timeout_ms", "idle_timeout_seconds", ...KEEP_ALIVE],
    required: ["host", "port"],
  },
  tcp_listener: {
    known: [
      "bind_address",
      "port",
      "accept_backlog",
      "allowed_peer_cidrs",
      "allow_any_peer",
      "idle_timeout_seconds",
      ...KEEP_ALIVE,
    ],
    required: ["bind_address", "port"],
  },
  folder: {
    known: ["path", "mode", "exclusive_open_check", ...FILES],
    required: ["path"],
  },
  sftp: {
    known: [
      "host",
      "port",
      "username",
      "auth",
      "host_key_fingerprint",
      "remote_directory",
      "connect_timeout_ms",
      "operation_timeout_ms",
      ...FILES,
    ],
    required: ["host", "username", "auth", "host_key_fingerprint", "remote_directory"],
  },
  password: { known: ["method", "password"], required: ["method", "password"] },
  key: {
    known: ["method", "private_key", "passphrase"],
    required: ["method", "private_key"],
  },
  secret: { known: ["secret"], required: ["secret"] },
  bridge: {
    known: [
      "channel_id",
      "label",
      "kind",
      "enabled",
      "tags",
      "endpoint_a",
      "endpoint_b",
      "connection",
      "buffer",
      "retry",
      "tap",
    ],
    required: ["channel_id", "label", "kind", "endpoint_a", "endpoint_b"],
  },
  connection: { known: ["connect_strategy", "receiver", "pairing_timeout_ms"] },
  buffer: {
    known: ["max_buffered_segments", "max_bytes", "overflow_action", "block_timeout_ms"],
  },
  retry: { known: RETRY },
  tap: {
    known: ["framer", "record_messages", "max_message_bytes", "retention"],
  },
  tapRetention: { known: RETENTION },
  message: {
    known: [
      "channel_id",
      "label",
      "kind",
      "enabled",
      "tags",
      "source",
      "filters",
      "destinations",
      "retry",
      "delivery",
      "retention",
    ],
    required: ["channel_id", "label", "kind", "source", "destinations"],
  },
  source: {
    known: ["endpoint", "protocol", "settings", "duplicate_detection"],
    required: ["endpoint", "protocol"],
  },
  destination: {
    known: [
      "destination_id",
      "label",
      "enabled",
      "endpoint",
      "protocol",
      "settings",
      "filters",
      "mapping",
      "delivery",
    ],
    required: ["destination_id", "label", "endpoint", "protocol"],
  },
  delivery: {
    known: [
      "max_attempts",
      "retry",
      "send_timeout_seconds",
      "ack_timeout_seconds",
      "ack_required",
      "retry_unacknowledged",
    ],
  },
  mapping: {
    known: ["tables", "require_single_group", "require", "rules"],
    required: ["rules"],
  },
  retention: { known: RETENTION },
  alerts: {
    known: [
      "endpoint_down_seconds",
      "endpoint_down_critical_seconds",
      "no_peer_seconds",
      "buffer_high_percent",
      "buffer_high_seconds",
      "delivery_backlog_count",
      "delivery_backlog_age_seconds",
      "delivery_attention_critical_count",
      "invalid_messages_count",
      "invalid_messages_window_seconds",
      "disk_low_megabytes",
      "disk_critical_megabytes",
    ],
  },
  // Protocol settings (section 7).
  astm: {
    known: [
      "charset",
      "role",
      "ack_timeout_ms",
      "receive_timeout_ms",
      "max_frame_retries",
      "max_frame_chars",
      "contention_wait_ms",
      "max_message_bytes",
      "enq_attempts",
      "busy_retry_ms",
      "sample_id_path",
    ],
  },
  au: {
    known: ["profile", "framing", "format", "frame_timeout_ms", "block_timeout_ms"],
    required: ["profile", "framing", "format", "frame_timeout_ms"],
  },
  au_framing: allRequired(["start", "end", "bcc", "use_etb", "max_wire_len", "class"]),
  au_format: {
    known: [
      "unit_no",
      "rack_digits",
      "sample_id_digits",
      "sex",
      "age",
      "month",
      "patient_info_digits",
      "result_digits",
      "test_code_digits",
    ],
    required: [
      "unit_no",
      "rack_digits",
      "sample_id_digits",
      "sex",
      "age",
      "month",
      "patient_info_digits",
      "result_digits",
    ],
  },
  hl7_mllp: {
    known: [
      "ack_mode",
      "send_application_ack",
      "version",
      "sending_application",
      "sending_facility",
      "receiving_application",
      "receiving_facility",
      "processing_id",
      "charset",
      "max_message_bytes",
      "require_end_cr",
      "ack_message_types",
      "reply_type",
      "frame_timeout_ms",
    ],
  },
  delimited_file: {
    known: [
      "delimiter",
      "quote",
      "header_row",
      "skip_lines",
      "charset",
      "line_ending",
      "write_bom",
      "columns",
      "record_mode",
      "group_by",
      "max_file_bytes",
      "file_name",
      "on_exists",
    ],
    required: ["columns"],
  },
  raw: { known: [] },
  // Mapping rules (section 8.2).
  copy: {
    known: ["op", "from", "steps", "to", "default", "required"],
    required: ["op", "from", "to"],
  },
  constant: allRequired(["op", "value", "to"]),
  lookup: {
    known: ["op", "from", "steps", "table", "to", "on_missing", "default", "ignore_case"],
    required: ["op", "from", "table", "to"],
  },
  concat: allRequired(["op", "parts", "to"]),
  datetime: {
    known: ["op", "from", "steps", "to", "input", "output", "from_timezone", "to_timezone"],
    required: ["op", "from", "to", "input", "output"],
  },
  when: {
    known: ["op", "condition", "then", "else"],
    required: ["op", "condition", "then"],
  },
  for_each: {
    known: ["op", "over", "into", "mode", "rules"],
    required: ["op", "over", "into", "rules"],
  },
  append: allRequired(["op", "into", "rules"]),
  calculate: {
    known: [
      "op",
      "formula",
      "to",
      "scale",
      "rounding",
      "comparator",
      "on_invalid",
      "default",
      "required",
      "notation",
    ],
    required: ["op", "formula", "to"],
  },
  sequence: { known: ["op", "to", "start"], required: ["op", "to"] },
  // A concat part (section 8.2): a `from` with optional steps, or a `text`.
  part: { known: ["from", "steps", "text"] },
  // Mapping preconditions (section 8.2, `require`).
  require_count: { known: ["count", "min", "max"], required: ["count"] },
  require_same: allRequired(["same"]),
  // Text steps (section 8.2), by step name.
  step_trim: allRequired(["step"]),
  step_upper: allRequired(["step"]),
  step_lower: allRequired(["step"]),
  step_strip_leading: allRequired(["step", "chars"]),
  step_split: allRequired(["step", "separator", "take"]),
  step_slice: {
    known: ["step", "start", "length"],
    required: ["step", "start"],
  },
  step_replace: allRequired(["step", "find", "with"]),
  // Calculation operands (section 8.2, Calculations).
  formula_from: { known: ["from", "steps"], required: ["from"] },
  formula_find: {
    known: ["find", "where", "from", "steps"],
    required: ["find", "where", "from"],
  },
  comparison: {
    known: ["path", "op", "value", "ignore_case"],
    required: ["path", "op"],
  },
};
