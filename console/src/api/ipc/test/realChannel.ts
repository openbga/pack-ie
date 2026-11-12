// A message channel from a real engine snapshot, mapped as the console maps it, for page tests
// that must not know the wire names.

import type { EngineSnapshot } from "@/ipc/generated";
import type { ChannelSnapshot, DestinationStatus } from "../../types";
import { destinationFrom } from "../destinations";
import realAlerts from "../real/snapshot-alerts.json";
import { snapshotFromIpc } from "../snapshot";

/** The snapshot's message channel (one the engine build does not run) and its destinations. */
export function realMessageChannel(): {
  channel: ChannelSnapshot;
  destinations: DestinationStatus[];
} {
  const wire = realAlerts as unknown as EngineSnapshot;
  const channel = snapshotFromIpc(wire, null).channels.find((each) => each.kind === "message");
  const raw = wire.channels.find((each) => each.kind === "message");
  if (channel === undefined || raw === undefined) throw new Error("fixture has no message channel");
  return { channel, destinations: raw.message?.destinations.map(destinationFrom) ?? [] };
}
