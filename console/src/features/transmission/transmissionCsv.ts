import type { ReportFile, TransmissionReport, TransmissionRow } from "@/api/types";
import { toCsv } from "@/lib/csv";

/**
 * One line per delivery, each carrying the channel, period and generation time so a line read
 * alone says what it is. Codes are the engine's (`handed_off`, `timed_out`), times RFC 3339.
 */
export const TRANSMISSION_CSV_HEADER = [
  "channel",
  "period_since",
  "period_until",
  "generated_at",
  "message_id",
  "received_at",
  "source_endpoint_id",
  "source_peer",
  "config_version",
  "mapping_version",
  "message_state",
  "control_id",
  "content_hash",
  "destination_id",
  "destination",
  "delivery_state",
  "ack_level",
  "ack_code",
  "ack_control_id",
  "sent_hash",
  "attempts",
  "first_attempt_at",
  "acknowledged_at",
] as const;

/**
 * The last line of the CSV of a report that lists only the first deliveries of its period: the
 * same text the engine's own CSV (`packie messages verify --csv`) ends with, so a file filed apart
 * from the page still says it is not complete.
 */
export const TRUNCATED_MARKER =
  "# truncated: more than 10000 deliveries in the period; the rows above are the first 10000. Report a shorter period to list every delivery.";

/**
 * The CSV of `rows`, which are the report's deliveries as the operator has them narrowed. A
 * report that stopped at the row limit ends with a line saying how many deliveries it holds of
 * the period's and then `TRUNCATED_MARKER`, whatever the narrowing.
 */
export function transmissionCsv(
  report: TransmissionReport,
  rows: readonly TransmissionRow[],
): string {
  const labels = new Map(report.destinations.map((each) => [each.destinationId, each.label]));
  const sheet = toCsv(
    TRANSMISSION_CSV_HEADER,
    rows.map((row) => [
      report.channelId,
      report.since,
      report.until,
      report.generatedAt,
      row.messageId,
      row.receivedAt,
      row.sourceEndpointId,
      row.sourcePeer,
      row.configVersion,
      row.mappingVersion,
      row.messageState,
      row.controlId,
      row.contentHash,
      row.destinationId,
      labels.get(row.destinationId) ?? row.destinationId,
      row.state,
      row.ackLevel,
      row.ackCode,
      row.ackControlId,
      row.sentHash,
      row.attempts,
      row.firstAttemptAt,
      row.acknowledgedAt,
    ]),
  );
  if (!report.truncated) return sheet;
  // How much of the period the file holds, then the engine's marker as the last line.
  const listed = `# listed: ${report.rows.length} of ${report.periodRowsTotal} deliveries of the period were in the report (limit ${report.rowsLimit}); ${report.messages} messages were received`;
  return `${sheet}${listed}\r\n${TRUNCATED_MARKER}\r\n`;
}

/** `packie-transmission-<channel>-<date>.csv`, the date being the day the report was generated. */
export function transmissionFileName(report: TransmissionReport): string {
  const channel = report.channelId.replace(/[^A-Za-z0-9._-]/g, "_");
  return `packie-transmission-${channel}-${report.generatedAt.slice(0, 10)}.csv`;
}

export function transmissionFile(
  report: TransmissionReport,
  rows: readonly TransmissionRow[],
): ReportFile {
  return {
    name: transmissionFileName(report),
    format: "csv",
    content: transmissionCsv(report, rows),
  };
}
