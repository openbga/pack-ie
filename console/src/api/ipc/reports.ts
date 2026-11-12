// Reports over the message store (`export_messages`, `verify_transmission`): the console's
// requests to the engine's and the report back. Pure.

import type {
  ExportRequest as RawExport,
  TransmissionQuery as RawQuery,
  TransmissionReport as RawReport,
} from "@/ipc/generated";
import type { MessageExportRequest, TransmissionQuery, TransmissionReport } from "../types";
import { filterToIpc } from "./messageMap";

/** What `save_message_export` takes besides the suggested file name; the shell adds `offset`. */
export function exportToIpc(
  request: MessageExportRequest,
): Omit<RawExport, "offset" | "export_id"> {
  return {
    filter: filterToIpc(request.filter),
    format: request.format,
    include_bodies: request.includeBodies,
  };
}

export function transmissionToIpc(query: TransmissionQuery): RawQuery {
  return { channel_id: query.channelId, since: query.since, until: query.until ?? null };
}

export function transmissionFromIpc(raw: RawReport): TransmissionReport {
  return {
    channelId: raw.channel_id,
    since: raw.since,
    until: raw.until,
    generatedAt: raw.generated_at,
    messages: raw.messages,
    destinations: raw.destinations.map((each) => ({
      destinationId: each.destination_id,
      label: each.label,
      deliveries: each.deliveries,
      accepted: each.accepted,
      received: each.received,
      handedOff: each.handed_off,
      unresolved: each.unresolved,
      closed: each.closed,
      pending: each.pending,
    })),
    rows: raw.rows.map((row) => ({
      messageId: row.message_id,
      receivedAt: row.received_at,
      messageState: row.message_state,
      controlId: row.control_id,
      sourceEndpointId: row.source_endpoint_id,
      sourcePeer: row.source_peer,
      configVersion: row.config_version,
      // The engine gives none for a delivery that was never mapped.
      mappingVersion: row.mapping_version ?? null,
      contentHash: row.content_hash,
      destinationId: row.destination_id,
      state: row.state,
      ackLevel: row.ack_level,
      ackCode: row.ack_code,
      ackControlId: row.ack_control_id,
      sentHash: row.sent_hash,
      attempts: row.attempts,
      firstAttemptAt: row.first_attempt_at,
      acknowledgedAt: row.acknowledged_at,
    })),
    truncated: raw.truncated,
    rowsLimit: raw.rows_limit,
    periodRowsTotal: raw.period_rows_total,
  };
}
