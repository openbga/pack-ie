import { Link } from "react-router";
import type { Alert } from "@/api/types";
import { Section } from "@/components/Section";
import { StatusBadge } from "@/components/StatusBadge";
import { formatAge } from "@/lib/format";
import { alertSeverityLabel, alertTone } from "@/lib/status";
import { enginePageOf } from "../shared-operate/alertTarget";
import styles from "./Dashboard.module.css";

const SHOWN = 5;

function target(
  alert: Alert,
  names: ReadonlyMap<string, string>,
): { to: string; label: string } | null {
  if (alert.channelId !== null) {
    return {
      to: `/channels/${alert.channelId}`,
      label: names.get(alert.channelId) ?? alert.channelId,
    };
  }
  if (alert.endpointId !== null) {
    return {
      to: `/endpoints/${alert.endpointId}`,
      label: names.get(alert.endpointId) ?? alert.endpointId,
    };
  }
  const page = enginePageOf(alert);
  return page === null ? null : { to: page.to, label: page.label };
}

/** The most severe open alerts with their next step; "View all" opens the alerts screen. */
export function AlertsPreview({
  alerts,
  names,
}: {
  alerts: readonly Alert[];
  /** Channel and endpoint labels by id. */
  names: ReadonlyMap<string, string>;
}) {
  return (
    <Section title="Open alerts">
      {alerts.length === 0 ? (
        <p className={styles.muted}>No open alerts.</p>
      ) : (
        <ul className={styles.rows} aria-label="Open alerts">
          {alerts.slice(0, SHOWN).map((alert) => {
            const concerns = target(alert, names);
            return (
              <li key={alert.id} className={styles.alertRow}>
                <span className={styles.row}>
                  <StatusBadge tone={alertTone(alert.severity)}>
                    {alertSeverityLabel(alert.severity)}
                  </StatusBadge>
                  <strong>{alert.title}</strong>
                  <span className={styles.muted}>{formatAge(alert.firstSeenAt)}</span>
                </span>
                {concerns && <Link to={concerns.to}>{concerns.label}</Link>}
                <span className={styles.rowDetail}>Next step: {alert.nextAction}</span>
              </li>
            );
          })}
        </ul>
      )}
      <Link to="/alerts">
        {alerts.length > SHOWN ? `View all (${alerts.length})` : "View alerts"}
      </Link>
    </Section>
  );
}
