/**
 * Test data: a fully populated runtime document (sparse, as the engine accepts it) and the
 * identifiers the other fixtures refer to. `../real/` holds JSON the running engine produced.
 */

import type { WireConfig } from "../wire";

export const IDS = {
  serial: "ep-serial",
  listener: "ep-listener",
  client: "ep-client",
  folder: "ep-folder",
  sftp: "ep-sftp",
  folderOut: "ep-folder-out",
  auListener: "ep-au",
  bridge: "ch-bridge",
  message: "ch-message",
  au: "ch-au",
  hl7: "d-hl7",
  csv: "d-csv",
  passthrough: "d-pass",
};

/** The mapping the rich message channel's HL7 destination carries, in the specified JSON. */
export const MAPPING = {
  tables: { sex: { M: "male", F: "female" }, empty: {} },
  rules: [
    { op: "copy", from: "PID.3", to: "R.3", default: "n/a", required: true },
    { op: "copy", from: "PID.5", to: "R.4" },
    { op: "constant", value: "LAB", to: "H.5" },
    {
      op: "lookup",
      from: "PID.8",
      table: "sex",
      to: "P.8",
      on_missing: "default",
      default: "unknown",
    },
    { op: "concat", parts: [{ from: "PID.5.1" }, { text: ", " }, { from: "PID.5.2" }], to: "P.6" },
    {
      op: "datetime",
      from: "OBR.7",
      to: "O.7",
      input: "yyyyMMddHHmmss",
      output: "yyyy-MM-dd HH:mm",
      from_timezone: "UTC",
      to_timezone: "Europe/Berlin",
    },
    {
      op: "when",
      condition: { path: "PV1.2", op: "in", value: ["I", "E"], ignore_case: true },
      // biome-ignore lint/suspicious/noThenProperty: the specified JSON names the rule field `then`
      then: [{ op: "constant", value: "inpatient", to: "P.9" }],
      else: [{ op: "constant", value: "outpatient", to: "P.9" }],
    },
    {
      op: "for_each",
      over: "OBX",
      into: "R",
      rules: [
        { op: "sequence", to: "$.1" },
        { op: "copy", from: "$.5", to: "$.4" },
        { op: "lookup", from: "$.7", table: "sex", to: "$.8", on_missing: "skip" },
      ],
    },
    { op: "sequence", to: "L.2", start: 7 },
  ],
};

/** A delimited destination's settings, as the engine has them (docs/spec/configuration.md 7.4). */
export const DELIMITED_SETTINGS = {
  delimiter: ";",
  quote: '"',
  header_row: true,
  skip_lines: 0,
  charset: "utf8",
  line_ending: "lf",
  write_bom: false,
  columns: [
    { name: "sample_id", header: "Sample", required: true },
    { name: "result", type: "decimal" },
  ],
  record_mode: "one_per_line",
  group_by: null,
  max_file_bytes: 1_048_576,
  file_name: "{sample_id}-{seq}.csv",
  on_exists: "suffix",
};

/** An AU instrument's online parameter sheet (docs/spec/configuration.md 7.2). */
const AU_SETTINGS = {
  profile: "au640",
  framing: {
    start: [2],
    end: [3],
    bcc: true,
    use_etb: true,
    max_wire_len: 512,
    class: "b",
  },
  format: {
    unit_no: true,
    rack_digits: 4,
    sample_id_digits: 10,
    sex: true,
    age: false,
    month: true,
    patient_info_digits: [10, 0, 0, 0, 0, 0],
    result_digits: 6,
  },
  frame_timeout_ms: 3000,
  block_timeout_ms: 5000,
};

const RETRY = { initial_delay_ms: 1500, max_delay_ms: 120_000, multiplier: 2, jitter_percent: 20 };

/** Every endpoint kind, both channel kinds, fields the console does not model set to non-defaults. */
export function richConfig(): WireConfig {
  return {
    schema_version: 1,
    config_version: 7,
    generated_at: "2026-11-06T12:00:00.000Z",
    diagnostics: { snapshot_interval_seconds: 5 },
    endpoints: [
      {
        endpoint_id: IDS.serial,
        label: "Analyzer (COM3)",
        kind: "serial",
        tags: { site: "north" },
        settings: {
          port_name: "COM3",
          baud_rate: 19200,
          data_bits: 7,
          parity: "even",
          stop_bits: "two",
          flow_control: "rts_cts",
          read_timeout_ms: 500,
          write_timeout_ms: 4000,
          virtual_link: null,
        },
      },
      {
        endpoint_id: IDS.listener,
        label: "Chemistry listener",
        kind: "tcp_listener",
        settings: {
          bind_address: "0.0.0.0",
          port: 5200,
          accept_backlog: 32,
          allowed_peer_cidrs: ["192.168.10.0/24", "10.0.0.5"],
          allow_any_peer: false,
          idle_timeout_seconds: 120,
          keep_alive: false,
          keep_alive_time_sec: 45,
          keep_alive_interval_sec: 15,
          keep_alive_retry_count: 5,
        },
      },
      {
        endpoint_id: IDS.auListener,
        label: "AU listener",
        kind: "tcp_listener",
        settings: { bind_address: "127.0.0.1", port: 5300 },
      },
      {
        endpoint_id: IDS.client,
        label: "LIS HL7",
        kind: "tcp_client",
        settings: {
          host: "lis.example",
          port: 6661,
          connect_timeout_ms: 7000,
          idle_timeout_seconds: 30,
          keep_alive: true,
          keep_alive_time_sec: 30,
          keep_alive_interval_sec: 10,
          keep_alive_retry_count: 3,
        },
      },
      {
        endpoint_id: IDS.folder,
        label: "Result files",
        enabled: false,
        kind: "folder",
        settings: {
          path: "/data/in",
          mode: "poll",
          poll_interval_seconds: 15,
          file_pattern: "*.csv",
          recursive: true,
          ignore_suffixes: [".tmp", ".part"],
          stable_seconds: 3,
          exclusive_open_check: false,
          on_consumed: "leave",
          processed_directory: "done",
          write_mode: "direct",
        },
      },
      {
        endpoint_id: IDS.sftp,
        label: "Partner SFTP",
        kind: "sftp",
        settings: {
          host: "sftp.example",
          port: 2222,
          username: "lab",
          auth: {
            method: "key",
            private_key: { secret: "lab-key" },
            passphrase: { secret: "lab-key-pass" },
          },
          host_key_fingerprint: "SHA256:abcdefghijklmnop",
          remote_directory: "/out",
          connect_timeout_ms: 12_000,
          operation_timeout_ms: 40_000,
          poll_interval_seconds: 45,
          file_pattern: "*.hl7",
          recursive: false,
          ignore_suffixes: [".tmp"],
          stable_seconds: 5,
          on_consumed: "delete",
          processed_directory: "processed",
          write_mode: "atomic_rename",
        },
      },
      {
        endpoint_id: IDS.folderOut,
        label: "Export folder",
        kind: "folder",
        settings: { path: "/data/out" },
      },
    ],
    channels: [
      {
        channel_id: IDS.bridge,
        label: "Analyzer to LIS",
        kind: "bridge",
        tags: { owner: "lab" },
        endpoint_a: IDS.serial,
        endpoint_b: IDS.client,
        connection: {
          connect_strategy: "receiver_then_sender",
          receiver: "a",
          pairing_timeout_ms: 7000,
        },
        buffer: {
          max_buffered_segments: 500,
          max_bytes: 2_097_152,
          overflow_action: "block_then_degrade",
          block_timeout_ms: 3000,
        },
        retry: {
          initial_delay_ms: 2000,
          max_delay_ms: 30_000,
          multiplier: 3,
          jitter_percent: 10,
          max_consecutive_failures_before_alert: 4,
        },
        tap: { framer: "hl7_mllp", record_messages: true, max_message_bytes: 65_536 },
      },
      {
        channel_id: IDS.message,
        label: "Chemistry results",
        kind: "message",
        source: {
          endpoint: IDS.listener,
          protocol: "astm",
          settings: {
            charset: "utf8",
            role: "instrument",
            ack_timeout_ms: 9000,
            receive_timeout_ms: 31_000,
            max_frame_retries: 4,
            max_frame_chars: 200,
            contention_wait_ms: 15_000,
            max_message_bytes: 2_097_152,
          },
          duplicate_detection: false,
        },
        filters: [
          { path: "PID.8", op: "equals", value: "F" },
          { path: "PV1.2", op: "in", value: ["I", "E"] },
          { path: "OBR.4", op: "exists" },
          {
            any: [
              { path: "MSH.9", op: "equals", value: "ORU^R01" },
              { path: "MSH.9", op: "equals", value: "ORU^R03" },
            ],
          },
          { path: "PID.5", op: "starts_with", value: "doe", ignore_case: true },
        ],
        destinations: [
          {
            destination_id: IDS.hl7,
            label: "LIS (HL7)",
            endpoint: IDS.client,
            protocol: "hl7_mllp",
            settings: {
              ack_mode: "enhanced",
              send_application_ack: false,
              version: "2.5.1",
              sending_application: "PACKIE",
              sending_facility: "LAB",
              receiving_application: "LIS",
              receiving_facility: "HOSPITAL",
              processing_id: "T",
              charset: "latin1",
              max_message_bytes: 2_097_152,
              frame_timeout_ms: 20_000,
            },
            filters: [{ path: "OBX.3", op: "not_in", value: ["X", "Y"] }],
            mapping: MAPPING,
            delivery: { max_attempts: 8, retry: RETRY, ack_required: "received" },
          },
          {
            destination_id: IDS.csv,
            label: "Export CSV",
            enabled: false,
            endpoint: IDS.folderOut,
            protocol: "delimited_file",
            settings: DELIMITED_SETTINGS,
            mapping: {
              rules: [
                { op: "copy", from: "PID.3", to: "col.sample_id" },
                { op: "copy", from: "OBX.5", to: "col.result" },
              ],
            },
          },
          {
            destination_id: IDS.passthrough,
            label: "Archive (as received)",
            endpoint: IDS.client,
            protocol: "astm",
            mapping: null,
          },
        ],
        retry: { initial_delay_ms: 500 },
        delivery: {
          max_attempts: 7,
          retry: { initial_delay_ms: 2500, max_delay_ms: 90_000 },
          ack_timeout_seconds: 45,
        },
        retention: { max_age_days: 14, max_messages: 100_000 },
      },
      {
        channel_id: IDS.au,
        label: "AU results",
        kind: "message",
        enabled: false,
        tags: { instrument: "AU640" },
        source: { endpoint: IDS.auListener, protocol: "au", settings: AU_SETTINGS },
        destinations: [
          {
            destination_id: "d-lis",
            label: "LIS",
            endpoint: IDS.client,
            protocol: "au",
            settings: AU_SETTINGS,
          },
        ],
        retention: { max_age_days: null },
      },
    ],
    alerts: {
      endpoint_down_seconds: 90,
      endpoint_down_critical_seconds: 1200,
      no_peer_seconds: 30,
      buffer_high_percent: 70,
      delivery_backlog_count: 500,
      disk_low_megabytes: 2048,
    },
  };
}
