import { useMemo } from "react";
import { useNavigate } from "react-router";
import { useEngineQuery, useSnapshot } from "@/api/hooks";
import type { Alert, AlertSeverity, Snapshot } from "@/api/types";
import { RefreshControl } from "@/components/RefreshControl";
import { Select } from "@/components/Select";
import { enginePageOf } from "../shared-operate/alertTarget";
import { Directory } from "../shared-operate/Directory";
import { useChoiceParam, useClearParams } from "../shared-operate/searchParam";
import { type AlertRow, alertColumnsFor, SEVERITY_RANK } from "./alertColumns";

type AlertScope = "open" | "cleared" | "all";

const SCOPES = ["open", "cleared", "all"] as const satisfies readonly AlertScope[];
const SEVERITIES = ["critical", "warning", "info"] as const satisfies readonly AlertSeverity[];
const SCOPE_OPTIONS = [
  { id: "open", label: "Open" },
  { id: "cleared", label: "Cleared" },
  { id: "all", label: "All" },
];
const SEVERITY_OPTIONS = [
  { id: "all", label: "All severities" },
  { id: "critical", label: "Critical" },
  { id: "warning", label: "Warning" },
  { id: "info", label: "Info" },
];
const FILTER_PARAMS = ["state", "severity"] as const;

const EMPTY_COPY: Record<AlertScope, { title: string; description: string }> = {
  open: {
    title: "No open alerts",
    description:
      "Everything the engine watches is within its limits. An alert appears here with the next step when a condition needs someone.",
  },
  cleared: {
    title: "No cleared alerts",
    description: "Alerts that cleared by themselves are listed here.",
  },
  all: {
    title: "No alerts",
    description: "The engine has not raised any alert.",
  },
};

/**
 * Open alerts first (critical, then warning, then first seen), then the ones that cleared,
 * the most recently cleared first (operator-console.md section 9).
 */
function sorted(alerts: Alert[]): readonly Alert[] {
  const time = (value: string | null) => (value === null ? 0 : Date.parse(value));
  return [...alerts].sort(
    (a, b) =>
      Number(b.state === "open") - Number(a.state === "open") ||
      (a.state === "open"
        ? SEVERITY_RANK[b.severity] - SEVERITY_RANK[a.severity] ||
          time(a.firstSeenAt) - time(b.firstSeenAt)
        : time(b.clearedAt) - time(a.clearedAt)),
  );
}

/** Names the channel and endpoint an alert concerns, from the snapshot's labels. */
function withLabels(alerts: Alert[], snapshot: Snapshot | undefined): readonly AlertRow[] {
  const channels = new Map(snapshot?.channels.map((channel) => [channel.id, channel.label]));
  const endpoints = new Map(snapshot?.endpoints.map((endpoint) => [endpoint.id, endpoint.label]));
  return sorted(alerts).map((alert) => ({
    ...alert,
    channelLabel:
      alert.channelId === null ? null : (channels.get(alert.channelId) ?? alert.channelId),
    endpointLabel:
      alert.endpointId === null ? null : (endpoints.get(alert.endpointId) ?? alert.endpointId),
  }));
}

const FIND_FIELDS = (alert: AlertRow) => [
  alert.title,
  alert.detail,
  alert.channelLabel,
  alert.endpointLabel,
];

/**
 * Alerts (LOG-03): conditions that need an operator, each with its severity, what it
 * concerns, when it began and the next step. Open alerts by default; a row opens the channel
 * or endpoint it concerns. An alert clears itself when its condition clears.
 */
export function AlertsPage() {
  const navigate = useNavigate();
  const [scope, setScope] = useChoiceParam<AlertScope>("state", SCOPES);
  const [severity, setSeverity] = useChoiceParam<AlertSeverity>("severity", SEVERITIES);
  const clear = useClearParams(FILTER_PARAMS);
  const chosen = scope ?? "open";
  const alerts = useEngineQuery(["listAlerts", chosen], (api) => api.listAlerts(chosen), {
    refetchInterval: 5_000,
  });
  const snapshot = useSnapshot();
  const labelled = useMemo(
    () => (alerts.data === undefined ? undefined : withLabels(alerts.data, snapshot.data)),
    [alerts.data, snapshot.data],
  );
  const keep = useMemo(
    () => (alert: AlertRow) => severity === null || alert.severity === severity,
    [severity],
  );
  // `Directory` reads rows through a stable function; the labelled rows are the "answer".
  const rowsOf = useMemo(() => () => labelled ?? [], [labelled]);
  return (
    <Directory
      title="Alerts"
      noun={{ many: "alerts", one: "alert" }}
      query={alerts}
      rowsOf={rowsOf}
      keep={keep}
      isFiltered={scope !== null || severity !== null}
      onClearFilters={clear}
      find={{ placeholder: "Alert, channel or endpoint", fields: FIND_FIELDS }}
      filters={
        <>
          <Select
            label="State"
            layout="inline"
            name="state"
            options={SCOPE_OPTIONS}
            value={chosen}
            onChange={(value) => setScope(SCOPES.find((each) => each === value) ?? null)}
          />
          <Select
            label="Severity"
            layout="inline"
            name="severity"
            options={SEVERITY_OPTIONS}
            value={severity ?? "all"}
            onChange={(value) => setSeverity(SEVERITIES.find((each) => each === value) ?? null)}
          />
        </>
      }
      toolbarActions={<RefreshControl query={alerts} />}
      empty={EMPTY_COPY[chosen]}
      columns={alertColumnsFor}
      getRowId={(alert) => alert.id}
      onRowAction={(alert) => {
        if (alert.channelId !== null) void navigate(`/channels/${alert.channelId}`);
        else if (alert.endpointId !== null) void navigate(`/endpoints/${alert.endpointId}`);
        else {
          const page = enginePageOf(alert);
          if (page !== null) void navigate(page.to);
        }
      }}
    />
  );
}
