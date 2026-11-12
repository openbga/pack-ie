import { CircleCheck, RotateCw, X } from "lucide-react";
import { Link } from "react-router";
import { useConnection } from "@/api/hooks";
import type {
  ChannelState,
  ConnectionStatus,
  NodeSnapshot,
  Snapshot,
  StoreStatus,
} from "@/api/types";
import { Facts } from "@/components/Facts";
import { StatusBadge } from "@/components/StatusBadge";
import { formatCount, formatDuration } from "@/lib/format";
import { storeSummary } from "@/lib/store";
import { ShapeChip } from "../shared-operate/ShapeChip";
import { channelShape, type Shape } from "../shared-operate/stateShapes";
import { useNow } from "../shared-operate/useNow";
import styles from "./HeaderStrip.module.css";

const STATES: readonly { state: ChannelState; label: string }[] = [
  { state: "running", label: "Running" },
  { state: "starting", label: "Starting" },
  { state: "retrying", label: "Retrying" },
  { state: "degraded", label: "Degraded" },
  { state: "stopped", label: "Stopped" },
];

function ChannelCounts({ snapshot }: { snapshot: Snapshot }) {
  if (snapshot.channels.length === 0) return <span className={styles.muted}>No channels</span>;
  return (
    <ul className={styles.chips} aria-label="Channels by state">
      {STATES.map(({ state, label }) => {
        const count = snapshot.channels.filter((channel) => channel.state === state).length;
        if (count === 0) return null;
        const shape = channelShape({ state, stopReason: null });
        return (
          <li key={state}>
            <ShapeChip shape={state === "stopped" ? { ...shape, label } : shape}>
              {`${count} ${label}`}
            </ShapeChip>
          </li>
        );
      })}
    </ul>
  );
}

function engineShape(state: NodeSnapshot["state"]): Shape {
  switch (state) {
    case "running":
      return { tone: "neutral", label: "Running", Icon: CircleCheck };
    case "degraded":
      return { tone: "warning", label: "Needs attention", Icon: RotateCw };
    case "stopping":
      return { tone: "info", label: "Stopping", Icon: RotateCw };
  }
}

function AlertCounts({ snapshot }: { snapshot: Snapshot }) {
  const open = snapshot.openAlerts.filter((alert) => alert.state === "open");
  const critical =
    snapshot.node.totals?.alertsCritical ?? open.filter((a) => a.severity === "critical").length;
  const warning =
    snapshot.node.totals?.alertsWarning ?? open.filter((a) => a.severity === "warning").length;
  if (critical === 0 && warning === 0) return <Link to="/alerts">None open</Link>;
  return (
    <span className={styles.chips}>
      {critical > 0 && <StatusBadge tone="danger">{`${critical} critical`}</StatusBadge>}
      {warning > 0 && <StatusBadge tone="warning">{`${warning} warning`}</StatusBadge>}
      <Link to="/alerts?state=open">View alerts</Link>
    </span>
  );
}

function Unresolved({ snapshot }: { snapshot: Snapshot }) {
  const count =
    snapshot.node.totals?.deliveriesUnresolved ??
    snapshot.channels.reduce((sum, channel) => sum + (channel.queue?.needsAttention ?? 0), 0);
  if (count === 0) return <>None</>;
  return (
    <span className={styles.chips}>
      <StatusBadge tone="warning">{`${formatCount(count)} need attention`}</StatusBadge>
      <Link to="/messages?quick=attention&range=any">View in Messages</Link>
    </span>
  );
}

function storeFacts(store: StoreStatus) {
  const summary = storeSummary(store);
  const figures =
    store.state === "ok" && store.messageCount !== null && store.databaseBytes !== null;
  return [
    {
      label: "Store",
      value: figures ? `${summary.messages} messages · ${summary.size}` : summary.state,
    },
    { label: "Disk free", value: summary.disk },
  ];
}

const CONNECTION_TEXT: Record<ConnectionStatus["state"], Shape> = {
  connected: { tone: "neutral", label: "Connected", Icon: CircleCheck },
  reconnecting: { tone: "warning", label: "Reconnecting", Icon: RotateCw },
  disconnected: { tone: "danger", label: "Disconnected", Icon: X },
};

function Connection({ status }: { status: ConnectionStatus }) {
  return (
    <span className={styles.chips}>
      <ShapeChip shape={CONNECTION_TEXT[status.state]} />
      {status.state === "reconnecting" && (
        <span className={styles.muted}>The last known state is shown dimmed.</span>
      )}
      {status.state !== "connected" && status.detail !== null && (
        <span className={styles.muted}>{status.detail}</span>
      )}
    </span>
  );
}

function ConfigVersion({ node }: { node: NodeSnapshot }) {
  return (
    <>
      Version {node.configVersion}
      {node.pendingConfigVersion !== null && (
        <>
          {" "}
          <StatusBadge tone="info">{`Applying version ${node.pendingConfigVersion}`}</StatusBadge>
        </>
      )}
    </>
  );
}

/**
 * The header strip (operator-console.md section 3): the engine's state, version, uptime and
 * configuration (and the pending one during an apply), channels by state, open alerts by
 * severity, unresolved deliveries, store and free disk when the engine reports them, and the
 * connection to the engine.
 */
export function HeaderStrip({ snapshot }: { snapshot: Snapshot }) {
  const connection = useConnection();
  const now = useNow();
  const { node } = snapshot;
  const uptime = Math.max(0, (now - Date.parse(node.startedAt)) / 1000);
  return (
    <section aria-label="Engine" className={styles.strip}>
      <Facts
        layout="compact"
        items={[
          { label: "Engine", value: <ShapeChip shape={engineShape(node.state)} /> },
          { label: "Node", value: node.nodeName },
          { label: "Version", value: node.version },
          { label: "Uptime", value: formatDuration(uptime) },
          { label: "Configuration", value: <ConfigVersion node={node} /> },
          { label: "Channels", value: <ChannelCounts snapshot={snapshot} /> },
          { label: "Alerts", value: <AlertCounts snapshot={snapshot} /> },
          { label: "Unresolved deliveries", value: <Unresolved snapshot={snapshot} /> },
          ...(node.store === null ? [] : storeFacts(node.store)),
          ...(connection.data === undefined
            ? []
            : [{ label: "Connection", value: <Connection status={connection.data} /> }]),
        ]}
      />
    </section>
  );
}
