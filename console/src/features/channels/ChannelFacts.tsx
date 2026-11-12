import type { ChannelDetail, ChannelSnapshot, NodeSnapshot } from "@/api/types";
import { Facts } from "@/components/Facts";
import { Timestamp } from "@/components/Timestamp";
import { formatAge, formatBytes, formatCount } from "@/lib/format";
import { channelKindLabel, protocolLabel } from "@/lib/status";
import { ChannelStateChip } from "../shared-operate/ChannelStateChip";
import { Countdown } from "../shared-operate/Countdown";
import { activityText } from "../shared-operate/channelRules";
import { sessionLabel, sourceSessionLabel } from "../shared-operate/stateShapes";
import { framerText, pairingText, retentionText, stopReasonText } from "./channelLabels";

function configFacts(config: ChannelDetail["config"]) {
  if (config.kind === "bridge") {
    return [
      { label: "Pairing", value: pairingText(config.pairing) },
      ...(config.pairingReceiver === null
        ? []
        : [{ label: "Receiver", value: `Side ${config.pairingReceiver.toUpperCase()}` }]),
      { label: "Buffer per direction", value: formatBytes(config.bufferBytes) },
      { label: "Message framing", value: framerText(config.tapFramer) },
    ];
  }
  return [
    { label: "Filters", value: formatCount(config.filters.length) },
    { label: "Destinations", value: formatCount(config.destinations.length) },
    { label: "Retention", value: retentionText(config.retention) },
  ];
}

function sessionFact(channel: ChannelSnapshot) {
  if (channel.bridge !== null) {
    return [{ label: "Session", value: sessionLabel(channel.bridge.sessionState) }];
  }
  if (channel.source !== null) {
    return [{ label: "Session", value: sourceSessionLabel(channel.source.session) }];
  }
  return [];
}

/** When an instance does not run the active configuration, the page says so. */
function versionFacts(channel: ChannelSnapshot, node: NodeSnapshot | null) {
  const instance = channel.configVersion;
  const active = node?.configVersion ?? null;
  return [
    {
      label: "Configuration version",
      value:
        instance === null
          ? "Not reported"
          : `${instance}${active === null || instance === active ? "" : " (not the active version)"}`,
    },
    {
      label: "Active configuration",
      value: active === null ? "Not reported" : `Version ${active}`,
    },
    {
      label: "Pending configuration",
      value:
        node === null
          ? "Not reported"
          : node.pendingConfigVersion === null
            ? "None"
            : `Version ${node.pendingConfigVersion}`,
    },
  ];
}

/** The channel's identity and standing in one compact, wrapping list. */
export function ChannelFacts({
  channel,
  config,
  node,
}: {
  channel: ChannelSnapshot;
  config: ChannelDetail["config"];
  node: NodeSnapshot | null;
}) {
  return (
    <Facts
      layout="compact"
      items={[
        { label: "Channel id", value: channel.id },
        { label: "Kind", value: channelKindLabel(channel.kind) },
        ...(channel.protocol === null
          ? []
          : [{ label: "Protocol", value: protocolLabel(channel.protocol) }]),
        { label: "Enabled", value: channel.enabled ? "Yes" : "No" },
        { label: "State", value: <ChannelStateChip channel={channel} /> },
        ...sessionFact(channel),
        ...(channel.stopReason === null
          ? []
          : [{ label: "Stop reason", value: stopReasonText(channel.stopReason) }]),
        ...versionFacts(channel, node),
        ...(channel.instanceStartedAt === null
          ? []
          : [
              {
                label: "Running since",
                value: (
                  <>
                    <Timestamp value={channel.instanceStartedAt} seconds /> (
                    {formatAge(channel.instanceStartedAt)})
                  </>
                ),
              },
            ]),
        {
          label: "Last activity",
          value:
            channel.lastActivityAt === null ? (
              activityText(null)
            ) : (
              <>
                {activityText(channel.lastActivityAt)} (
                <Timestamp value={channel.lastActivityAt} seconds />)
              </>
            ),
        },
        { label: "Retries", value: formatCount(channel.counters.retries) },
        ...(channel.nextRetryAt === null
          ? []
          : [{ label: "Next retry", value: <Countdown until={channel.nextRetryAt} /> }]),
        ...configFacts(config),
      ]}
    />
  );
}
