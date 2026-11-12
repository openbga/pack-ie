// Key settings of endpoints and channels in words, for the read-only view of a version.
// Secrets appear by name only; the document never holds their values.
import type { ChannelConfig, ConfigDocument, EndpointConfig, RetryPolicy } from "@/api/types";
import { formatBytes, formatCount } from "@/lib/format";
import { endpointKindLabel, protocolLabel } from "@/lib/status";
import { framerText, pairingText, retentionText } from "../channels/channelLabels";

export interface Setting {
  label: string;
  value: string;
}

const PARITY = { none: "no parity", even: "even parity", odd: "odd parity" } as const;
const FLOW = { none: "no flow control", rts_cts: "RTS/CTS", xon_xoff: "XON/XOFF" } as const;

function idleText(seconds: number): string {
  return seconds === 0 ? "off" : `${formatCount(seconds)} s`;
}

function kindSettings(endpoint: EndpointConfig): Setting[] {
  switch (endpoint.kind) {
    case "serial":
      return [
        { label: "Port", value: endpoint.port },
        {
          label: "Line",
          value: `${formatCount(endpoint.baudRate)} baud, ${endpoint.dataBits} data bits, ${PARITY[endpoint.parity]}, ${endpoint.stopBits} stop ${endpoint.stopBits === 1 ? "bit" : "bits"}, ${FLOW[endpoint.flowControl]}`,
        },
      ];
    case "tcp_client":
      return [
        { label: "Connects to", value: `${endpoint.host}:${endpoint.port}` },
        { label: "Idle timeout", value: idleText(endpoint.idleTimeoutSeconds) },
      ];
    case "tcp_listener":
      return [
        { label: "Listens on", value: `${endpoint.bindAddress}:${endpoint.port}` },
        {
          label: "Peers",
          value: endpoint.allowAnyPeer
            ? "Any peer"
            : `${endpoint.allowlist.length} allowed: ${endpoint.allowlist.join(", ") || "none"}`,
        },
        { label: "Idle timeout", value: idleText(endpoint.idleTimeoutSeconds) },
      ];
    case "folder":
      return [
        { label: "Folder", value: endpoint.path },
        {
          label: "Mode",
          value:
            endpoint.mode === "watch" ? "Watches" : `Polls every ${endpoint.pollIntervalSec} s`,
        },
        { label: "Files", value: endpoint.filePattern },
      ];
    case "sftp":
      return [
        { label: "Server", value: `${endpoint.username}@${endpoint.host}:${endpoint.port}` },
        {
          label: "Remote path",
          value: `${endpoint.remotePath} (${endpoint.filePattern}), every ${endpoint.pollIntervalSec} s`,
        },
        {
          label: "Authentication",
          value: `${endpoint.authMethod === "password" ? "Password" : "Private key"} from secret ${endpoint.secretName || "(not set)"}`,
        },
        { label: "Host key", value: endpoint.hostKeyFingerprint },
      ];
  }
}

/** An endpoint's key settings, in the order a person reads them. */
export function endpointSettings(endpoint: EndpointConfig): Setting[] {
  return [
    { label: "Kind", value: endpointKindLabel(endpoint.kind) },
    { label: "Enabled", value: endpoint.enabled ? "Yes" : "No" },
    ...kindSettings(endpoint),
  ];
}

function retryText(retry: RetryPolicy): string {
  const attempts =
    retry.maxAttempts === null ? "retries for ever" : `${retry.maxAttempts} attempts`;
  return `${formatCount(retry.initialMs)} ms growing to ${formatCount(retry.maxMs)} ms, ${attempts}`;
}

/** A channel's key settings; endpoints are named, not given by id. */
export function channelSettings(channel: ChannelConfig, document: ConfigDocument): Setting[] {
  const name = (id: string) => document.endpoints.find((each) => each.id === id)?.label ?? id;
  const common = [
    { label: "Kind", value: channel.kind === "bridge" ? "Bridge" : "Message channel" },
    { label: "Enabled", value: channel.enabled ? "Yes" : "No" },
  ];
  if (channel.kind === "bridge") {
    return [
      ...common,
      { label: "Endpoint A", value: name(channel.endpointAId) },
      { label: "Endpoint B", value: name(channel.endpointBId) },
      { label: "Pairing", value: pairingText(channel.pairing) },
      { label: "Buffer per direction", value: formatBytes(channel.bufferBytes) },
      { label: "Message framing", value: framerText(channel.tapFramer) },
      { label: "Recorded messages kept", value: retentionText(channel.tapRetention) },
      { label: "Reconnect", value: retryText(channel.retry) },
    ];
  }
  return [
    ...common,
    {
      label: "Source",
      value: `${name(channel.source.endpointId)} (${protocolLabel(channel.source.protocol)})`,
    },
    { label: "Source filters", value: formatCount(channel.filters.length) },
    {
      label: "Destinations",
      value:
        channel.destinations
          .map(
            (each) =>
              `${each.label} to ${name(each.endpointId)} (${protocolLabel(each.protocol)}${each.enabled ? "" : ", disabled"}, ${each.mappings.length} mapping rules)`,
          )
          .join("; ") || "none",
    },
    { label: "Retention", value: retentionText(channel.retention) },
  ];
}
