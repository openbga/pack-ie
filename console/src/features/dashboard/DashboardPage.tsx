import { useMemo } from "react";
import { Link } from "react-router";
import { useConnection, useSnapshot } from "@/api/hooks";
import type { Snapshot } from "@/api/types";
import { Alert } from "@/components/Alert";
import { Button } from "@/components/Button";
import { EmptyState } from "@/components/EmptyState";
import { Page, PageHeader } from "@/components/Page";
import { QueryContent } from "@/components/QueryContent";
import { useFormatTime } from "@/lib/format";
import { AlertsPreview } from "./AlertsPreview";
import { ChannelTable } from "./ChannelTable";
import styles from "./Dashboard.module.css";
import { EndpointsSummary } from "./EndpointsSummary";
import { HeaderStrip } from "./HeaderStrip";
import { rankChannels } from "./rank";

/**
 * The dashboard (CON-02): the engine's header strip, then one channel table in the spec's
 * rank order (a channel that is failing, retrying or has a critical alert comes first), then
 * compact endpoint and alert summaries. It follows the engine's snapshot, which updates every
 * second; the last known state is dimmed while the engine connection is being re-established.
 */
export function DashboardPage() {
  const snapshot = useSnapshot();
  const formatTime = useFormatTime();
  const updated =
    snapshot.dataUpdatedAt > 0
      ? formatTime(new Date(snapshot.dataUpdatedAt).toISOString(), { today: new Date() })
      : null;
  return (
    <Page>
      <PageHeader
        title="Dashboard"
        actions={updated && <span className={styles.updated}>Updated {updated}</span>}
      />
      <QueryContent query={snapshot} what="dashboard" loadingRows={10}>
        {(data) => <DashboardBody snapshot={data} />}
      </QueryContent>
    </Page>
  );
}

function DashboardBody({ snapshot }: { snapshot: Snapshot }) {
  const connection = useConnection();
  const rows = useMemo(() => rankChannels(snapshot), [snapshot]);
  const names = new Map([
    ...snapshot.channels.map((channel) => [channel.id, channel.label] as const),
    ...snapshot.endpoints.map((endpoint) => [endpoint.id, endpoint.label] as const),
  ]);
  const unhealthy = snapshot.endpoints.filter(
    (endpoint) => endpoint.state === "error" || endpoint.state === "retrying",
  );
  return (
    <div
      className={styles.body}
      data-stale={connection.data?.state === "reconnecting" || undefined}
    >
      <HeaderStrip snapshot={snapshot} />
      {snapshot.openAlerts.some((alert) => alert.kind === "audit_unavailable") && (
        <Alert tone="danger" title="The audit log cannot be written">
          Configuration changes, channel commands, resends and views of message content are refused
          until it can be written again. Message traffic continues.{" "}
          <Link to="/audit">Open the audit log</Link>
        </Alert>
      )}
      {rows.length === 0 ? (
        <EmptyState
          title="No channels yet"
          description="A channel connects an instrument or system to another. Add one to start moving data."
          action={
            <Button href="/channels/new" variant="primary">
              Add channel
            </Button>
          }
        />
      ) : (
        <ChannelTable rows={rows} />
      )}
      <div className={styles.columns}>
        <AlertsPreview alerts={snapshot.openAlerts} names={names} />
        <EndpointsSummary endpoints={snapshot.endpoints} unhealthy={unhealthy} />
      </div>
    </div>
  );
}
