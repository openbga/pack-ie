import type { Alert, AlertKind, AlertSeverity } from "../types";
import { HOUR, iso, MINUTE } from "./util";

/** What the rules say is wrong right now; the store turns it into open and cleared alerts. */
export interface AlertCondition {
  key: string;
  kind: AlertKind;
  severity: AlertSeverity;
  title: string;
  detail: string;
  channelId: string | null;
  endpointId: string | null;
  nextAction: string;
  sinceMs?: number;
}

export interface AlertChange {
  opened: Alert[];
  cleared: Alert[];
}

const SEVERITY_ORDER: Record<AlertSeverity, number> = { critical: 0, warning: 1, info: 2 };
const HISTORY_LIMIT = 300;
/** A condition that persists is raised again this often; each time counts as an occurrence. */
const RAISE_MS = 30_000;

export class AlertStore {
  private readonly open = new Map<string, Alert>();
  /** Cleared alerts, newest first. */
  private history: Alert[] = [];
  private nextId = 1;
  /** When each open condition was last raised, by key. */
  private readonly raised = new Map<string, number>();

  newId(): string {
    const id = `al-${String(this.nextId).padStart(4, "0")}`;
    this.nextId += 1;
    return id;
  }

  /** Adds already-cleared alerts (the history the mock starts with). */
  seedHistory(alerts: Alert[]): void {
    this.history = [...alerts].sort((a, b) => b.lastSeenAt.localeCompare(a.lastSeenAt));
  }

  isOpen(key: string): boolean {
    return this.open.has(key);
  }

  reconcile(conditions: AlertCondition[], nowMs: number): AlertChange {
    const change: AlertChange = { opened: [], cleared: [] };
    const seen = new Set<string>();
    for (const condition of conditions) {
      seen.add(condition.key);
      const existing = this.open.get(condition.key);
      if (existing === undefined) change.opened.push(this.openAlert(condition, nowMs));
      else this.refresh(existing, condition, nowMs);
    }
    for (const [key, alert] of [...this.open]) {
      if (seen.has(key)) continue;
      this.open.delete(key);
      this.raised.delete(key);
      alert.state = "cleared";
      alert.clearedAt = iso(nowMs);
      this.history.unshift(alert);
      change.cleared.push(alert);
    }
    if (this.history.length > HISTORY_LIMIT) this.history.length = HISTORY_LIMIT;
    return change;
  }

  private openAlert(condition: AlertCondition, nowMs: number): Alert {
    const sinceMs = condition.sinceMs ?? nowMs;
    const missed = Math.max(0, Math.floor((nowMs - sinceMs) / RAISE_MS));
    this.raised.set(condition.key, sinceMs + missed * RAISE_MS);
    const since = iso(sinceMs);
    const alert: Alert = {
      id: this.newId(),
      severity: condition.severity,
      kind: condition.kind,
      title: condition.title,
      detail: condition.detail,
      channelId: condition.channelId,
      endpointId: condition.endpointId,
      firstSeenAt: since,
      lastSeenAt: iso(nowMs),
      occurrences: 1 + missed,
      state: "open",
      clearedAt: null,
      nextAction: condition.nextAction,
    };
    this.open.set(condition.key, alert);
    return alert;
  }

  private refresh(alert: Alert, condition: AlertCondition, nowMs: number): void {
    const { key } = condition;
    const missed = Math.floor((nowMs - (this.raised.get(key) ?? nowMs)) / RAISE_MS);
    if (missed > 0) {
      alert.occurrences = (alert.occurrences ?? 1) + missed;
      this.raised.set(key, (this.raised.get(key) ?? nowMs) + missed * RAISE_MS);
    }
    alert.lastSeenAt = iso(nowMs);
    alert.severity = condition.severity;
    alert.detail = condition.detail;
    alert.title = condition.title;
    alert.nextAction = condition.nextAction;
  }

  /** Open alerts, most severe first, then most recently seen first. */
  openAlerts(): Alert[] {
    return [...this.open.values()]
      .sort(
        (a, b) =>
          SEVERITY_ORDER[a.severity] - SEVERITY_ORDER[b.severity] ||
          b.lastSeenAt.localeCompare(a.lastSeenAt),
      )
      .map((alert) => ({ ...alert }));
  }

  list(state: "open" | "cleared" | "all"): Alert[] {
    if (state === "open") return this.openAlerts();
    const cleared = this.history.map((alert) => ({ ...alert }));
    return state === "cleared" ? cleared : [...this.openAlerts(), ...cleared];
  }
}

interface HistoryTemplate {
  kind: AlertKind;
  severity: AlertSeverity;
  channelId: string | null;
  endpointId: string | null;
  title: string;
  detail: string;
  nextAction: string;
  agoMs: number;
  durationMs: number;
}

const HISTORY: HistoryTemplate[] = [
  {
    kind: "endpoint_down",
    severity: "critical",
    channelId: "ch-msg-xn1000",
    endpointId: "ep-xn1000",
    title: "Sysmex XN-1000 is down",
    detail: "peer connection lost: connection reset",
    nextAction: "Check the instrument's network cable. The listener accepts it again by itself.",
    agoMs: 5 * HOUR,
    durationMs: 12 * MINUTE,
  },
  {
    kind: "delivery_needs_attention",
    severity: "warning",
    channelId: "ch-msg-c311",
    endpointId: null,
    title: "cobas c 311 results: deliveries failing",
    detail: "7 deliveries timed out waiting for an acknowledgement",
    nextAction:
      "Open Messages filtered by needs attention, check the LIS, then resend or close them.",
    agoMs: 9 * HOUR,
    durationMs: 35 * MINUTE,
  },
  {
    kind: "delivery_backlog",
    severity: "warning",
    channelId: "ch-msg-au680",
    endpointId: null,
    title: "AU680 results: queue is filling",
    detail: "Queue depth reached 84% of the alert threshold",
    nextAction:
      "Check that the LIS HL7 destination is acknowledging; the queue drains once it does.",
    agoMs: 21 * HOUR,
    durationMs: 52 * MINUTE,
  },
  {
    kind: "disk_low",
    severity: "warning",
    channelId: null,
    endpointId: null,
    title: "Disk space is low",
    detail: "Free space on the data drive fell below the alert threshold",
    nextAction: "Free disk space or lower message retention under Channels, Retention.",
    agoMs: 30 * HOUR,
    durationMs: 6 * HOUR,
  },
  {
    kind: "no_peer_connected",
    severity: "critical",
    channelId: "ch-msg-i1000",
    endpointId: null,
    title: "Architect i1000SR results is degraded",
    detail: "LIS answered AR (application reject) for 5 results in a row",
    nextAction: "Open the rejected messages, correct the order data in the LIS, then resend them.",
    agoMs: 2 * 24 * HOUR,
    durationMs: 70 * MINUTE,
  },
  {
    kind: "endpoint_down",
    severity: "warning",
    channelId: "ch-br-v5600",
    endpointId: "ep-mw-5",
    title: "Middleware feed 5 is reconnecting",
    detail: "connect timeout after 5000 ms (10.0.4.12:3005)",
    nextAction: "Check that the middleware server is reachable. The channel keeps retrying.",
    agoMs: 2 * 24 * HOUR + 4 * HOUR,
    durationMs: 9 * MINUTE,
  },
  {
    kind: "delivery_backlog",
    severity: "info",
    channelId: "ch-msg-xn1000",
    endpointId: null,
    title: "Sysmex XN-1000 results: queue above 60%",
    detail: "Queue depth reached 61% of the alert threshold",
    nextAction: "No action needed unless the queue keeps growing.",
    agoMs: 3 * 24 * HOUR,
    durationMs: 14 * MINUTE,
  },
  {
    kind: "bridge_buffer_pressure",
    severity: "warning",
    channelId: "ch-br-c503",
    endpointId: "ep-mw-2",
    title: "cobas c 503 to middleware: buffer is filling",
    detail: "A buffer was 86% full while the middleware feed was reconnecting",
    nextAction: "Check the receiving side of the bridge; the buffer drains once it is connected.",
    agoMs: 2 * 24 * HOUR + 9 * HOUR,
    durationMs: 11 * MINUTE,
  },
  {
    kind: "delivery_needs_attention",
    severity: "critical",
    channelId: "ch-msg-reflab",
    endpointId: null,
    title: "Reference lab results: deliveries failing",
    detail: "26 deliveries failed: SFTP authentication failed",
    nextAction:
      "Re-enter the SFTP password under Settings, Secrets, then resend the failed deliveries.",
    agoMs: 3 * 24 * HOUR + 8 * HOUR,
    durationMs: 3 * HOUR,
  },
  {
    kind: "endpoint_down",
    severity: "critical",
    channelId: "ch-br-c503",
    endpointId: "ep-a-c503",
    title: "cobas c 503 (COM5) is down",
    detail: "COM5: access denied (port in use by another program)",
    nextAction: "Close the other program that opened the port; the channel recovers by itself.",
    agoMs: 4 * 24 * HOUR,
    durationMs: 25 * MINUTE,
  },
  {
    kind: "no_peer_connected",
    severity: "warning",
    channelId: "ch-msg-sta",
    endpointId: null,
    title: "STA Compact results is degraded",
    detail: "Standby LIS destination did not acknowledge",
    nextAction: "Check the standby LIS interface, or remove the destination.",
    agoMs: 5 * 24 * HOUR,
    durationMs: 2 * HOUR,
  },
  {
    kind: "endpoint_down",
    severity: "warning",
    channelId: "ch-msg-dashboard",
    endpointId: "ep-dashboard",
    title: "Lab dashboard feed is reconnecting",
    detail: "connection reset by peer (10.0.5.20:8081)",
    nextAction: "No action needed unless this persists; check the dashboard service.",
    agoMs: 6 * 24 * HOUR,
    durationMs: 4 * MINUTE,
  },
  {
    kind: "disk_low",
    severity: "warning",
    channelId: null,
    endpointId: null,
    title: "Disk space is low",
    detail: "Free space on the data drive fell below the alert threshold",
    nextAction: "Free disk space or lower message retention under Channels, Retention.",
    agoMs: 6 * 24 * HOUR + 7 * HOUR,
    durationMs: 11 * HOUR,
  },
  {
    kind: "delivery_needs_attention",
    severity: "warning",
    channelId: "ch-msg-e411",
    endpointId: null,
    title: "cobas e 411 results: deliveries failing",
    detail: "6 deliveries were rejected: unknown patient",
    nextAction:
      "Open Messages filtered by needs attention, fix the patient in the LIS, then resend.",
    agoMs: 7 * 24 * HOUR,
    durationMs: 95 * MINUTE,
  },
  {
    kind: "endpoint_down",
    severity: "critical",
    channelId: "ch-msg-poc",
    endpointId: "ep-poc-in",
    title: "Point-of-care export folder is down",
    detail: "cannot read folder D:\\LabData\\POC\\Incoming: access denied",
    nextAction: "Check the permissions of the service account on this folder.",
    agoMs: 8 * 24 * HOUR,
    durationMs: 40 * MINUTE,
  },
];

/** Cleared alerts spread over the last days, so the Alerts page has a history to show. */
export function buildAlertHistory(store: AlertStore, nowMs: number): Alert[] {
  return HISTORY.map((template) => {
    const first = nowMs - template.agoMs;
    return {
      id: store.newId(),
      severity: template.severity,
      kind: template.kind,
      title: template.title,
      detail: template.detail,
      channelId: template.channelId,
      endpointId: template.endpointId,
      firstSeenAt: iso(first),
      lastSeenAt: iso(first + template.durationMs),
      state: "cleared",
      clearedAt: iso(first + template.durationMs),
      occurrences: Math.max(1, Math.round(template.durationMs / (5 * MINUTE))),
      nextAction: template.nextAction,
    } satisfies Alert;
  });
}
