import type { DestinationTransmission } from "@/api/types";
import { formatCount } from "@/lib/format";
import layout from "@/styles/layout.module.css";
import styles from "./Transmission.module.css";

interface Column {
  id: string;
  header: string;
  /** What the column counts, as the header's title. */
  meaning: string;
  count: (row: DestinationTransmission) => number;
}

const COLUMNS: readonly Column[] = [
  {
    id: "deliveries",
    header: "Deliveries",
    meaning: "Deliveries created for the messages received in the period.",
    count: (row) => row.deliveries,
  },
  {
    id: "accepted",
    header: "Accepted",
    meaning: "Acknowledged as accepted by the destination's application.",
    count: (row) => row.accepted,
  },
  {
    id: "received",
    header: "Received",
    meaning: "The destination received the message. Processing was not confirmed.",
    count: (row) => row.received,
  },
  {
    id: "handedOff",
    header: "Written, not confirmed",
    meaning: "The message was written to the destination, which did not confirm it.",
    count: (row) => row.handedOff,
  },
  {
    id: "unresolved",
    header: "Unresolved",
    meaning: "Rejected, failed, no acknowledgement in time, or outcome unknown.",
    count: (row) => row.unresolved,
  },
  {
    id: "closed",
    header: "Closed",
    meaning: "Closed by an operator without a confirmation.",
    count: (row) => row.closed,
  },
  {
    id: "pending",
    header: "Pending",
    meaning: "Still queued or being sent.",
    count: (row) => row.pending,
  },
];

/** What each destination confirmed, for every destination of the report. */
export function TotalsTable({
  destinations,
}: {
  destinations: readonly DestinationTransmission[];
}) {
  if (destinations.length === 0) {
    return <p className={layout.hint}>No deliveries were created in this period.</p>;
  }
  return (
    <>
      <div className={styles.scroll}>
        <table aria-label="Totals by destination" className={styles.totals}>
          <thead>
            <tr>
              <th scope="col">Destination</th>
              {COLUMNS.map((column) => (
                <th key={column.id} scope="col" data-align="end" title={column.meaning}>
                  {column.header}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {destinations.map((row) => (
              <tr key={row.destinationId}>
                <th scope="row">{row.label}</th>
                {COLUMNS.map((column) => (
                  <td key={column.id} data-align="end">
                    {formatCount(column.count(row))}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <p className={layout.hint}>
        Only Accepted means the destination's application took the message. Received and Written,
        not confirmed prove less: they do not show that it was processed.
      </p>
    </>
  );
}
