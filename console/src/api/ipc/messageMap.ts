/**
 * Stored messages: the engine's DTOs to the console's, and the console's grid query, body and
 * resend requests to the engine's. Pure. The vocabularies (states, directions, origins, ack
 * levels, attempt results) are the engine's own, so values pass through; what is renamed is
 * the field names (camelCase) and the sort columns.
 */

import type {
  AttemptRecord,
  MessageBody as RawBody,
  BodyRequest as RawBodyRequest,
  DeliveryChip as RawChip,
  DeliveryRecord as RawDelivery,
  ParsedFields as RawFields,
  MessageFilter as RawFilter,
  MessageQuery as RawQuery,
  MessageRecord as RawRecord,
  ResendRequest as RawResendRequest,
  ResendResult as RawResendResult,
  MessageSortField as RawSortField,
  MessageSummary as RawSummary,
} from "@/ipc/generated";
import type {
  BodyRequest,
  Delivery,
  DeliveryAttempt,
  DeliveryChip,
  MessageBody,
  MessageDetail,
  MessageFields,
  MessageFilter,
  MessageQuery,
  MessageSortField,
  MessageSummary,
  ResendRequest,
  ResendResult,
} from "../types";

// ── Summaries ──────────────────────────────────────────────────────────────────────────

function chipFromIpc(raw: RawChip): DeliveryChip {
  return {
    deliveryId: raw.delivery_id,
    destinationId: raw.destination_id,
    destinationLabel: raw.destination_label,
    state: raw.state,
    ackLevel: raw.ack_level,
  };
}

export function summaryFromIpc(raw: RawSummary): MessageSummary {
  return {
    id: raw.message_id,
    channelId: raw.channel_id,
    channelLabel: raw.channel_label,
    seq: raw.seq,
    direction: raw.direction,
    state: raw.state,
    origin: raw.origin,
    receivedAt: raw.received_at,
    protocol: raw.protocol,
    messageType: raw.message_type,
    controlId: raw.control_id,
    sampleId: raw.sample_id,
    patientId: raw.patient_id,
    sizeBytes: raw.byte_length,
    sourcePeer: raw.source_peer,
    configVersion: raw.config_version,
    resentFrom: raw.resent_from,
    deliverySummary: raw.delivery_summary,
    deliveries: raw.deliveries.map(chipFromIpc),
  };
}

// ── Query ──────────────────────────────────────────────────────────────────────────────

/** The engine's page limit; the Messages page offers smaller ones. */
const MAX_PAGE = 1000;

const SORT_FIELDS: Record<MessageSortField, RawSortField> = {
  receivedAt: "received_at",
  channel: "channel",
  state: "state",
  messageType: "message_type",
  sizeBytes: "byte_length",
};

export function filterToIpc(filter: MessageFilter): RawFilter {
  const identifier = filter.identifier?.value.trim() ?? "";
  const text = filter.text?.trim() ?? "";
  const messageIdPrefix = filter.messageIdPrefix?.trim() ?? "";
  return {
    since: filter.since ?? null,
    until: filter.until ?? null,
    channels: filter.channelIds ?? [],
    directions: filter.directions ?? [],
    states: filter.states ?? [],
    delivery_states: filter.deliveryStates ?? [],
    delivery_summaries: filter.deliverySummaries ?? [],
    identifier:
      filter.identifier === undefined || identifier === ""
        ? null
        : { value: identifier, prefix: filter.identifier.prefix },
    text: text === "" ? null : text,
    message_id_prefix: messageIdPrefix === "" ? null : messageIdPrefix,
  };
}

export function queryToIpc(query: MessageQuery): RawQuery {
  return {
    filter: filterToIpc(query.filter),
    sort: { field: SORT_FIELDS[query.sort.field], descending: query.sort.descending },
    offset: Math.max(0, query.offset),
    limit: Math.min(MAX_PAGE, Math.max(1, query.limit)),
  };
}

// ── Deliveries and the record ──────────────────────────────────────────────────────────

function attemptFromIpc(raw: AttemptRecord): DeliveryAttempt {
  return {
    attemptNo: raw.attempt_no,
    startedAt: raw.started_at,
    endedAt: raw.ended_at,
    result: raw.result,
    ackCode: raw.ack_code,
    ackControlId: raw.ack_control_id,
    errorCode: raw.error_code,
    ackLevel: raw.ack_level,
  };
}

export function deliveryFromIpc(raw: RawDelivery): Delivery {
  return {
    id: raw.delivery_id,
    messageId: raw.message_id,
    channelId: raw.channel_id,
    destinationId: raw.destination_id,
    destinationLabel: raw.destination_label,
    state: raw.state,
    ackLevel: raw.ack_level,
    attempts: raw.attempts,
    totalAttempts: raw.total_attempts,
    createdAt: raw.created_at,
    updatedAt: raw.updated_at,
    nextAttemptAt: raw.next_attempt_at,
    reasonCode: raw.reason_code,
    closedReason: raw.closed_reason,
    lastError: raw.last_error,
    resendReason: raw.resend_reason,
    mappingVersion: raw.mapping_version,
    transformedHash: raw.transformed_hash,
    attemptLog: raw.attempt_log.map(attemptFromIpc),
  };
}

/** `endpointLabel` is the source endpoint's label from the active document, when it names it. */
export function detailFromIpc(raw: RawRecord, endpointLabel: string | undefined): MessageDetail {
  return {
    summary: summaryFromIpc(raw.summary),
    committedAt: raw.committed_at,
    contentHash: raw.content_hash,
    sourceIdentity: raw.source_identity,
    sourceEndpointId: raw.source_endpoint_id,
    sourceEndpointLabel: endpointLabel ?? raw.source_endpoint_id,
    sourceFile:
      raw.source_file === null
        ? null
        : {
            name: raw.source_file.name,
            fileHash: raw.source_file.file_hash,
            firstRow: raw.source_file.first_row,
            lastRow: raw.source_file.last_row,
          },
    responseCode: raw.response_code,
    respondedAt: raw.responded_at,
    abortReason: raw.abort_reason,
    reasonCode: raw.reason_code,
    duplicateOf: raw.duplicate_of,
    revisionOf: raw.revision_of,
    revisedBy: raw.revised_by,
    deliveries: raw.deliveries.map(deliveryFromIpc),
  };
}

// ── Parsed fields ──────────────────────────────────────────────────────────────────────

export function fieldsFromIpc(raw: RawFields): MessageFields {
  return {
    fields: raw.fields.map((field) => ({ path: field.path, value: field.value })),
    truncated: raw.truncated,
    unavailable: raw.unavailable,
  };
}

// ── Bodies ─────────────────────────────────────────────────────────────────────────────

export function bodyRequestToIpc(request: BodyRequest): RawBodyRequest {
  return {
    message_id: request.messageId,
    which:
      request.which.kind === "original"
        ? { kind: "original" }
        : { kind: "transformed", delivery_id: request.which.deliveryId },
    as: request.as,
  };
}

/**
 * The engine renders both formats (`text`: control characters by name and one segment or
 * record per line; `hex`: a dump with offsets), so the console only shows `content`.
 */
export function bodyFromIpc(raw: RawBody): MessageBody {
  return {
    messageId: raw.message_id,
    which:
      raw.which.kind === "original"
        ? { kind: "original" }
        : { kind: "transformed", deliveryId: raw.which.delivery_id },
    as: raw.as,
    content: raw.content,
    byteLength: raw.byte_length,
    hash: raw.hash,
    truncated: raw.truncated,
  };
}

// ── Resend ─────────────────────────────────────────────────────────────────────────────

export function resendToIpc(request: ResendRequest): RawResendRequest {
  return {
    message_id: request.messageId,
    destinations: request.destinations === "all" ? "all" : { destinations: request.destinations },
    mode: request.mode,
    confirm_duplicate_risk: request.confirmDuplicateRisk,
    reason: request.reason?.trim() ? request.reason.trim() : null,
    confirm_superseded: request.confirmSuperseded ?? false,
  };
}

export function resendFromIpc(raw: RawResendResult): ResendResult {
  return {
    newMessageId: raw.new_message_id,
    destinations: raw.destinations.map((outcome) => ({
      destinationId: outcome.destination_id,
      destinationLabel: outcome.destination_label,
      deliveryId: outcome.delivery_id,
      rejected: outcome.rejected,
    })),
  };
}
