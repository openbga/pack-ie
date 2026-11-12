import { Link, useParams } from "react-router";
import { useEndpoint, useSnapshot } from "@/api/hooks";
import type { Alert, ChannelSnapshot, EndpointDetail, EndpointSnapshot } from "@/api/types";
import { Button } from "@/components/Button";
import { EmptyState } from "@/components/EmptyState";
import { Facts } from "@/components/Facts";
import { type MiniColumn, MiniTable } from "@/components/MiniTable";
import { Page, PageHeader } from "@/components/Page";
import { QueryContent } from "@/components/QueryContent";
import { Section } from "@/components/Section";
import { StatusBadge } from "@/components/StatusBadge";
import { Timestamp } from "@/components/Timestamp";
import { formatDuration } from "@/lib/format";
import { alertSeverityLabel, alertTone, endpointKindLabel, endpointUrgency } from "@/lib/status";
import { ChannelStateChip } from "../shared-operate/ChannelStateChip";
import { Countdown } from "../shared-operate/Countdown";
import { CountersTable } from "../shared-operate/CountersTable";
import { activityText } from "../shared-operate/channelRules";
import { LastErrorNote } from "../shared-operate/LastErrorNote";
import { RecordNotFound } from "../shared-operate/RecordNotFound";
import { ShapeChip } from "../shared-operate/ShapeChip";
import { endpointShape } from "../shared-operate/stateShapes";
import { useNow } from "../shared-operate/useNow";
import { RestartChannels } from "./RestartChannels";
import { TestEndpoint } from "./TestEndpoint";

type UsingChannel = EndpointDetail["channels"][number];

function errorTone(endpoint: EndpointSnapshot): "danger" | "warning" | "info" {
  const urgency = endpointUrgency(endpoint.state);
  return urgency >= 3 ? "danger" : urgency === 2 ? "warning" : "info";
}

/** How long the endpoint has been in this state, as a live duration: "12 min", "3 d 4 h". */
function StateFor({ since }: { since: string }) {
  const now = useNow();
  const seconds = Math.max(0, (now - Date.parse(since)) / 1000);
  return (
    <>
      {seconds < 60 ? "Under a minute" : formatDuration(seconds)} (since{" "}
      <Timestamp value={since} seconds />)
    </>
  );
}

function endpointFacts(endpoint: EndpointSnapshot, heldBy: UsingChannel | null) {
  return [
    { label: "Endpoint id", value: endpoint.id },
    { label: "Kind", value: endpointKindLabel(endpoint.kind) },
    { label: "Points to", value: endpoint.summary },
    { label: "Enabled", value: endpoint.enabled ? "Yes" : "No" },
    { label: "State", value: <ShapeChip shape={endpointShape(endpoint.state)} /> },
    { label: "In this state for", value: <StateFor since={endpoint.stateSince} /> },
    {
      label: "Held by",
      value:
        endpoint.heldBy === null ? (
          "No channel"
        ) : (
          <Link to={`/channels/${endpoint.heldBy}`}>{heldBy?.label ?? endpoint.heldBy}</Link>
        ),
    },
    { label: "Active peer", value: endpoint.activePeer ?? "None" },
    { label: "Failure streak", value: String(endpoint.consecutiveFailureCount) },
    {
      label: "Next retry",
      value:
        endpoint.nextRetryAt === null ? (
          "None scheduled"
        ) : (
          <Countdown until={endpoint.nextRetryAt} />
        ),
    },
    {
      label: "Last activity",
      value:
        endpoint.lastActivityAt === null ? (
          activityText(null)
        ) : (
          <>
            {activityText(endpoint.lastActivityAt)} (
            <Timestamp value={endpoint.lastActivityAt} seconds />)
          </>
        ),
    },
  ];
}

/**
 * One endpoint (EPT-06; operator-console.md section 4): its settings summary, state and for
 * how long, which channel holds it and its peer, the failure streak and next retry, counters,
 * the last error with the next step, alerts and the channels that use it, with a shortcut to
 * restart them. The state follows the engine's snapshot.
 */
export function EndpointPage() {
  const { id = "" } = useParams();
  const detail = useEndpoint(id);
  if (detail.isError && detail.error.code === "not_found") {
    return <RecordNotFound what="Endpoint" listLabel="Endpoints" listHref="/endpoints" />;
  }
  return (
    <Page>
      <QueryContent query={detail} what="endpoint" pageTitle="Endpoint" loadingRows={8}>
        {(data) => <EndpointBody detail={data} />}
      </QueryContent>
    </Page>
  );
}

function EndpointBody({ detail }: { detail: EndpointDetail }) {
  const snapshot = useSnapshot();
  const endpoint =
    snapshot.data?.endpoints.find((candidate) => candidate.id === detail.snapshot.id) ??
    detail.snapshot;
  const live = snapshot.data?.channels ?? [];
  const using = live.filter((channel) => detail.channels.some((each) => each.id === channel.id));
  const alerts = (snapshot.data?.openAlerts ?? []).filter(
    (alert) => alert.endpointId === endpoint.id && alert.state === "open",
  );
  const heldBy = detail.channels.find((channel) => channel.id === endpoint.heldBy) ?? null;
  return (
    <>
      <PageHeader
        title={endpoint.label}
        breadcrumbs={[
          { label: "Endpoints", href: "/endpoints" },
          { label: endpoint.label, href: `/endpoints/${endpoint.id}` },
        ]}
        status={<ShapeChip shape={endpointShape(endpoint.state)} />}
        actions={
          <>
            <RestartChannels endpointLabel={endpoint.label} channels={using} />
            <Button href={`/endpoints/${endpoint.id}/edit`}>Edit</Button>
          </>
        }
      />
      {endpoint.lastError && (
        <LastErrorNote
          error={endpoint.lastError}
          tone={errorTone(endpoint)}
          context={{
            failureStreak: endpoint.consecutiveFailureCount,
            retry:
              endpoint.nextRetryAt === null ? (
                "No retry is scheduled"
              ) : (
                <>
                  Next attempt <Countdown until={endpoint.nextRetryAt} />
                </>
              ),
          }}
        />
      )}
      <Facts layout="compact" items={endpointFacts(endpoint, heldBy)} />
      <Section title="Counters">
        <CountersTable
          counters={endpoint.counters}
          label="Endpoint counters"
          messages={using.some((channel) => channel.kind === "message")}
        />
      </Section>
      <EndpointAlerts alerts={alerts} />
      <ChannelsUsing channels={detail.channels} live={live} heldBy={endpoint.heldBy} />
      <TestEndpoint endpoint={endpoint} />
    </>
  );
}

function EndpointAlerts({ alerts }: { alerts: readonly Alert[] }) {
  return (
    <Section title="Active alerts" actions={<Button href="/alerts">View alerts</Button>}>
      {alerts.length === 0 ? (
        <p>No active alerts for this endpoint.</p>
      ) : (
        <ul aria-label="Active alerts of this endpoint">
          {alerts.map((alert) => (
            <li key={alert.id}>
              <StatusBadge tone={alertTone(alert.severity)}>
                {alertSeverityLabel(alert.severity)}
              </StatusBadge>{" "}
              <strong>{alert.title}</strong> · Next step: {alert.nextAction}
            </li>
          ))}
        </ul>
      )}
    </Section>
  );
}

function ChannelsUsing({
  channels,
  live,
  heldBy,
}: {
  channels: readonly UsingChannel[];
  live: readonly ChannelSnapshot[];
  heldBy: string | null;
}) {
  const columns: readonly MiniColumn<UsingChannel>[] = [
    {
      id: "channel",
      header: "Channel",
      cell: (channel) => <Link to={`/channels/${channel.id}`}>{channel.label}</Link>,
    },
    { id: "role", header: "Used as", cell: (channel) => channel.role },
    {
      id: "holds",
      header: "Holds it",
      cell: (channel) => (channel.id === heldBy ? "Yes" : "No"),
    },
    {
      id: "state",
      header: "Channel state",
      cell: (channel) => {
        const snapshot = live.find((candidate) => candidate.id === channel.id);
        return snapshot === undefined ? "Unknown" : <ChannelStateChip channel={snapshot} />;
      },
    },
  ];
  return (
    <Section title="Channels using this endpoint">
      {channels.length === 0 ? (
        <EmptyState
          title="No channel uses this endpoint"
          description="Add it to a channel to move data through it."
          action={<Button href="/channels/new">Add channel</Button>}
        />
      ) : (
        <MiniTable
          label="Channels using this endpoint"
          columns={columns}
          rows={channels}
          getRowId={(channel) => channel.id}
        />
      )}
    </Section>
  );
}
