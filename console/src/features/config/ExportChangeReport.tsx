import { useState } from "react";
import { useConnection } from "@/api/hooks";
import type { ConfigVersionSummary } from "@/api/types";
import { Alert } from "@/components/Alert";
import { Button } from "@/components/Button";
import {
  ExportDialogFrame,
  ExportFormatSelect,
  useExportDone,
} from "@/components/ExportDialogFrame";
import { LoadingState } from "@/components/LoadingState";
import { Select } from "@/components/Select";
import { type ReportFormat, useChangeReportExport } from "./useChangeReport";

const FORMATS: readonly { id: ReportFormat; label: string }[] = [
  { id: "csv", label: "CSV (one sheet, a row per version, outcome, change and verification)" },
  { id: "json", label: "JSON (one document, nested by version)" },
];

interface ExportChangeReportProps {
  /** Every version of the history. */
  versions: readonly ConfigVersionSummary[];
  /** The version open in the history, which the report can be limited to. */
  selected: number | undefined;
}

function ExportDialog({
  versions,
  selected,
  onClose,
}: ExportChangeReportProps & { onClose: () => void }) {
  const [format, setFormat] = useState<ReportFormat>("csv");
  const [scope, setScope] = useState<"all" | "selected">("all");
  const done = useExportDone(onClose);
  const connection = useConnection();
  const exporting = useChangeReportExport();
  const chosen =
    scope === "selected" ? versions.filter((each) => each.version === selected) : versions;
  const scopes = [
    { id: "all", label: `All versions (${versions.length})` },
    ...(selected === undefined ? [] : [{ id: "selected", label: `Version ${selected} only` }]),
  ];
  function start() {
    exporting.mutate(
      {
        kept: versions,
        scope: chosen,
        format,
        generatedBy: connection.data?.actor ?? null,
      },
      {
        onSuccess: done(
          "Change report saved",
          (result) =>
            `${chosen.length === 1 ? "1 version" : `${chosen.length} versions`} saved to ${result.savedTo}.`,
        ),
      },
    );
  }
  const { progress } = exporting;
  return (
    <ExportDialogFrame
      title="Export change report"
      description="Saves the configuration's change-control record as a file."
      command={exporting}
      errorTitle="The change report could not be built"
      onSubmit={start}
      onClose={onClose}
    >
      <Select
        label="Versions"
        name="scope"
        options={scopes}
        value={scope}
        isDisabled={exporting.isPending}
        onChange={(next) => setScope(next === "selected" ? "selected" : "all")}
      />
      <ExportFormatSelect
        formats={FORMATS}
        value={format}
        isDisabled={exporting.isPending}
        onChange={setFormat}
      />
      <p>
        For each version: who applied it, when and why, what it did to each channel, what differs
        from the version before it (each field with its value before and after) and every
        verification with its account, time and reference. Version N is compared with version N-1:
        before is the value in N-1 and after the value in N. The oldest version has no comparison.
      </p>
      <Alert tone="warning" compact title="The file may contain patient information">
        Notes and verification references are text operators entered, and can include patient
        identifiers. Save the file only to the lab's protected location.
      </Alert>
      <Alert tone="info" compact>
        The console builds the file from the version history. The export is recorded in the audit
        log, with your account and the number of versions; it is refused when the log cannot record
        it. The file holds the laboratory's change-control record: account names, times, notes and
        references, and configuration values such as host names and ports, never secrets. Keep it
        where your quality procedures say.
      </Alert>
      {progress && (
        <LoadingState label={`Reading versions: ${progress.read} of ${progress.total}…`} />
      )}
    </ExportDialogFrame>
  );
}

/** "Export change report…": the history as a CSV or JSON file for the laboratory's records. */
export function ExportChangeReport(props: ExportChangeReportProps) {
  const [isOpen, setOpen] = useState(false);
  return (
    <>
      <Button onPress={() => setOpen(true)}>Export change report…</Button>
      {isOpen && <ExportDialog {...props} onClose={() => setOpen(false)} />}
    </>
  );
}
