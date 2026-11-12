import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useEngineMutation } from "@/api/hooks";
import type { ConfigDocument, ConfigVersionSummary, SavedReport } from "@/api/types";
import {
  buildChangeReport,
  changeReportCsv,
  changeReportJson,
  type ReportVersion,
} from "./changeReport";

export type ReportFormat = "csv" | "json";

/** Versions read at once: enough to be quick, few enough not to flood the engine. */
const READ_AT_ONCE = 6;

export interface ChangeReportRequest {
  /** Every kept version, so a version's predecessor can be told from one that is gone. */
  kept: readonly ConfigVersionSummary[];
  /** The versions the report covers. */
  scope: readonly ConfigVersionSummary[];
  format: ReportFormat;
  generatedBy: string | null;
}

/** Runs `read` over `items`, `limit` at a time, calling `onEach` as each finishes. */
async function readAll<Item, Result>(
  items: readonly Item[],
  read: (item: Item) => Promise<Result>,
  onEach: () => void,
): Promise<Result[]> {
  const results: Result[] = new Array(items.length);
  let next = 0;
  async function worker() {
    while (next < items.length) {
      const index = next++;
      results[index] = await read(items[index] as Item);
      onEach();
    }
  }
  await Promise.all(Array.from({ length: Math.min(READ_AT_ONCE, items.length) }, worker));
  return results;
}

/**
 * Builds the change report of some versions and saves it: reads each version's document (and its
 * predecessor's, to compare with), builds the file and hands it to the engine's save dialog.
 * `progress` counts the documents read while it runs; a read that fails comes back as `error`,
 * and starting again reuses what was already read. Resolves with null when the operator cancels
 * the save dialog.
 */
export function useChangeReportExport() {
  const client = useQueryClient();
  const [progress, setProgress] = useState<{ read: number; total: number } | null>(null);
  const exporting = useEngineMutation<ChangeReportRequest, SavedReport | null>(
    async (api, { kept, scope, format, generatedBy }) => {
      const keptNumbers = new Set(kept.map((each) => each.version));
      const needed = [
        ...new Set(
          scope.flatMap((each) =>
            [each.version, each.version - 1].filter((n) => keptNumbers.has(n)),
          ),
        ),
      ];
      let read = 0;
      setProgress({ read, total: needed.length });
      try {
        const documents = new Map<number, ConfigDocument>(
          await readAll(
            needed,
            async (version) =>
              [
                version,
                // A saved version never changes, so a document read before is still right.
                await client.fetchQuery({
                  queryKey: ["getConfigVersion", version],
                  queryFn: () => api.getConfigVersion(version),
                  staleTime: Number.POSITIVE_INFINITY,
                }),
              ] as const,
            () => setProgress({ read: ++read, total: needed.length }),
          ),
        );
        const versions = [...scope]
          .sort((a, b) => a.version - b.version)
          .map(
            (summary): ReportVersion => ({
              summary,
              document: documents.get(summary.version) as ConfigDocument,
              previous: documents.get(summary.version - 1) ?? null,
            }),
          );
        const generatedAt = new Date().toISOString();
        const report = buildChangeReport({ generatedAt, generatedBy, versions });
        const scopeName = versions.length === 1 ? `v${versions[0]?.summary.version}-` : "";
        return await api.saveReportFile({
          name: `packie-change-report-${scopeName}${generatedAt.slice(0, 10)}.${format}`,
          format,
          content: format === "csv" ? changeReportCsv(report) : changeReportJson(report),
          audit: { kind: "change_report", rows: versions.length },
        });
      } finally {
        setProgress(null);
      }
    },
  );
  return { ...exporting, progress };
}
