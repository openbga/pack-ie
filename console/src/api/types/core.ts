// Errors, the engine's shared vocabulary (the enums re-exported from the generated types), settings and connection status.

import type {
  AckLevel,
  AlertKind,
  AttemptResult,
  AuditAction,
  AuditExportFormat,
  AuditObjectType,
  AuditOrigin,
  AuditOutcome,
  AuditProblemKind,
  BodyFormat,
  ChannelKind,
  ChannelState,
  ClientExportKind,
  ConnectStrategy,
  DeliverySummary,
  EndpointKind,
  EndpointState,
  LogLevel,
  MessageDirection,
  MessageOrigin,
  MessageState,
  RejectionCode,
  ResendMode,
} from "@/ipc/generated";
import type { NotificationSettings } from "./setup";

export type {
  AckLevel,
  AlertKind,
  AttemptResult,
  AuditAction,
  AuditExportFormat,
  AuditObjectType,
  AuditOrigin,
  AuditOutcome,
  AuditProblemKind,
  BodyFormat,
  ChannelKind,
  ChannelState,
  ClientExportKind,
  DeliverySummary,
  EndpointKind,
  EndpointState,
  LogLevel,
  MessageDirection,
  MessageOrigin,
  MessageState,
  RejectionCode,
  ResendMode,
};

/** How a bridge pairs its two endpoints (the engine calls it the connect strategy). */
export type PairingStrategy = ConnectStrategy;

export type ErrorCode =
  | "validation"
  | "not_found"
  | "conflict"
  | "engine_unavailable"
  | "not_implemented"
  | "unavailable"
  | "internal";

export interface FieldError {
  /** Engine path, such as `channels[2].source.endpointId`. */
  field: string;
  message: string;
}

/** What every failing `EngineApi` call rejects with. */
export interface EngineError {
  code: ErrorCode;
  message: string;
  fieldErrors: FieldError[];
  /** The engine's stable reason when it refused a command (`superseded`, `reason_required`); absent otherwise. */
  rejection?: RejectionCode;
  /** Finds the matching log line; empty when the call never reached the engine. */
  correlationId: string;
}

/** Why a `stopped` channel is stopped. */
export type StopReason =
  | "paused"
  | "disabled"
  | "configuration"
  | "fatal"
  | "replaced"
  | "shutdown"
  | "store_unavailable"
  | "not_implemented";

/** Protocol tags of docs/spec/configuration.md §6.4. */
export type Protocol = "astm" | "au" | "hl7_mllp" | "delimited_file" | "raw";

export type DeliveryState =
  | "queued"
  | "sending"
  | "acknowledged"
  | "rejected"
  | "timed_out"
  | "outcome_unknown"
  | "failed"
  | "cancelled";

export type AlertSeverity = "critical" | "warning" | "info";

/** `bootstrap.json`: read at start; edits need a restart, so the console shows it read-only. */
export interface EngineSettings {
  nodeId: string;
  nodeName: string;
  /** The fields below are null when the engine cannot tell the console (no bootstrap file found). */
  stateDir: string | null;
  logDir: string | null;
  ipcPath: string | null;
  /**
   * The bootstrap file the engine was started with, which the command line needs as
   * `--bootstrap` to act on this engine's state; absent or null when it is not known.
   */
  bootstrapPath?: string | null;
  hosting: "service" | "console" | null;
  logRetentionDays: number | null;
  logRingSize: number | null;
  /** The bootstrap's `notifications` section; absent or null when it is not known or not configured. */
  notifications?: NotificationSettings | null;
}

export interface SecretInfo {
  name: string;
  /** Null when the engine does not say where the value lives. */
  storage: "os_keystore" | "restricted_file" | null;
  /** Whether a value is stored (null: unknown); the value itself is never readable. */
  hasValue: boolean | null;
  usedByEndpointIds: string[];
}

export interface EngineInfo {
  engineVersion: string;
  consoleVersion: string;
  protocolVersion: number;
  buildCommit: string | null;
  platform: string;
  license: string;
}

/** CON-01: whether an engine is reachable, and where the console looked for it. */
export interface ConnectionStatus {
  state: "connected" | "disconnected" | "reconnecting";
  /** Socket path or pipe name tried; null when none could be determined. */
  endpoint: string | null;
  /** Why the last attempt failed, in words for the operator. */
  detail: string | null;
  /** RFC 3339: when this state began. */
  since: string;
  /** The OS account the engine attributes this console's actions to; null before the greeting. */
  actor: string | null;
}

/** What the connected engine can do; pages hide controls the engine cannot serve. */
export interface EngineCapabilities {
  /** `demo`: messages shown are synthetic sample data, not the engine's store. */
  messages: "engine" | "demo";
  /** Whether secrets can be written through the engine. */
  secretsWritable: boolean;
  /** Whether alerts that have cleared are listed (not only this session's). */
  alertHistory: boolean;
}
