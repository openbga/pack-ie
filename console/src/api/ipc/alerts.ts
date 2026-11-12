// The engine's alerts (generated `Alert`) as the console's, most severe first.
import type { Alert as RawAlert } from "@/ipc/generated";
import type { Alert, AlertSeverity } from "../types";

const SEVERITY_ORDER: Record<AlertSeverity, number> = { critical: 0, warning: 1, info: 2 };

/** `details` (numbers and codes) as `count: 3, rejected: 2`. */
function detailText(details: RawAlert["details"]): string {
  return Object.entries(details)
    .filter(([, value]) => typeof value === "number" || typeof value === "string")
    .map(([key, value]) => `${key.replaceAll("_", " ")}: ${value}`)
    .join(", ");
}

export function alertFromIpc(alert: RawAlert): Alert {
  return {
    id: alert.alert_id,
    severity: alert.severity,
    kind: alert.code,
    title: alert.summary,
    detail: detailText(alert.details),
    channelId: alert.channel_id,
    endpointId: alert.endpoint_id,
    firstSeenAt: alert.first_seen_at,
    lastSeenAt: alert.last_seen_at,
    occurrences: alert.occurrences,
    state: alert.state === "cleared" ? "cleared" : "open",
    clearedAt: alert.cleared_at,
    nextAction: alert.next_action,
  };
}

/** Most severe first; the engine's order (newest first) otherwise. */
export function sortAlerts(alerts: Alert[]): Alert[] {
  return alerts
    .map((alert, index) => ({ alert, index }))
    .sort(
      (a, b) =>
        SEVERITY_ORDER[a.alert.severity] - SEVERITY_ORDER[b.alert.severity] || a.index - b.index,
    )
    .map(({ alert }) => alert);
}
