import { Link } from "react-router";
import type { TransmissionRow } from "@/api/types";
import type { DataGridColumn } from "@/components/DataGrid";
import { StatusBadge } from "@/components/StatusBadge";
import { Timestamp } from "@/components/Timestamp";
import styles from "./Transmission.module.css";
import { ackLevelLabel, ackLevelSentence, stateStatus } from "./transmissionWording";

const HASH_SHOWN = 12;

/** A hash in its first characters; the whole of it is the title. */
function Hash({ value }: { value: string | null }) {
  if (value === null) return <span className={styles.muted}>None</span>;
  return (
    <span className={styles.hash} title={value}>
      {value.length > HASH_SHOWN ? `${value.slice(0, HASH_SHOWN)}…` : value}
    </span>
  );
}

const time = (value: string | null, none: string) =>
  value === null ? (
    <span className={styles.muted}>{none}</span>
  ) : (
    <Timestamp value={value} compact seconds />
  );

/** The id of one delivery's row: a message has one row per destination. */
export const rowId = (row: TransmissionRow) => `${row.messageId}/${row.destinationId}`;

/**
 * The columns of the report. `labels` names the destinations; `details` adds the sent hash,
 * attempts and first attempt, `sources` the source endpoint and peer and the configuration and
 * mapping versions the delivery ran under.
 */
export function transmissionColumns(
  labels: ReadonlyMap<string, string>,
  details: boolean,
  sources: boolean,
): readonly DataGridColumn<TransmissionRow>[] {
  const destination = (row: TransmissionRow) => labels.get(row.destinationId) ?? row.destinationId;
  const columns: DataGridColumn<TransmissionRow>[] = [
    {
      id: "received",
      header: "Received",
      identifier: true,
      width: 150,
      sortValue: (row) => row.receivedAt,
      cell: (row) => <Timestamp value={row.receivedAt} compact seconds />,
    },
    {
      id: "message",
      header: "Message",
      width: 150,
      sortValue: (row) => row.messageId,
      cell: (row) => (
        <Link to={`/messages/${encodeURIComponent(row.messageId)}`} title={row.messageId}>
          {row.messageId}
        </Link>
      ),
    },
    {
      id: "control",
      header: "Control ID",
      width: 130,
      sortValue: (row) => row.controlId ?? undefined,
      cell: (row) => row.controlId ?? <span className={styles.muted}>Not provided</span>,
    },
    {
      id: "contentHash",
      header: "Content hash",
      width: 140,
      cell: (row) => <Hash value={row.contentHash} />,
    },
    {
      id: "destination",
      header: "Destination",
      grow: 1,
      sortValue: destination,
      cell: destination,
    },
    {
      id: "ackLevel",
      header: "Ack level",
      width: 190,
      sortValue: (row) => ackLevelLabel(row.ackLevel),
      cell: (row) => (
        <span title={ackLevelSentence(row.ackLevel)}>{ackLevelLabel(row.ackLevel)}</span>
      ),
    },
    {
      id: "ackCode",
      header: "Ack code",
      width: 90,
      sortValue: (row) => row.ackCode ?? undefined,
      cell: (row) => row.ackCode ?? <span className={styles.muted}>None</span>,
    },
    {
      id: "ackControlId",
      header: "ACK ID",
      width: 130,
      sortValue: (row) => row.ackControlId ?? undefined,
      cell: (row) => row.ackControlId ?? <span className={styles.muted}>None</span>,
    },
    {
      id: "acknowledgedAt",
      header: "Acknowledged at",
      width: 150,
      sortValue: (row) => row.acknowledgedAt ?? undefined,
      cell: (row) => time(row.acknowledgedAt, "Not acknowledged"),
    },
    {
      id: "state",
      header: "Outcome",
      grow: 1,
      wrap: true,
      sortValue: (row) => stateStatus(row.state).label,
      cell: (row) => {
        const status = stateStatus(row.state);
        return <StatusBadge tone={status.tone}>{status.label}</StatusBadge>;
      },
    },
  ];
  if (details) {
    columns.push(
      {
        id: "sentHash",
        header: "Sent hash",
        width: 140,
        cell: (row) => <Hash value={row.sentHash} />,
      },
      {
        id: "attempts",
        header: "Attempts",
        width: 90,
        align: "end",
        sortValue: (row) => row.attempts,
        cell: (row) => row.attempts,
      },
      {
        id: "firstAttempt",
        header: "First attempt",
        width: 150,
        sortValue: (row) => row.firstAttemptAt ?? undefined,
        cell: (row) => time(row.firstAttemptAt, "No attempt"),
      },
    );
  }
  if (sources) {
    columns.push(
      {
        id: "sourceEndpoint",
        header: "Source endpoint",
        width: 150,
        sortValue: (row) => row.sourceEndpointId,
        cell: (row) => row.sourceEndpointId,
      },
      {
        id: "sourcePeer",
        header: "Source peer",
        width: 140,
        sortValue: (row) => row.sourcePeer ?? undefined,
        cell: (row) => row.sourcePeer ?? <span className={styles.muted}>Not recorded</span>,
      },
      {
        id: "configVersion",
        header: "Config version",
        width: 110,
        align: "end",
        sortValue: (row) => row.configVersion,
        cell: (row) => row.configVersion,
      },
      {
        id: "mappingVersion",
        header: "Mapping version",
        width: 120,
        align: "end",
        sortValue: (row) => row.mappingVersion ?? undefined,
        cell: (row) => row.mappingVersion ?? <span className={styles.muted}>No mapping</span>,
      },
    );
  }
  return columns;
}
