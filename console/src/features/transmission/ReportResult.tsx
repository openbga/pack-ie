import { useMemo } from "react";
import type { TransmissionReport } from "@/api/types";
import { Alert } from "@/components/Alert";
import { EmptyState } from "@/components/EmptyState";
import { Facts } from "@/components/Facts";
import { Section } from "@/components/Section";
import { Timestamp } from "@/components/Timestamp";
import { formatCount } from "@/lib/format";
import { DeliveriesSection } from "./DeliveriesSection";
import { TotalsTable } from "./TotalsTable";
import styles from "./Transmission.module.css";

interface ReportResultProps {
  report: TransmissionReport;
  /** The channel's label, or its id when the configuration no longer names it. */
  channelLabel: string;
  /** The destination the grid and the export are narrowed to; the totals are not. */
  destination: string | undefined;
  onDestination: (destination: string | undefined) => void;
}

/**
 * What one run of the report found: its summary, the totals of every destination, and the
 * deliveries (all destinations or one) with the export of the rows shown.
 */
export function ReportResult({
  report,
  channelLabel,
  destination,
  onDestination,
}: ReportResultProps) {
  const labels = useMemo(
    () => new Map(report.destinations.map((each) => [each.destinationId, each.label])),
    [report.destinations],
  );
  const summary = (
    <Section title="Report">
      <Facts
        layout="compact"
        items={[
          { label: "Channel", value: channelLabel },
          {
            label: "Period",
            value: (
              <>
                <Timestamp value={report.since} seconds /> to{" "}
                <Timestamp value={report.until} seconds />
              </>
            ),
          },
          { label: "Generated", value: <Timestamp value={report.generatedAt} seconds /> },
          { label: "Messages received", value: formatCount(report.messages) },
        ]}
      />
    </Section>
  );
  if (report.messages === 0) {
    return (
      <div className={styles.results}>
        {summary}
        <EmptyState
          title="No messages were received in this period"
          description="Choose a longer period, or check that the channel was running."
        />
      </div>
    );
  }
  return (
    <div className={styles.results}>
      {summary}
      {report.truncated && (
        <Alert tone="warning" title="The report is incomplete">
          Only the first {formatCount(report.rows.length)} of {formatCount(report.periodRowsTotal)}{" "}
          deliveries are listed. Choose a shorter period to list every one. Messages received and
          the totals below cover the whole period.
        </Alert>
      )}
      <Section title="Totals by destination">
        <TotalsTable destinations={report.destinations} />
      </Section>
      <DeliveriesSection
        report={report}
        labels={labels}
        // A destination the report does not list narrows nothing, as if none were chosen.
        destination={destination !== undefined && labels.has(destination) ? destination : undefined}
        onDestination={onDestination}
      />
    </div>
  );
}
