import { useParams } from "react-router";
import { useChannel, useSnapshot } from "@/api/hooks";
import type { ChannelDetail, NodeSnapshot } from "@/api/types";
import { Alert } from "@/components/Alert";
import { Button } from "@/components/Button";
import { Page, PageHeader } from "@/components/Page";
import { QueryContent } from "@/components/QueryContent";
import { Section } from "@/components/Section";
import { ChannelStateChip } from "../shared-operate/ChannelStateChip";
import { CountersTable } from "../shared-operate/CountersTable";
import { allowedChannelActions } from "../shared-operate/channelRules";
import { RecordNotFound } from "../shared-operate/RecordNotFound";
import { RetryState } from "../shared-operate/RetryState";
import { CHANNEL_ACTION_LABELS, useChannelActions } from "../shared-operate/useChannelActions";
import {
  BridgeBuffersSection,
  BridgeEndpointsSection,
  BridgeTrafficSection,
} from "./BridgeSections";
import { ChannelError } from "./ChannelError";
import { ChannelAlertsSection, RecentErrorsSection, RecentEventsSection } from "./ChannelEvents";
import { ChannelFacts } from "./ChannelFacts";
import styles from "./ChannelPage.module.css";
import { DestinationsSection, SourceSection } from "./MessageSections";
import { RecentMessages } from "./RecentMessages";
import { TrafficPanel } from "./TrafficPanel";

/**
 * One channel (CON-03; operator-console.md section 4): its state and commands, the last
 * error with full context and the retry state, facts including the configuration versions,
 * what is specific to a bridge or a message channel, recent events and errors, active alerts
 * and the live transcript. The state follows the engine's snapshot; the rest refreshes every
 * few seconds.
 */
export function ChannelPage() {
  const { id = "" } = useParams();
  const detail = useChannel(id);
  if (detail.isError && detail.error.code === "not_found") {
    return <RecordNotFound what="Channel" listLabel="Channels" listHref="/channels" />;
  }
  return (
    <Page>
      <QueryContent query={detail} what="channel" pageTitle="Channel" loadingRows={8}>
        {(data) => <ChannelBody key={id} detail={data} />}
      </QueryContent>
    </Page>
  );
}

function ChannelBody({ detail }: { detail: ChannelDetail }) {
  const snapshot = useSnapshot();
  const actions = useChannelActions();
  const channel =
    snapshot.data?.channels.find((candidate) => candidate.id === detail.snapshot.id) ??
    detail.snapshot;
  const node: NodeSnapshot | null = snapshot.data?.node ?? null;
  const alerts = (snapshot.data?.openAlerts ?? []).filter(
    (alert) => alert.channelId === channel.id && alert.state === "open",
  );
  const allowed = allowedChannelActions(channel);
  const isBridge = channel.kind === "bridge";
  const [a, b] = channel.endpoints;
  return (
    <>
      <PageHeader
        title={channel.label}
        breadcrumbs={[
          { label: "Channels", href: "/channels" },
          { label: channel.label, href: `/channels/${channel.id}` },
        ]}
        status={<ChannelStateChip channel={channel} />}
        actions={
          <>
            {allowed.map((action) => (
              <Button
                key={action}
                variant={action === "resume" ? "primary" : "secondary"}
                onPress={() => actions.ask(channel, action)}
              >
                {CHANNEL_ACTION_LABELS[action]}
              </Button>
            ))}
            <Button href={`/channels/${channel.id}/edit`}>Edit</Button>
          </>
        }
      />
      <ChannelError
        channel={channel}
        destinations={detail.destinations}
        endpoints={snapshot.data?.endpoints ?? []}
      />
      {channel.stopReason === "not_implemented" && (
        <Alert tone="info" title="Not implemented in this engine build">
          This engine build does not run {isBridge ? "bridge" : "message"} channels yet, so nothing
          is received or delivered by this channel. The configuration is not at fault.
        </Alert>
      )}
      {channel.lastError === null &&
        channel.state !== "running" &&
        channel.stopReason !== "not_implemented" && (
          <p className={styles.retry}>
            <RetryState channel={channel} destinations={detail.destinations} />
          </p>
        )}
      <ChannelFacts channel={channel} config={detail.config} node={node} />
      {isBridge ? (
        <>
          <BridgeEndpointsSection channel={channel} />
          <div className={styles.pair}>
            <BridgeTrafficSection channel={channel} />
            <BridgeBuffersSection channel={channel} />
          </div>
        </>
      ) : (
        <>
          <SourceSection channel={channel} />
          <DestinationsSection channel={channel} destinations={detail.destinations} />
          <RecentMessages channelId={channel.id} />
          <Section title="Counters">
            <CountersTable counters={channel.counters} label="Channel counters" />
          </Section>
        </>
      )}
      <ChannelAlertsSection alerts={alerts} />
      <RecentErrorsSection errors={channel.recentErrors} />
      <RecentEventsSection channelId={channel.id} />
      <TrafficPanel
        channelId={channel.id}
        kind={channel.kind}
        endpointLabels={isBridge && a && b ? [a.label, b.label] : null}
        destinations={detail.destinations}
      />
      {actions.dialog}
    </>
  );
}
