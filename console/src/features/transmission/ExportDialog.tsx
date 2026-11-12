import { useEngineMutation } from "@/api/hooks";
import type { ReportFile, TransmissionReport, TransmissionRow } from "@/api/types";
import { Alert } from "@/components/Alert";
import { ExportDialogFrame, useExportDone } from "@/components/ExportDialogFrame";
import { formatCount } from "@/lib/format";
import { transmissionFile } from "./transmissionCsv";

interface ExportDialogProps {
  report: TransmissionReport;
  /** The deliveries shown: the destination filter is already applied. */
  rows: readonly TransmissionRow[];
  /** "all destinations" or the destination's label. */
  scope: string;
  onClose: () => void;
}

/**
 * Saves the deliveries shown as a CSV file. The shell asks where to save it; cancelling that
 * does nothing. Mount it only while it is open.
 */
export function ExportDialog({ report, rows, scope, onClose }: ExportDialogProps) {
  const saving = useEngineMutation((api, file: ReportFile) => api.saveReportFile(file));
  const done = useExportDone(onClose);
  const count = `${formatCount(rows.length)} ${rows.length === 1 ? "delivery" : "deliveries"}`;
  function start() {
    saving.mutate(transmissionFile(report, rows), {
      onSuccess: done(
        "Transmission report exported",
        (saved) => `${count} saved to ${saved.savedTo}.`,
      ),
    });
  }
  return (
    <ExportDialogFrame
      title="Export transmission report"
      description="Saves the deliveries shown as a CSV file, one line each."
      submitLabel="Export CSV"
      command={saving}
      errorTitle="The report could not be saved"
      onSubmit={start}
      onClose={onClose}
    >
      <p>
        {count} for {scope}.
      </p>
      {report.truncated && (
        <Alert tone="warning" compact title="This report is incomplete">
          The period has more deliveries than one report lists: only the first{" "}
          {formatCount(report.rows.length)} of {formatCount(report.periodRowsTotal)} are in the
          report, so the file holds them and no more. The file ends with a line starting “#
          truncated:” that says so. The totals on the page cover the whole period; the file has no
          totals. For the laboratory's records, close this and report a shorter period.
        </Alert>
      )}
      <Alert tone="info" compact>
        The file lists each message's control id, which can point to a patient's result, and the
        hashes of its content. It holds no message content. Keep it with the laboratory's records.
      </Alert>
    </ExportDialogFrame>
  );
}
