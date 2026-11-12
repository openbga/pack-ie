import { clone } from "@/lib/clone";
import type { ChannelConfig, ChannelDetail, EndpointDetail } from "../types";
import { destinationStatuses } from "./destinations";
import { channelsUsing, endpointRole } from "./endpointInfo";
import { channelSnapshot, endpointRefs, endpointSnapshot, type SnapshotInputs } from "./snapshot";

export function channelDetail(config: ChannelConfig, inputs: SnapshotInputs): ChannelDetail | null {
  const snapshot = channelSnapshot(config, inputs);
  if (snapshot === null) return null;
  const refs = endpointRefs(config, inputs.document, inputs.derived);
  return {
    snapshot,
    config: clone(config),
    destinations: config.kind === "message" ? destinationStatuses(config, inputs, refs) : [],
  };
}

export function endpointDetail(id: string, inputs: SnapshotInputs): EndpointDetail | null {
  const config = inputs.document.endpoints.find((endpoint) => endpoint.id === id);
  if (config === undefined) return null;
  const snapshot = endpointSnapshot(config, inputs);
  if (snapshot === null) return null;
  return {
    snapshot,
    config: clone(config),
    channels: channelsUsing(inputs.document, id).map((channel) => ({
      id: channel.id,
      label: channel.label,
      role: endpointRole(channel, id),
    })),
  };
}
