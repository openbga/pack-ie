/** Channel and endpoint detail: one snapshot item, its document and what ties them together. */

import type { EngineSnapshot } from "@/ipc/generated";
import type { ChannelDetail, EndpointDetail, Snapshot } from "../types";
import { channelFromWire } from "./channelConfig";
import { destinationFrom } from "./destinations";
import { endpointFromWire } from "./endpointConfig";
import { engineError } from "./errors";
import type { WireChannel, WireConfig } from "./wire";

export function channelDetail(
  id: string,
  raw: EngineSnapshot,
  snapshot: Snapshot,
  config: WireConfig,
): ChannelDetail {
  const channelSnapshot = snapshot.channels.find((c) => c.id === id);
  const channelConfig = config.channels?.find((c) => c.channel_id === id);
  if (channelSnapshot === undefined || channelConfig === undefined) {
    throw engineError(
      "not_found",
      `Channel ${id} was not found. A configuration change may have removed it.`,
    );
  }
  const rawChannel = raw.channels.find((c) => c.channel_id === id);
  const destinations = rawChannel?.message?.destinations ?? [];
  return {
    snapshot: channelSnapshot,
    config: channelFromWire(channelConfig),
    destinations: destinations.map(destinationFrom),
  };
}

function rolesIn(channel: WireChannel, endpointId: string): string[] {
  if (channel.kind === "bridge") {
    return [
      ...(channel.endpoint_a === endpointId ? ["A"] : []),
      ...(channel.endpoint_b === endpointId ? ["B"] : []),
    ];
  }
  return [
    ...(channel.source.endpoint === endpointId ? ["source"] : []),
    ...(channel.destinations.some((d) => d.endpoint === endpointId) ? ["destination"] : []),
  ];
}

export function endpointDetail(id: string, snapshot: Snapshot, config: WireConfig): EndpointDetail {
  const endpointSnapshot = snapshot.endpoints.find((e) => e.id === id);
  const endpointConfig = config.endpoints?.find((e) => e.endpoint_id === id);
  if (endpointSnapshot === undefined || endpointConfig === undefined) {
    throw engineError(
      "not_found",
      `Endpoint ${id} was not found. A configuration change may have removed it.`,
    );
  }
  return {
    snapshot: endpointSnapshot,
    config: endpointFromWire(endpointConfig),
    channels: (config.channels ?? []).flatMap((channel) =>
      rolesIn(channel, id).map((role) => ({ id: channel.channel_id, label: channel.label, role })),
    ),
  };
}
