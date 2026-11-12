import { useSnapshot } from "@/api/hooks";
import { StatusBadge } from "@/components/StatusBadge";
import { formatAge } from "@/lib/format";
import styles from "./AppShell.module.css";

/** The top bar's one-line answer: which node, and whether it is healthy. */
export function NodeStatus() {
  const snapshot = useSnapshot();
  if (snapshot.isPending) return <span className={styles.nodeName}>Connecting…</span>;
  if (snapshot.isError) {
    return <StatusBadge tone="danger">Engine not answering</StatusBadge>;
  }
  const { node, takenAt } = snapshot.data;
  const degraded = node.state === "degraded";
  return (
    <>
      <span className={styles.nodeName} title={`Updated ${formatAge(takenAt)}`}>
        {node.nodeName}
      </span>
      <StatusBadge tone={degraded ? "warning" : "neutral"}>
        {degraded ? "Needs attention" : "Running"}
      </StatusBadge>
    </>
  );
}
