import type { ChannelConfig, ConfigDocument, EndpointConfig } from "../types";

const PARITY_LETTER = { none: "N", even: "E", odd: "O", mark: "M", space: "S" } as const;

/** One line describing where an endpoint points, as the snapshot shows it. */
export function endpointSummary(endpoint: EndpointConfig): string {
  switch (endpoint.kind) {
    case "serial": {
      const parity = PARITY_LETTER[endpoint.parity];
      return `${endpoint.port} · ${endpoint.baudRate} ${endpoint.dataBits}${parity}${endpoint.stopBits}`;
    }
    case "tcp_client":
      return `tcp://${endpoint.host}:${endpoint.port}`;
    case "tcp_listener":
      return `listen ${endpoint.bindAddress}:${endpoint.port}`;
    case "folder":
      return endpoint.path;
    case "sftp":
      return `sftp://${endpoint.username}@${endpoint.host}:${endpoint.port}${endpoint.remotePath}`;
  }
}

/** The short target used in log lines: `10.0.4.12:3001`, `COM3`, a folder path. */
export function endpointTarget(endpoint: EndpointConfig): string {
  switch (endpoint.kind) {
    case "serial":
      return endpoint.port;
    case "tcp_client":
    case "sftp":
      return `${endpoint.host}:${endpoint.port}`;
    case "tcp_listener":
      return `${endpoint.bindAddress}:${endpoint.port}`;
    case "folder":
      return endpoint.path;
  }
}

/** `[A, B]` for a bridge, `[source, ...destinations]` for a message channel. */
export function channelEndpointIds(channel: ChannelConfig): string[] {
  if (channel.kind === "bridge") return [channel.endpointAId, channel.endpointBId];
  return [channel.source.endpointId, ...channel.destinations.map((d) => d.endpointId)];
}

/** The channels that use an endpoint, on either side. */
export function channelsUsing(document: ConfigDocument, endpointId: string): ChannelConfig[] {
  return document.channels.filter((channel) => channelEndpointIds(channel).includes(endpointId));
}

/** The role of an endpoint in one channel, for the endpoint detail page. */
export function endpointRole(channel: ChannelConfig, endpointId: string): string {
  if (channel.kind === "bridge") {
    return channel.endpointAId === endpointId ? "Bridge side A" : "Bridge side B";
  }
  return channel.source.endpointId === endpointId ? "Source" : "Destination";
}

export function findChannel(document: ConfigDocument, id: string): ChannelConfig | undefined {
  return document.channels.find((channel) => channel.id === id);
}
