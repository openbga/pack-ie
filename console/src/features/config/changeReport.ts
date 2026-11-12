// The change report: the lab's change-control record of the configuration, built here from the
// version history (who applied each version, when, why, what it did to each channel, what
// differs from the version before, and every sign-off) as one portable file.
import type { ConfigDocument, ConfigVersionSummary } from "@/api/types";
import { toCsv } from "@/lib/csv";
import { compareDocuments, type DocumentComparison, type FieldChange } from "./documentCompare";
import { channelNamer, sortedOutcomes } from "./outcomes";

/** A version with its document and the document of the version before it, when that is kept. */
export interface ReportVersion {
  summary: ConfigVersionSummary;
  document: ConfigDocument;
  /** Null for the oldest version, which has nothing to be compared with. */
  previous: ConfigDocument | null;
}

export interface ReportChange {
  field: string;
  before: string;
  after: string;
}

export interface ChangeReport {
  generated_at: string;
  /** The account the console is signed in as; null when the engine has not said. */
  generated_by: string | null;
  /** How to read `comparison` of each version. */
  comparison_direction: string;
  versions: {
    version: number;
    applied_by: string | null;
    applied_at: string;
    note: string | null;
    outcomes: {
      channel_id: string;
      channel: string;
      outcome: string;
      reason: string | null;
    }[];
    verifications: { verified_by: string; verified_at: string; reference: string }[];
    /** Null for the oldest version. */
    comparison: { against: number; changes: ReportChange[] } | null;
  }[];
}

export const COMPARISON_DIRECTION =
  "Each version is compared with the version before it (N compared with N-1): before is the value in N-1, after is the value in N. The oldest version has no comparison.";

/** Every difference of a comparison as `field`, `before`, `after`; arrays list their members too. */
function flatten(label: string, change: FieldChange): ReportChange[] {
  const field = label === "" ? change.label : `${label} › ${change.label}`;
  return [
    { field, before: change.before, after: change.after },
    ...(change.detail ?? []).flatMap((each) => flatten(field, each)),
  ];
}

/** The comparison as flat changes: each item that was added, removed or changed, then thresholds. */
export function flattenComparison(comparison: DocumentComparison): ReportChange[] {
  const items = (noun: string, changes: DocumentComparison["channels"]) =>
    changes.flatMap((item): ReportChange[] => {
      const name = `${noun} ${item.label} (${item.id})`;
      if (item.kind === "added") return [{ field: name, before: "not present", after: "added" }];
      if (item.kind === "removed") return [{ field: name, before: "present", after: "removed" }];
      return item.fields.flatMap((field) => flatten(name, field));
    });
  return [
    ...items("Endpoint", comparison.endpoints),
    ...items("Channel", comparison.channels),
    ...comparison.thresholds.flatMap((each) => flatten("Alert thresholds", each)),
  ];
}

interface ReportInput {
  generatedAt: string;
  generatedBy: string | null;
  /** Oldest first. */
  versions: readonly ReportVersion[];
}

export function buildChangeReport({
  generatedAt,
  generatedBy,
  versions,
}: ReportInput): ChangeReport {
  return {
    generated_at: generatedAt,
    generated_by: generatedBy,
    comparison_direction: COMPARISON_DIRECTION,
    versions: versions.map(({ summary, document, previous }) => {
      const nameOf = channelNamer(document.channels, previous?.channels);
      return {
        version: summary.version,
        applied_by: summary.appliedBy,
        applied_at: summary.appliedAt,
        note: summary.note,
        outcomes: sortedOutcomes(summary.outcomes).map((each) => ({
          channel_id: each.channelId,
          channel: nameOf(each.channelId),
          outcome: each.outcome,
          reason: each.reason ?? null,
        })),
        verifications: summary.verifications.map((each) => ({
          verified_by: each.by,
          verified_at: each.at,
          reference: each.note,
        })),
        comparison:
          previous === null
            ? null
            : {
                against: summary.version - 1,
                changes: flattenComparison(compareDocuments(previous, document)),
              },
      };
    }),
  };
}

export function changeReportJson(report: ChangeReport): string {
  return `${JSON.stringify(report, null, 2)}\n`;
}

const CSV_HEADER = [
  "kind",
  "version",
  "applied_by",
  "applied_at",
  "note",
  "compared_with",
  "channel_id",
  "channel",
  "outcome",
  "reason",
  "field",
  "before",
  "after",
  "verified_by",
  "verified_at",
  "reference",
] as const;

/**
 * One flat sheet: a `report` row that says when and by whom it was made and how to read the
 * comparisons, then per version a `version` row (who, when, why), an `outcome` row per channel,
 * a `change` row per difference from the version before and a `verification` row per sign-off.
 */
export function changeReportCsv(report: ChangeReport): string {
  const row = (values: Partial<Record<(typeof CSV_HEADER)[number], string | number | null>>) =>
    CSV_HEADER.map((column) => values[column] ?? "");
  const rows = [
    row({
      kind: "report",
      note: `Change report generated ${report.generated_at} by ${report.generated_by ?? "an account the engine did not name"}. ${report.comparison_direction}`,
    }),
    ...report.versions.flatMap((each) => [
      row({
        kind: "version",
        version: each.version,
        applied_by: each.applied_by,
        applied_at: each.applied_at,
        note: each.note,
        compared_with: each.comparison?.against,
      }),
      ...each.outcomes.map((outcome) =>
        row({ kind: "outcome", version: each.version, ...outcome }),
      ),
      ...(each.comparison?.changes ?? []).map((change) =>
        row({
          kind: "change",
          version: each.version,
          compared_with: each.comparison?.against,
          ...change,
        }),
      ),
      ...each.verifications.map((verification) =>
        row({ kind: "verification", version: each.version, ...verification }),
      ),
    ]),
  ];
  return toCsv(CSV_HEADER, rows);
}
