import { useMemo, useState } from "react";
import type { TransmissionReport } from "@/api/types";
import { Button } from "@/components/Button";
import { Checkbox } from "@/components/Checkbox";
import { DataGrid } from "@/components/DataGrid";
import { EmptyState } from "@/components/EmptyState";
import { Section } from "@/components/Section";
import { Select } from "@/components/Select";
import { Toolbar } from "@/components/Toolbar";
import { formatCount } from "@/lib/format";
import { ExportDialog } from "./ExportDialog";
import { rowId, transmissionColumns } from "./transmissionColumns";

const ALL = "__all";

interface DeliveriesSectionProps {
  report: TransmissionReport;
  /** Destination labels by id. */
  labels: ReadonlyMap<string, string>;
  /** A destination of the report, or undefined for all of them. */
  destination: string | undefined;
  onDestination: (destination: string | undefined) => void;
}

const deliveries = (count: number) =>
  `${formatCount(count)} ${count === 1 ? "delivery" : "deliveries"}`;

/**
 * The report's deliveries, narrowed to one destination if asked, and the export of exactly
 * the rows shown.
 */
export function DeliveriesSection({
  report,
  labels,
  destination,
  onDestination,
}: DeliveriesSectionProps) {
  const [details, setDetails] = useState(false);
  const [sources, setSources] = useState(false);
  const [exporting, setExporting] = useState(false);
  const columns = useMemo(
    () => transmissionColumns(labels, details, sources),
    [labels, details, sources],
  );
  const rows = useMemo(
    () =>
      destination === undefined
        ? report.rows
        : report.rows.filter((row) => row.destinationId === destination),
    [report.rows, destination],
  );
  const options = useMemo(
    () => [
      { id: ALL, label: "All destinations" },
      ...report.destinations.map((each) => ({ id: each.destinationId, label: each.label })),
    ],
    [report.destinations],
  );
  const scope =
    destination === undefined ? "all destinations" : (labels.get(destination) ?? destination);
  return (
    <Section title="Deliveries">
      <Toolbar
        count={
          rows.length === report.rows.length
            ? deliveries(rows.length)
            : `${formatCount(rows.length)} of ${deliveries(report.rows.length)}`
        }
        actions={
          <Button isDisabled={rows.length === 0} onPress={() => setExporting(true)}>
            Export CSV…
          </Button>
        }
      >
        <Select
          layout="inline"
          label="Destination"
          name="destination"
          options={options}
          value={destination ?? ALL}
          onChange={(next) => onDestination(next === null || next === ALL ? undefined : next)}
        />
        <Checkbox isSelected={details} onChange={setDetails}>
          Show sent hash, attempts and first attempt
        </Checkbox>
        <Checkbox isSelected={sources} onChange={setSources}>
          Show source and versions
        </Checkbox>
      </Toolbar>
      {rows.length === 0 ? (
        <EmptyState
          title="No deliveries to list"
          description={
            destination === undefined
              ? "None of the messages received in this period was sent on: filtered, duplicate and invalid messages create no delivery."
              : `No message of this period was sent to ${scope}.`
          }
          action={
            destination === undefined ? undefined : (
              <Button onPress={() => onDestination(undefined)}>Show all destinations</Button>
            )
          }
        />
      ) : (
        <DataGrid label="Deliveries" columns={columns} rows={rows} getRowId={rowId} />
      )}
      {exporting && (
        <ExportDialog
          report={report}
          rows={rows}
          scope={scope}
          onClose={() => setExporting(false)}
        />
      )}
    </Section>
  );
}
