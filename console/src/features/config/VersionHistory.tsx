import { useMemo } from "react";
import { useSearchParams } from "react-router";
import { useEngineQuery } from "@/api/hooks";
import type { ConfigVersionSummary } from "@/api/types";
import { DataGrid, type DataGridColumn } from "@/components/DataGrid";
import { Facts } from "@/components/Facts";
import { QueryContent } from "@/components/QueryContent";
import { Section } from "@/components/Section";
import { StatusBadge } from "@/components/StatusBadge";
import { Timestamp } from "@/components/Timestamp";
import styles from "@/styles/layout.module.css";
import { ExportChangeReport } from "./ExportChangeReport";
import { channelNamer, summarizeOutcomes } from "./outcomes";
import { RollbackAction } from "./RollbackAction";
import { VerifyVersion } from "./VerifyVersion";
import { DocumentFacts, VersionDiff } from "./VersionDiff";
import { VersionDocument } from "./VersionDocument";
import { VersionOutcomes } from "./VersionOutcomes";

/** "Verified" with the number of sign-offs, or the plain words for none. */
function VerifiedCell({ row }: { row: ConfigVersionSummary }) {
  if (row.verifications.length === 0) return "Not verified since change";
  const count = row.verifications.length;
  return (
    <StatusBadge tone="success">{count === 1 ? "Verified" : `Verified (${count})`}</StatusBadge>
  );
}

/** The per-channel outcomes in a line; a failure is flagged in its own tone. */
function OutcomesCell({ row }: { row: ConfigVersionSummary }) {
  const summary = summarizeOutcomes(row.outcomes);
  return row.outcomes.some((each) => each.outcome === "failed") ? (
    <StatusBadge tone="danger">{summary}</StatusBadge>
  ) : (
    summary
  );
}

const COLUMNS: DataGridColumn<ConfigVersionSummary>[] = [
  {
    id: "version",
    header: "Version",
    identifier: true,
    width: 110,
    cell: (row) => `Version ${row.version}`,
    sortValue: (row) => row.version,
    defaultSort: "desc",
  },
  {
    id: "appliedAt",
    header: "Applied",
    width: 200,
    cell: (row) => <Timestamp value={row.appliedAt} />,
    sortValue: (row) => row.appliedAt,
  },
  {
    id: "appliedBy",
    header: "Applied by",
    width: 160,
    cell: (row) => row.appliedBy ?? "Not recorded",
    sortValue: (row) => row.appliedBy ?? "",
  },
  {
    id: "note",
    header: "Note",
    wrap: true,
    cell: (row) => row.note ?? "No note",
  },
  {
    id: "outcomes",
    header: "Channel outcomes",
    width: 230,
    wrap: true,
    cell: (row) => <OutcomesCell row={row} />,
  },
  {
    id: "verified",
    header: "Verified",
    width: 190,
    cell: (row) => <VerifiedCell row={row} />,
  },
  {
    id: "current",
    header: "State",
    width: 130,
    cell: (row) => (
      <StatusBadge tone="neutral">{row.current ? "Current" : "Superseded"}</StatusBadge>
    ),
  },
];

/** Each sign-off: who, when and their reference; or the plain words for none. */
function Verifications({ summary }: { summary: ConfigVersionSummary }) {
  if (summary.verifications.length === 0) {
    return <p>Not verified since the change.</p>;
  }
  return (
    <ul className={styles.list} aria-label="Verifications">
      {summary.verifications.map((each) => (
        <li key={`${each.at}|${each.by}`}>
          {each.by}, <Timestamp value={each.at} seconds></Timestamp>: {each.note}
        </li>
      ))}
    </ul>
  );
}

function VersionDetail({ summary }: { summary: ConfigVersionSummary }) {
  const target = useEngineQuery(["getConfigVersion", summary.version], (api) =>
    api.getConfigVersion(summary.version),
  );
  const current = useEngineQuery(["getConfig"], (api) => api.getConfig());
  // A channel this version removed is in neither document: its label is in the one before.
  const previous = useEngineQuery(
    ["getConfigVersion", summary.version - 1],
    (api) => api.getConfigVersion(summary.version - 1),
    { enabled: summary.version > 1 && summary.outcomes.some((each) => each.outcome === "removed") },
  );
  const nameOf = channelNamer(
    target.data?.channels,
    current.data?.channels,
    previous.data?.channels,
  );
  return (
    <Section title={`Version ${summary.version}`}>
      <Facts
        layout="compact"
        items={[
          { label: "Applied", value: <Timestamp value={summary.appliedAt} seconds /> },
          { label: "Applied by", value: summary.appliedBy ?? "Not recorded" },
          { label: "Note", value: summary.note ?? "No note" },
          { label: "State", value: summary.current ? "Current" : "Superseded" },
        ]}
      />
      <h3 className={styles.subheading}>Channel outcomes</h3>
      <VersionOutcomes outcomes={summary.outcomes} isActive={summary.current} nameOf={nameOf} />
      <h3 className={styles.subheading}>Verification</h3>
      <Verifications summary={summary} />
      <div>
        <VerifyVersion version={summary.version} />
      </div>
      <QueryContent query={target} what={`version ${summary.version}`}>
        {(document) => (
          <div className={styles.stack}>
            <DocumentFacts document={document} />
            <h3 className={styles.subheading}>Configuration in this version (read-only)</h3>
            <VersionDocument document={document} />
            {summary.current ? (
              <p className={styles.hint}>This is the configuration the engine is running.</p>
            ) : (
              <QueryContent query={current} what="current configuration">
                {(running) => (
                  <>
                    <h3 className={styles.subheading}>
                      Compared with the active version ({running.version})
                    </h3>
                    <VersionDiff active={running} version={document} number={summary.version} />
                    <div>
                      <RollbackAction
                        version={summary.version}
                        current={running}
                        target={document}
                      />
                    </div>
                  </>
                )}
              </QueryContent>
            )}
          </div>
        )}
      </QueryContent>
    </Section>
  );
}

/**
 * The configuration history: every applied version, newest first. Choosing one (a click or
 * Enter; `?version=` in the URL) shows what it holds and how it differs from the current
 * version, with a roll back that creates a new version.
 */
export function VersionHistory() {
  const versions = useEngineQuery(["listConfigVersions"], (api) => api.listConfigVersions());
  const [params, setParams] = useSearchParams();
  const requested = Number(params.get("version"));
  const selected = useMemo(
    () => versions.data?.find((each) => each.version === requested),
    [versions.data, requested],
  );
  return (
    <QueryContent
      query={versions}
      what="version history"
      loadingRows={4}
      empty={{
        when: (data) => data.length === 0,
        title: "No versions yet",
        description: "A version is recorded each time a configuration is applied.",
      }}
    >
      {(data) => (
        <div className={styles.stack}>
          <Section
            title="Applied versions"
            actions={<ExportChangeReport versions={data} selected={selected?.version} />}
          >
            <DataGrid
              label="Configuration versions"
              columns={COLUMNS}
              rows={data}
              getRowId={(row) => String(row.version)}
              openRowId={selected ? String(selected.version) : null}
              onRowAction={(row) => setParams({ version: String(row.version) })}
            />
          </Section>
          {selected && <VersionDetail key={selected.version} summary={selected} />}
        </div>
      )}
    </QueryContent>
  );
}
