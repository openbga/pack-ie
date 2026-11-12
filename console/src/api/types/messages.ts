// The message store: messages, deliveries, bodies and resends.
import type {
  AckLevel,
  AttemptResult,
  BodyFormat,
  ClientExportKind,
  DeliveryState,
  DeliverySummary,
  MessageDirection,
  MessageOrigin,
  MessageState,
  Protocol,
  RejectionCode,
  ResendMode,
} from "./core";

// The store's model as docs/spec/message-channels.md §3 and §4 describe it. Identifiers are
// nullable because a protocol may not provide one.

/** One destination's delivery as a grid row shows it. */
export interface DeliveryChip {
  deliveryId: string;
  destinationId: string;
  destinationLabel: string;
  state: DeliveryState;
  ackLevel: AckLevel;
}

export interface MessageSummary {
  id: string;
  channelId: string;
  channelLabel: string;
  /** Receipt sequence within the channel. */
  seq: number;
  direction: MessageDirection;
  state: MessageState;
  origin: MessageOrigin;
  receivedAt: string;
  protocol: Protocol;
  /** `ORU^R01`, `astm`, `au`, `delimited`; null when unknown. */
  messageType: string | null;
  controlId: string | null;
  sampleId: string | null;
  patientId: string | null;
  sizeBytes: number;
  /** Address, port or server that sent it. */
  sourcePeer: string | null;
  /** Configuration version in force at receipt. */
  configVersion: number;
  /** The message this one was resent from. */
  resentFrom: string | null;
  /** Roll-up of the deliveries. */
  deliverySummary: DeliverySummary;
  /** One chip per delivery, in destination order. */
  deliveries: DeliveryChip[];
}

/** Columns the engine sorts on; ties break by receipt sequence. */
export type MessageSortField = "receivedAt" | "channel" | "state" | "messageType" | "sizeBytes";

/** Control, sample or patient id, equal to `value` or starting with it. */
interface IdentifierFilter {
  value: string;
  prefix: boolean;
}

/** An absent or empty criterion matches everything; list criteria match any of their values. */
export interface MessageFilter {
  /** Received at or after. */
  since?: string;
  /** Received before. */
  until?: string;
  channelIds?: string[];
  directions?: MessageDirection[];
  states?: MessageState[];
  /** Messages with at least one delivery in any of these states. */
  deliveryStates?: DeliveryState[];
  deliverySummaries?: DeliverySummary[];
  identifier?: IdentifierFilter;
  /** Substring of identifiers, message type or stored bodies; 3 characters or more. */
  text?: string;
  /** Start of a message ID: 8 to 32 hex digits, dashes optional, any case. */
  messageIdPrefix?: string;
}

export interface MessageQuery {
  filter: MessageFilter;
  sort: { field: MessageSortField; descending: boolean };
  offset: number;
  limit: number;
}

export interface MessagePage {
  rows: MessageSummary[];
  /** Messages matching the filter, all pages. */
  total: number;
  offset: number;
  limit: number;
}

/** One delivery attempt; `result` is null while it is in flight. */
export interface DeliveryAttempt {
  /** From 1 across all attempt cycles of the delivery. */
  attemptNo: number;
  startedAt: string;
  endedAt: string | null;
  result: AttemptResult | null;
  /** The protocol acknowledgement code received (`AA`, `CA`, `AR`). */
  ackCode: string | null;
  /** The acknowledgement message's own control id (HL7 MSH-10 of the ACK); null when none arrived or the protocol has none. */
  ackControlId: string | null;
  /** The failure's code (`connection_refused`, `write_timeout`). */
  errorCode: string | null;
  ackLevel: AckLevel;
}

/** One message to one destination. */
export interface Delivery {
  id: string;
  messageId: string;
  channelId: string;
  destinationId: string;
  destinationLabel: string;
  state: DeliveryState;
  /** What was proven; never read a level below `accepted` as processing success. */
  ackLevel: AckLevel;
  /** Attempts in the current cycle. */
  attempts: number;
  /** Attempts in all cycles. */
  totalAttempts: number;
  createdAt: string;
  updatedAt: string;
  /** While `queued` for a retry. */
  nextAttemptAt: string | null;
  /** Why it is in this state (`transform_error`, `engine_restarted`). */
  reasonCode: string | null;
  /** The operator's reason, when closed. */
  closedReason: string | null;
  /** The last failure in words (the attempt's error, or the failing mapping rule and path); never message content. */
  lastError: string | null;
  /** The operator's reason for the resend that started the current attempt cycle. */
  resendReason: string | null;
  /** The configuration version whose mapping produced the transformed body. */
  mappingVersion: number | null;
  transformedHash: string | null;
  /** Oldest first. */
  attemptLog: DeliveryAttempt[];
}

/** Where in a file a message came from. */
export interface SourceFile {
  name: string;
  fileHash: string;
  firstRow: number;
  lastRow: number;
}

/** One field of a message by the path filters and mappings use: `MSH.9.1`, `R.3.4`. */
export interface MessageField {
  path: string;
  value: string;
}

/**
 * The original's fields as the engine read them (`getMessageFields`). They are message content:
 * fetched only by an explicit, audited "Show content", held in memory like a body.
 */
export interface MessageFields {
  /** In message order; empty when `unavailable`. */
  fields: MessageField[];
  /** More fields exist than are listed. */
  truncated: boolean;
  /** Why no fields are listed, in the engine's words; never message content. */
  unavailable: string | null;
}

/** Everything about a message except its bodies. */
export interface MessageDetail {
  summary: MessageSummary;
  committedAt: string | null;
  contentHash: string;
  /** What duplicate detection compares, when the message has one. */
  sourceIdentity: string | null;
  sourceEndpointId: string;
  /** The endpoint's label when the active configuration still names it. */
  sourceEndpointLabel: string;
  sourceFile: SourceFile | null;
  /** The acknowledgement sent back to the sender (`AA`, `CA`, `ACK`). */
  responseCode: string | null;
  respondedAt: string | null;
  abortReason: string | null;
  /** Why it was filtered or is invalid (`source_filter`, `parse_error`). */
  reasonCode: string | null;
  /** The earlier message this one duplicates. */
  duplicateOf: string | null;
  /** The earlier message with the same identity and other content. */
  revisionOf: string | null;
  /** The newest later message that revises this one: this message is superseded by it. */
  revisedBy: string | null;
  /** In destination order. */
  deliveries: Delivery[];
}

export type BodySelector = { kind: "original" } | { kind: "transformed"; deliveryId: string };

export interface BodyRequest {
  messageId: string;
  which: BodySelector;
  /** `text` shows control characters by name, one segment or record per line. */
  as: BodyFormat;
}

/** A body as the engine rendered it, for the local client that asked only. */
export interface MessageBody {
  messageId: string;
  which: BodySelector;
  as: BodyFormat;
  content: string;
  /** Size of the stored bytes. */
  byteLength: number;
  /** SHA-256 of the stored bytes, lowercase hex. */
  hash: string;
  /** The rendering stops early because the body exceeds the display limit. */
  truncated: boolean;
}

export interface ResendRequest {
  messageId: string;
  /** `all` or destination ids. */
  destinations: "all" | string[];
  mode: ResendMode;
  /** Needed to resend a delivery that may already have been processed. */
  confirmDuplicateRisk: boolean;
  /** Why, 1 to 500 characters; recorded with the new attempt cycle. Left out when empty. */
  reason?: string;
  /** Needed to resend a message that a newer revision supersedes; the engine audits it. */
  confirmSuperseded?: boolean;
}

/** A precondition that failed: a stable code and what is wrong. */
export interface Rejection {
  code: RejectionCode;
  message: string;
}

/** What a resend did for one destination. */
export interface ResendOutcome {
  destinationId: string;
  destinationLabel: string;
  /** The queued delivery, when one was queued. */
  deliveryId: string | null;
  /** Why none was queued. */
  rejected: Rejection | null;
}

export interface ResendResult {
  /** The new message, for `new_message`. */
  newMessageId: string | null;
  /** One entry per requested destination. */
  destinations: ResendOutcome[];
}

// Reports over the store (docs/spec/message-channels.md §6, compliance P7 and P9).

export type MessageExportFormat = "csv" | "jsonl";

export interface MessageExportRequest {
  /** The Messages page's current filter. */
  filter: MessageFilter;
  format: MessageExportFormat;
  /** Also writes each original body: message content, audited as a content view. */
  includeBodies: boolean;
}

/** Where an export went; `exportMessages` resolves with null when the operator cancelled. */
export interface MessageExportResult {
  /** The file the operator chose, or the browser's download name in the demo. */
  savedTo: string;
  /** Messages written. */
  rows: number;
}

/** One channel and period: what it received and what each destination confirmed. */
export interface TransmissionQuery {
  channelId: string;
  /** Received at or after (RFC 3339). */
  since: string;
  /** Received before; the engine's default is now. */
  until?: string;
}

/** What one destination was sent and what it confirmed. */
export interface DestinationTransmission {
  destinationId: string;
  label: string;
  deliveries: number;
  /** Acknowledged at `accepted`. */
  accepted: number;
  /** Acknowledged at `received`. */
  received: number;
  /** Acknowledged at `handed_off`: written, not confirmed by the destination. */
  handedOff: number;
  /** Rejected, failed, timed out or outcome unknown. */
  unresolved: number;
  /** Closed by an operator. */
  closed: number;
  /** Still queued or sending. */
  pending: number;
}

/** One delivery of the report. */
export interface TransmissionRow {
  messageId: string;
  receivedAt: string;
  messageState: MessageState;
  /** The sender's control id. */
  controlId: string | null;
  /** The endpoint the message was received on. */
  sourceEndpointId: string;
  /** The connected peer it came from (address or device), as recorded at receipt. */
  sourcePeer: string | null;
  /** The configuration version active when the message was received. */
  configVersion: number;
  /** The configuration version whose mapping produced the bytes sent to this destination. */
  mappingVersion: number | null;
  /** SHA-256 of the original bytes, lowercase hex. */
  contentHash: string;
  destinationId: string;
  state: DeliveryState;
  ackLevel: AckLevel;
  /** The last attempt's acknowledgement code (`AA`, `CA`). */
  ackCode: string | null;
  /** The acknowledgement message's own control id (HL7 MSH-10 of the ACK). */
  ackControlId: string | null;
  /** SHA-256 of the bytes sent; null for a pass-through or unsent delivery. */
  sentHash: string | null;
  attempts: number;
  firstAttemptAt: string | null;
  acknowledgedAt: string | null;
}

export interface TransmissionReport {
  channelId: string;
  since: string;
  until: string;
  generatedAt: string;
  /** Messages received in the period. */
  messages: number;
  destinations: DestinationTransmission[];
  /** One per delivery, oldest message first. */
  rows: TransmissionRow[];
  /** More deliveries than the engine lists (10,000): ask for a shorter period. */
  truncated: boolean;
  /** The most rows one report lists. */
  rowsLimit: number;
  /** The deliveries of the whole period, which a complete report would list. */
  periodRowsTotal: number;
}

/** What the engine is told before the console writes an export it built itself. */
interface ReportAudit {
  kind: ClientExportKind;
  /** Versions, for a change report. */
  rows: number;
}

/** A report the console writes itself (change report, transmission CSV), saved through the shell. */
export interface ReportFile {
  /** The suggested file name, with its extension. */
  name: string;
  format: "csv" | "json";
  content: string;
  /**
   * Set for an export the engine does not otherwise see (the change report): it is recorded in
   * the audit log first, and when it cannot be recorded nothing is written.
   */
  audit?: ReportAudit;
}

/** Where a report went; `saveReportFile` resolves with null when the operator cancelled. */
export interface SavedReport {
  /** The file the operator chose, or the browser's download name in the demo. */
  savedTo: string;
}
