// The configuration document (architecture §5, runtime.json).
import type { PairingStrategy, Protocol } from "./core";

//
// The editable shape of the engine's `RuntimeConfig`. Fields the console does not edit
// (timeouts, keep-alive tuning, …) are kept by `src/api/ipc/` and sent back unchanged.

interface EndpointConfigBase {
  id: string;
  label: string;
  enabled: boolean;
}

export interface SerialEndpointConfig extends EndpointConfigBase {
  kind: "serial";
  port: string;
  baudRate: number;
  dataBits: 5 | 6 | 7 | 8;
  parity: "none" | "even" | "odd";
  stopBits: 1 | 2;
  flowControl: "none" | "rts_cts" | "xon_xoff";
}

export interface TcpClientEndpointConfig extends EndpointConfigBase {
  kind: "tcp_client";
  host: string;
  port: number;
  connectTimeoutMs: number;
  /** 0 disables the idle timeout. */
  idleTimeoutSeconds: number;
  keepalive: boolean;
}

export interface TcpListenerEndpointConfig extends EndpointConfigBase {
  kind: "tcp_listener";
  bindAddress: string;
  port: number;
  /** Peer allowlist: IP addresses or CIDR ranges. */
  allowlist: string[];
  /** Required when the listener is not on loopback and the allowlist is empty. */
  allowAnyPeer: boolean;
  /** 0 disables the idle timeout. */
  idleTimeoutSeconds: number;
  keepalive: boolean;
}

export interface FolderEndpointConfig extends EndpointConfigBase {
  kind: "folder";
  path: string;
  mode: "watch" | "poll";
  pollIntervalSec: number;
  /** One glob matched against file names; `*` and `?` only, such as `*.csv`. */
  filePattern: string;
}

export interface SftpEndpointConfig extends EndpointConfigBase {
  kind: "sftp";
  host: string;
  port: number;
  username: string;
  authMethod: "password" | "private_key";
  /** Name of the secret holding the password, or the private key; never the secret itself. */
  secretName: string;
  /** Private key only: name of the secret holding its passphrase; empty for none. */
  passphraseSecretName: string;
  /** `SHA256:` followed by base64; the server key is pinned. */
  hostKeyFingerprint: string;
  remotePath: string;
  pollIntervalSec: number;
  /** One glob matched against file names; `*` and `?` only, such as `*.csv`. */
  filePattern: string;
}

export type EndpointConfig =
  | SerialEndpointConfig
  | TcpClientEndpointConfig
  | TcpListenerEndpointConfig
  | FolderEndpointConfig
  | SftpEndpointConfig;

export type TapFramer = "none" | "astm" | "au" | "hl7_mllp";

export interface RetryPolicy {
  initialMs: number;
  maxMs: number;
  /** Null retries forever (bridges); destinations must set it. */
  maxAttempts: number | null;
}

export interface RetentionPolicy {
  /** Keep messages this many days; null for no age limit. */
  days: number | null;
  /** Keep at most this many messages; null for no count limit. */
  maxMessages: number | null;
}

export interface BridgeChannelConfig {
  kind: "bridge";
  id: string;
  label: string;
  enabled: boolean;
  endpointAId: string;
  endpointBId: string;
  pairing: PairingStrategy;
  /** Required for `receiver_then_sender`, absent otherwise. */
  pairingReceiver: "a" | "b" | null;
  bufferBytes: number;
  retry: RetryPolicy;
  tapFramer: TapFramer;
  /** How long the tap keeps the messages it records; either limit null for no limit. */
  tapRetention: RetentionPolicy;
}

/** HL7 acknowledgement modes; `none` (no acknowledgement expected) is for destinations only. */
export type AckMode = "original" | "enhanced" | "none";

/**
 * The online parameter sheet of an AU400/AU640 (docs/spec/configuration.md §7.2). There are no
 * defaults: the laboratory supplies the instrument's own settings.
 */
export interface AuSettings {
  profile: "au400" | "au640";
  /** One or two control bytes (1 to 31), never ACK (6) or NAK (21). */
  framingStart: number[];
  framingEnd: number[];
  /** One XOR check byte per frame. */
  bcc: boolean;
  /** ETB for intermediate blocks; only when the end is `[3]`. */
  useEtb: boolean;
  maxWireLen: 256 | 512 | 1024;
  /** `a`: no ACK/NAK on receive; `b`: ACK after durable commit. */
  framingClass: "a" | "b";
  unitNo: boolean;
  rackDigits: 0 | 4 | 5;
  sampleIdDigits: number;
  sex: boolean;
  age: boolean;
  month: boolean;
  /** Six digit counts, each 0 to 20. */
  patientInfoDigits: number[];
  resultDigits: 6 | 9;
  /** 100 to 9900 in steps of 100. */
  frameTimeoutMs: number;
  /** Wait for the next block of an unfinished sample; absent is the engine's default, 9900. */
  blockTimeoutMs?: number;
  /** Width of a test code in result items and requests; absent is the engine's default, 2. */
  testCodeDigits?: 2 | 3;
}

export interface ConnectorSettings {
  /** HL7 only. */
  ackMode?: AckMode;
  /** HL7 only: the version of generated messages (MSH-12), such as `2.5.1`. */
  hl7Version?: string;
  /** HL7 only: a frame must end with a carriage return after the end block (default true). */
  requireEndCr?: boolean;
  /** HL7 destination: the message codes (MSH-9.1) accepted as the answer; default `ACK`. */
  ackMessageTypes?: string[];
  /** HL7 source: answer order messages with an order response instead of an `ACK`. */
  replyType?: "ack" | "order_response";
  /** ASTM only: the side PACK-IE plays in link contention. */
  astmRole?: "computer" | "instrument";
  /** ASTM only: text characters per frame, 64 to 64000 (default 240). */
  maxFrameChars?: number;
  /**
   * ASTM source: where the order record carries the sample id that Messages search by,
   * `O.<field>[.<component>]` (field 3 to 99, component 1 to 99); default `O.3`.
   */
  sampleIdPath?: string;
  /** ASTM destination: ENQ transmissions per message, 1 to 10 (default 3). */
  enqAttempts?: number;
  /** ASTM destination: wait in ms after a NAK or silence to ENQ before the next ENQ (default 10000). */
  busyRetryMs?: number;
  /** AU only. */
  au?: AuSettings;
  /** Delimited files only: the column template as JSON text (see docs/spec/configuration.md §7.4). */
  templateJson?: string;
}

export interface ConnectorConfig {
  id: string;
  endpointId: string;
  protocol: Protocol;
  settings: ConnectorSettings;
}

/** The connector messages come from. */
export interface SourceConfig extends ConnectorConfig {
  /** Record repeats by source identity and content instead of processing them twice (MSG-06). */
  duplicateDetection: boolean;
}

/**
 * One destination: its own filters, mapping and code tables (message-channels.md §6). With no
 * mappings the original message passes through unchanged and the destination's protocol must
 * be the source's.
 */
export interface DestinationConfig extends ConnectorConfig {
  label: string;
  /** A disabled destination gets no new deliveries; its queued ones wait. */
  enabled: boolean;
  /** All must pass for a delivery to be created for this destination. */
  filters: FilterRule[];
  mappings: MappingRule[];
  codeTables: CodeTables;
  /**
   * The engine's mapping holds something this console cannot show (an operation or field it does
   * not know), so `mappings` and `codeTables` are empty here. The mapping stays as it is.
   */
  mappingHidden?: true;
  /**
   * The mapping refuses a message that holds more than one patient, order or specimen group
   * (`require_single_group`); written only when true.
   */
  requireSingleGroup?: true;
  /** Source preconditions (`require`); a message that fails one fails this destination's mapping. */
  require?: RequireCondition[];
  retry: RetryPolicy;
}

export type FilterOperator =
  | "equals"
  | "not_equals"
  | "in"
  | "not_in"
  | "starts_with"
  | "contains"
  | "matches"
  | "exists"
  | "not_exists"
  | "gt"
  | "gte"
  | "lt"
  | "lte";

/** One comparison of a field. */
export interface Comparison {
  /** Path into the parsed message: `PID.3[0].1`, `R.3`, `col.sample_id`. */
  path: string;
  operator: FilterOperator;
  /** `in` and `not_in`: comma-separated values. Ignored by `exists` and `not_exists`. */
  value: string;
  ignoreCase: boolean;
}

/**
 * A condition as docs/spec/configuration.md §8.1 has it: one comparison, or a combination of
 * conditions (`all` of them, `any` of them, or `not` one).
 */
export type Condition =
  | Comparison
  | { all: Condition[] }
  | { any: Condition[] }
  | { not: Condition };

/** One filter; a message passes the channel only when every filter holds. */
export type FilterRule = {
  /** Editor-only key; never sent to the engine. */
  id: string;
} & Condition;

/** A condition inside a `when` rule. */
export type MappingCondition = Condition;

export type LookupOnMissing = "error" | "keep" | "default" | "skip";

/**
 * One text step applied to the text read from a path, before the rule uses it
 * (docs/spec/configuration.md §8.2, text steps). Steps run in order.
 */
export type Step =
  | { step: "trim" }
  | { step: "upper" }
  | { step: "lower" }
  | { step: "strip_leading"; chars: string }
  /** Part number `take` of the text split on `separator`; 0 is the first, -1 the last. */
  | { step: "split"; separator: string; take: number }
  /** At most `length` characters from `start` (0-based); null for the rest of the text. */
  | { step: "slice"; start: number; length: number | null }
  | { step: "replace"; find: string; with: string };

export type StepKind = Step["step"];

/** The operators of a `calculate` formula; `abs` takes one formula, the others two to sixteen. */
export type FormulaOp = "add" | "sub" | "mul" | "div" | "min" | "max" | "abs";

/**
 * A `calculate` formula as the engine reads it (docs/spec/configuration.md §8.2): a constant
 * decimal (kept as the text typed), the decimal at a path, the decimal at a path of the first
 * record that matches a condition, or an operator over formulas.
 */
export type Formula =
  | { constant: string }
  | { from: string; steps: Step[] }
  | { find: string; where: Condition; from: string; steps: Step[] }
  | { op: FormulaOp; args: Formula[] };

export type CalculateRounding = "half_up" | "half_even" | "truncate";

/**
 * What a `calculate` does with an operand that is not a number, a zero divisor and the like:
 * fail the message, write nothing, write the operand's text unchanged or write the default.
 */
export type CalculateOnInvalid = "error" | "skip" | "keep" | "default";

/** One piece of a `concat`: the text at a path (after its steps), or literal text. */
export type ConcatPart = { from: string; steps?: Step[] } | { text: string };

/**
 * One rule of an ordered mapping, as in docs/spec/configuration.md §8.2. `id` is an
 * editor-only key. `when` and `for_each` hold rules of their own; inside `for_each`, paths
 * starting `$.` address the current input occurrence (`from`); in `to` they address the new record, the same record (`update`) or the next repetition (`repeat`).
 */
export type MappingRule = { id: string } & (
  | {
      op: "copy";
      from: string;
      to: string;
      default: string;
      required: boolean;
      steps?: Step[];
    }
  | { op: "constant"; value: string; to: string }
  | {
      op: "lookup";
      from: string;
      table: string;
      to: string;
      onMissing: LookupOnMissing;
      default: string;
      /** Match the code without regard to letter case. */
      ignoreCase: boolean;
      steps?: Step[];
    }
  | { op: "concat"; parts: ConcatPart[]; to: string }
  | {
      op: "datetime";
      from: string;
      to: string;
      input: string;
      output: string;
      fromTimezone: string;
      toTimezone: string;
      steps?: Step[];
    }
  | {
      op: "calculate";
      formula: Formula;
      to: string;
      /** Decimal places of the result, 0 to 12; null writes the exact result. */
      scale: number | null;
      rounding: CalculateRounding;
      /** `carry` (the engine's default) calculates with the number of `<5` and keeps the `<`. */
      comparator: "reject" | "carry";
      /** `sn` reads and writes the comparator and the number of an HL7 structured numeric. */
      notation: "text" | "sn";
      onInvalid: CalculateOnInvalid;
      default: string;
      required: boolean;
    }
  | { op: "when"; condition: MappingCondition; then: MappingRule[]; else: MappingRule[] }
  | {
      op: "for_each";
      over: string;
      into: string;
      rules: MappingRule[];
      /**
       * `update` rewrites each existing occurrence in place; `repeat` writes each as one repetition
       * of the field `into` names (`O.5`); absent creates a new record for each.
       */
      mode?: "update" | "repeat";
    }
  /** Adds one record that is built from `rules`, only when every rule writes a value. */
  | { op: "append"; into: string; rules: MappingRule[] }
  | { op: "sequence"; to: string; start: number }
);

export type MappingOp = MappingRule["op"];

/**
 * A precondition on the source message, checked before any rule runs: the number of
 * occurrences of a record is within limits, or every occurrence of a path holds the same text.
 */
export type RequireCondition =
  | { count: string; min: number | null; max: number | null }
  | { same: string };

/** Named code tables for `lookup` rules: table name → code → text. */
export type CodeTables = Record<string, Record<string, string>>;

export interface MessageChannelConfig {
  kind: "message";
  id: string;
  label: string;
  enabled: boolean;
  source: SourceConfig;
  /** Source filters: all must pass, otherwise the message is `filtered`. */
  filters: FilterRule[];
  destinations: DestinationConfig[];
  retention: RetentionPolicy;
}

export type ChannelConfig = BridgeChannelConfig | MessageChannelConfig;

/** `alerts` of runtime.json (docs/spec/diagnostics.md alert catalog). */
export interface AlertThresholds {
  endpointDownSeconds: number;
  queueDepthWarning: number;
  bufferPressurePercent: number;
  diskFreeMinMib: number;
}

/** The versioned `runtime.json` document. */
export interface ConfigDocument {
  version: number;
  endpoints: EndpointConfig[];
  channels: ChannelConfig[];
  alertThresholds: AlertThresholds;
}

export interface ConfigIssue {
  severity: "error" | "warning";
  /** Engine path of the offending value. */
  path: string;
  message: string;
  /** The engine's stable problem code (`port_conflict`, `retry_invalid`, ...), when it gave one. */
  code?: string;
}

/** What applying a document does to one channel or endpoint, as the engine plans it. */
export type PlanChange = "added" | "changed" | "updated" | "removed" | "unchanged" | "stopped";

export interface PlanItem {
  id: string;
  label: string;
  change: PlanChange;
}

/**
 * What applying a valid document would do: `added` starts, `changed` restarts, `updated`
 * takes the change in place, `removed` stops and removes, `stopped` is a channel the new
 * document disables (stopped, not started), `unchanged` is untouched.
 */
export interface ApplyPlan {
  channels: PlanItem[];
  endpoints: PlanItem[];
  alertsChanged: boolean;
}

export interface ValidationResult {
  /** True when there are no `error` issues; warnings do not block apply. */
  ok: boolean;
  issues: ConfigIssue[];
  /** What applying would change; empty (no channels or endpoints) when the document has errors. */
  plan: ApplyPlan;
}

/**
 * What applying a version did to one channel (configuration.md §10 Result). `reason` is the
 * engine's cause for `failed` and absent otherwise.
 */
export interface ChannelOutcome {
  channelId: string;
  outcome: ChannelOutcomeKind;
  reason?: string;
}

export type ChannelOutcomeKind =
  | "unchanged"
  | "updated"
  | "started"
  | "restarted"
  | "stopped"
  | "removed"
  | "failed";

export interface ApplyResult {
  version: number;
  /** The engine's outcome for every channel, in its order. */
  outcomes: ChannelOutcome[];
  /** Channels that were restarted because their configuration changed. */
  restartedChannelIds: string[];
  unchangedChannelCount: number;
  /**
   * Channels whose change could not be applied. They keep their previous definition and the
   * next apply retries them; the rest of the document is in effect.
   */
  failures: { channelId: string; reason: string }[];
}

/** A sign-off that a version works as intended after its change (change control). */
export interface ConfigVerification {
  /** The OS account that signed it off. */
  by: string;
  at: string;
  /** The lab's reference, such as a validation record number. */
  note: string;
}

export interface ConfigVersionSummary {
  version: number;
  /** When the version was saved. */
  appliedAt: string;
  /** The OS account that applied it; null for versions saved before the record was kept. */
  appliedBy: string | null;
  /** Why it was applied (`rollback of 12`, `imported from the runtime file`); null when none. */
  note: string | null;
  endpoints: number;
  channels: number;
  /** Oldest first; empty means "not verified since change". */
  verifications: ConfigVerification[];
  /** What applying it did to each channel; empty for versions saved before outcomes were kept and for the startup import. */
  outcomes: ChannelOutcome[];
  current: boolean;
}
