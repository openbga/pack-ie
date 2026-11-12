import { toCsv } from "@/lib/csv";
import type { MessageExportRequest, MessageSummary } from "../types";
import type { MockWorld } from "./world";

/**
 * The most messages the mock writes to one file. The mock history holds up to 412,000 messages
 * and builds the file in memory; the engine streams the whole selection. The count the audit log
 * records is that of every match, as the engine's is.
 */
const MOCK_EXPORT_LIMIT = 5000;

/** The columns of a CSV export, as the engine writes them; `body` follows when content is included. */
const CSV_COLUMNS = [
  "message_id",
  "received_at",
  "channel_id",
  "direction",
  "state",
  "protocol",
  "message_type",
  "control_id",
  "sample_id",
  "patient_id",
  "byte_length",
  "deliveries",
] as const;

/** `lis:acknowledged:accepted;lims:queued:none`: the deliveries of a CSV row in one cell. */
function deliveriesCell(message: MessageSummary): string {
  return message.deliveries.map((d) => `${d.destinationId}:${d.state}:${d.ackLevel}`).join(";");
}

function csvRow(message: MessageSummary, body: string | null): (string | number | null)[] {
  return [
    message.id,
    message.receivedAt,
    message.channelId,
    message.direction,
    message.state,
    message.protocol,
    message.messageType,
    message.controlId,
    message.sampleId,
    message.patientId,
    message.sizeBytes,
    deliveriesCell(message),
    ...(body === null ? [] : [body]),
  ];
}

function jsonRow(message: MessageSummary, body: string | null): string {
  return JSON.stringify({
    message_id: message.id,
    received_at: message.receivedAt,
    channel_id: message.channelId,
    direction: message.direction,
    state: message.state,
    protocol: message.protocol,
    message_type: message.messageType,
    control_id: message.controlId,
    sample_id: message.sampleId,
    patient_id: message.patientId,
    byte_length: message.sizeBytes,
    deliveries: message.deliveries.map((d) => ({
      destination_id: d.destinationId,
      state: d.state,
      ack_level: d.ackLevel,
    })),
    ...(body === null ? {} : { body }),
  });
}

/**
 * The text of an export of the messages `request` selects, oldest first (operator-console.md
 * §6 Export), with the number of messages in it and of those that match. The filter is applied
 * as the Messages list applies it; a selection of more than `MOCK_EXPORT_LIMIT` is cut there.
 */
export function exportMessagesText(
  world: MockWorld,
  request: MessageExportRequest,
): { content: string; rows: number; matching: number } {
  const { rows: messages, total } = world.messages.select(request.filter, MOCK_EXPORT_LIMIT);
  const bodies = messages.map((message) =>
    request.includeBodies ? world.messages.originalText(message.id) : null,
  );
  const content =
    request.format === "csv"
      ? toCsv(
          request.includeBodies ? [...CSV_COLUMNS, "body"] : CSV_COLUMNS,
          messages.map((message, i) => csvRow(message, bodies[i] ?? null)),
        )
      : messages.map((message, i) => `${jsonRow(message, bodies[i] ?? null)}\n`).join("");
  return { content, rows: messages.length, matching: total };
}
