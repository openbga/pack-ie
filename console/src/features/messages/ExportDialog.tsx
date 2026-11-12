import { useState } from "react";
import { useEngineMutation } from "@/api/hooks";
import type { MessageExportFormat, MessageExportRequest } from "@/api/types";
import { Alert } from "@/components/Alert";
import { Checkbox } from "@/components/Checkbox";
import {
  ExportDialogFrame,
  ExportFormatSelect,
  useExportDone,
} from "@/components/ExportDialogFrame";
import { formatCount } from "@/lib/format";

const FORMATS: readonly { id: MessageExportFormat; label: string }[] = [
  { id: "csv", label: "CSV (comma-separated values)" },
  { id: "jsonl", label: "JSON lines (one message per line)" },
];

const plural = (count: number) => (count === 1 ? "message" : "messages");

interface ExportDialogProps {
  /** The filters in words; the time range is always among them. */
  filters: readonly string[];
  /** How many messages match, when the list knows. */
  total: number | null;
  /** The engine's filter, read when the export starts so a time range counts back from then. */
  filter: () => MessageExportRequest["filter"];
  onClose: () => void;
}

/**
 * Exports every message the list's filters select, oldest first (spec §6 Export). The file
 * lists patient identifiers whatever is chosen, which the dialog says; the original bodies are a
 * separate, explicit choice with a stronger warning, which the engine records as a content
 * view. The shell asks where to save the file; cancelling that does nothing. Mount it only
 * while it is open.
 */
export function ExportDialog({ filters, total, filter, onClose }: ExportDialogProps) {
  const [format, setFormat] = useState<MessageExportFormat>("csv");
  const [includeBodies, setIncludeBodies] = useState(false);
  const exporting = useEngineMutation(
    (api, request: MessageExportRequest) => api.exportMessages(request),
    { invalidates: ["getAuditEvents"] },
  );
  const done = useExportDone(onClose);
  function start() {
    exporting.mutate(
      { filter: filter(), format, includeBodies },
      {
        onSuccess: done(
          "Messages exported",
          (result) =>
            `${formatCount(result.rows)} ${plural(result.rows)} saved to ${result.savedTo}${includeBodies ? ", with their content" : ""}.`,
        ),
      },
    );
  }
  return (
    <ExportDialogFrame
      title="Export messages"
      description="Saves every message that matches the current filters, oldest first."
      command={exporting}
      errorTitle="The export failed"
      errorNote="A partial file is not kept."
      onSubmit={start}
      onClose={onClose}
    >
      <ExportFormatSelect formats={FORMATS} value={format} onChange={setFormat} />
      <div>
        <p>Filters in effect: {filters.join(", ")}.</p>
        <p>
          {total === null
            ? "The number of matching messages is not known yet."
            : total === 0
              ? "No messages match: the file would hold only its header."
              : `${formatCount(total)} ${plural(total)} match.`}
        </p>
      </div>
      <Alert tone="warning" title="The file lists patient identifiers" compact>
        Each message's row holds its sample, patient and control identifiers, even without the
        message content. Save the file to a protected location and delete it when it is no longer
        needed. The engine records this export in the audit log.
      </Alert>
      <Checkbox isSelected={includeBodies} onChange={setIncludeBodies}>
        Include message content (original bodies)
      </Checkbox>
      {includeBodies && (
        <Alert tone="warning" title="The file will contain message content" compact>
          Message content can include patient information. Save the file to a protected location,
          share it only with people who may see it, and delete it when it is no longer needed. The
          engine records this export in the audit log as a content view.
        </Alert>
      )}
    </ExportDialogFrame>
  );
}
