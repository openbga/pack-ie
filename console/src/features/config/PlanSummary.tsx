import type { ApplyPlan, PlanChange, PlanItem } from "@/api/types";
import styles from "@/styles/layout.module.css";

const labelsOf = (items: readonly PlanItem[], change: PlanChange) =>
  items.filter((item) => item.change === change).map((item) => item.label);

/** Whether applying removes a channel, which closes its unresolved deliveries. */
export const removesChannels = (plan: ApplyPlan) =>
  plan.channels.some((item) => item.change === "removed");

/**
 * What applying does, in operator terms: which channels restart, update in place, start,
 * stop, or are left untouched, then the endpoint and threshold changes. An endpoint change
 * shows here too, as the restart of the channels that use it.
 */
export function PlanSummary({ plan }: { plan: ApplyPlan }) {
  const { channels, endpoints } = plan;
  const untouched = labelsOf(channels, "unchanged").length;
  const groups: [key: string, title: string, labels: string[]][] = [
    ["restarted", "Restarted (changed)", labelsOf(channels, "changed")],
    ["updated", "Updated in place", labelsOf(channels, "updated")],
    ["started", "Started (new)", labelsOf(channels, "added")],
    ["stopped", "Stopped (disabled)", labelsOf(channels, "stopped")],
    ["removed", "Stopped and removed", labelsOf(channels, "removed")],
    ["endpoints-added", "Endpoints added", labelsOf(endpoints, "added")],
    ["endpoints-changed", "Endpoints changed", labelsOf(endpoints, "changed")],
    ["endpoints-updated", "Endpoints updated", labelsOf(endpoints, "updated")],
    ["endpoints-removed", "Endpoints removed", labelsOf(endpoints, "removed")],
  ];
  const restarts = labelsOf(channels, "changed").length;
  return (
    <ul className={styles.list} aria-label="Plan">
      {groups
        .filter(([, , labels]) => labels.length > 0)
        .map(([key, title, labels]) => (
          <li key={key}>
            {title}: {labels.join(", ")}
          </li>
        ))}
      {plan.alertsChanged && <li>Alert thresholds change.</li>}
      {restarts === 0 && <li>No channel is restarted.</li>}
      <li>
        Untouched: {untouched} {untouched === 1 ? "channel" : "channels"}, which keep running.
      </li>
    </ul>
  );
}
