import type { ConfigDocument } from "@/api/types";
import { Facts } from "@/components/Facts";
import { formatCount } from "@/lib/format";
import styles from "@/styles/layout.module.css";
import config from "./config.module.css";
import {
  compareDocuments,
  type DocumentComparison,
  type FieldChange,
  hasDifferences,
  type ItemChange,
} from "./documentCompare";

/** Counts of what a configuration holds. */
export function DocumentFacts({ document }: { document: ConfigDocument }) {
  const bridges = document.channels.filter((channel) => channel.kind === "bridge").length;
  const messages = document.channels.length - bridges;
  const { alertThresholds } = document;
  return (
    <Facts
      layout="compact"
      items={[
        { label: "Endpoints", value: formatCount(document.endpoints.length) },
        {
          label: "Channels",
          value: `${formatCount(document.channels.length)} (${bridges} bridge, ${messages} message)`,
        },
        {
          label: "Alert thresholds",
          value: `endpoint down ${alertThresholds.endpointDownSeconds} s, queue warning ${formatCount(alertThresholds.queueDepthWarning)}, buffer ${alertThresholds.bufferPressurePercent}%, disk free ${formatCount(alertThresholds.diskFreeMinMib)} MiB`,
        },
      ]}
    />
  );
}

const KIND_TEXT = {
  added: "Added",
  removed: "Removed",
  changed: "Changed",
} as const;

function countOf(changes: readonly ItemChange[], kind: ItemChange["kind"]): number {
  return changes.filter((each) => each.kind === kind).length;
}

/** "1 changed, 1 added" for one collection; empty when it did not change. */
function collectionText(changes: readonly ItemChange[]): string {
  return (["changed", "added", "removed"] as const)
    .map((kind) => [kind, countOf(changes, kind)] as const)
    .filter(([, count]) => count > 0)
    .map(([kind, count]) => `${count} ${kind}`)
    .join(", ");
}

/**
 * What a comparison holds, one line per collection, for the rollback confirmation:
 * "Endpoints: 1 changed", "Channels: 1 added, 1 removed", "Alert thresholds differ".
 */
export function describeComparison(comparison: DocumentComparison): string[] {
  return [
    comparison.endpoints.length > 0 && `Endpoints: ${collectionText(comparison.endpoints)}`,
    comparison.channels.length > 0 && `Channels: ${collectionText(comparison.channels)}`,
    comparison.thresholds.length > 0 && "Alert thresholds differ",
  ].filter((line): line is string => typeof line === "string");
}

/** `field: old → new`, with the members of an array summary behind a disclosure. */
function ChangeLine({ change }: { change: FieldChange }) {
  return (
    <li className={config.change}>
      <span>
        {change.label}: {change.before} → {change.after}
      </span>
      {change.detail && change.detail.length > 0 && (
        <details className={config.changeDetail}>
          <summary>Show the {change.detail.length === 1 ? "change" : "changes"}</summary>
          <ChangeList changes={change.detail} />
        </details>
      )}
    </li>
  );
}

function ChangeList({ changes }: { changes: readonly FieldChange[] }) {
  return (
    <ul className={styles.list}>
      {changes.map((change) => (
        <ChangeLine key={change.label} change={change} />
      ))}
    </ul>
  );
}

function ItemChanges({
  title,
  version,
  changes,
}: {
  title: string;
  version: number;
  changes: readonly ItemChange[];
}) {
  if (changes.length === 0) return null;
  const where = {
    added: `only in version ${version}`,
    removed: `not in version ${version}`,
    changed: "",
  };
  return (
    <section aria-label={`${title} that differ`} className={styles.stack}>
      <h4 className={styles.subheading}>
        {title} ({collectionText(changes)})
      </h4>
      <ul className={styles.list}>
        {changes.map((item) => (
          <li key={item.id}>
            {KIND_TEXT[item.kind]}: {item.label} <span className={styles.code}>{item.id}</span>
            {where[item.kind] && <span className={styles.hint}> ({where[item.kind]})</span>}
            {item.fields.length > 0 && <ChangeList changes={item.fields} />}
          </li>
        ))}
      </ul>
    </section>
  );
}

/**
 * How a version differs from the active configuration: each endpoint and channel that rolling
 * back would add, remove or change, and the changed fields as `field: current → in version`.
 */
export function VersionDiff({
  active,
  version,
  number,
}: {
  active: ConfigDocument;
  version: ConfigDocument;
  number: number;
}) {
  const comparison = compareDocuments(active, version);
  if (!hasDifferences(comparison)) {
    return <p>This version holds the same configuration as the active version.</p>;
  }
  return (
    <div className={styles.stack}>
      <p className={styles.hint}>
        Each line reads: field, the active value (version {active.version}), then the value in
        version {number}. Rolling back would make the active configuration match version {number}.
      </p>
      <ItemChanges title="Endpoints" version={number} changes={comparison.endpoints} />
      <ItemChanges title="Channels" version={number} changes={comparison.channels} />
      {comparison.thresholds.length > 0 && (
        <section aria-label="Alert thresholds that differ" className={styles.stack}>
          <h4 className={styles.subheading}>Alert thresholds</h4>
          <ChangeList changes={comparison.thresholds} />
        </section>
      )}
    </div>
  );
}
