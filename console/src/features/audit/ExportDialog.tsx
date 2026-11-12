import { useState } from "react";
import { useEngineMutation } from "@/api/hooks";
import type { AuditExportFormat, AuditFilter } from "@/api/types";
import { Alert } from "@/components/Alert";
import {
  ExportDialogFrame,
  ExportFormatSelect,
  useExportDone,
} from "@/components/ExportDialogFrame";
import { formatCount } from "@/lib/format";

const FORMATS: readonly { id: AuditExportFormat; label: string }[] = [
  { id: "csv", label: "CSV (comma-separated values)" },
  { id: "json_lines", label: "JSON lines (one event per line)" },
];

interface ExportDialogProps {
  /** The filters in words; empty when every event is exported. */
  filters: readonly string[];
  /** The engine's filter, read when the export starts so a time preset counts back from then. */
  filter: () => AuditFilter;
  onClose: () => void;
}

/**
 * Exports the events matching the current filters, oldest first, in a format. The engine asks
 * where to save the file; cancelling that does nothing. Mount it only while it is open.
 */
export function ExportDialog({ filters, filter, onClose }: ExportDialogProps) {
  const [format, setFormat] = useState<AuditExportFormat>("csv");
  const exporting = useEngineMutation(
    (api, request: { format: AuditExportFormat; filter: AuditFilter }) => api.exportAudit(request),
    { invalidates: ["getAuditEvents"] },
  );
  const done = useExportDone(onClose);
  function start() {
    exporting.mutate(
      { format, filter: filter() },
      {
        onSuccess: done(
          "Audit log exported",
          (result) =>
            `${formatCount(result.rows)} ${result.rows === 1 ? "event" : "events"} saved to ${result.savedTo}.`,
        ),
      },
    );
  }
  return (
    <ExportDialogFrame
      title="Export audit events"
      description="Saves every event that matches the current filters, oldest first."
      command={exporting}
      errorTitle="The export failed"
      onSubmit={start}
      onClose={onClose}
    >
      <ExportFormatSelect formats={FORMATS} value={format} onChange={setFormat} />
      <p>
        {filters.length === 0
          ? "No filters are set: every event is exported."
          : `Filters in effect: ${filters.join(", ")}.`}
      </p>
      <Alert tone="warning" compact title="The file may contain patient information">
        Details can include patient identifiers that were searched for and text operators entered,
        such as reasons and notes. Save the file only to the lab's protected location.
      </Alert>
      <Alert tone="info" compact>
        The export is recorded in the audit log, with your account. The file holds the events'
        details, which can include text operators entered.
      </Alert>
    </ExportDialogFrame>
  );
}
