import type { BridgeChannelConfig, PairingStrategy, TapFramer } from "../types";
import { CHANNEL_META } from "./channelMeta";
import { type BridgeSpec, bridgeSpecs } from "./endpointCatalog";
import type { PanelKey } from "./panels";

const BRIDGE_TAP: Record<string, TapFramer> = {
  au5800: "au",
  c503: "astm",
  e801: "astm",
  im1600: "astm",
  v5600: "astm",
  acltop: "astm",
  u411: "none",
  b221: "none",
  d100: "astm",
  xn3000: "astm",
  dxh900: "hl7_mllp",
  ci4100: "astm",
};

const BRIDGE_PANEL: Record<string, PanelKey> = {
  au5800: "chem",
  c503: "chem",
  e801: "immuno",
  im1600: "immuno",
  v5600: "chem",
  acltop: "coag",
  u411: "urine",
  b221: "gases",
  d100: "hba1c",
  xn3000: "hema",
  dxh900: "hema",
  ci4100: "immuno",
};

function pairingFor(spec: BridgeSpec): PairingStrategy {
  if (spec.slug === "ci4100") return "receiver_then_sender";
  return spec.endpointA.kind === "tcp_listener" ? "wait_for_listener_then_pair" : "parallel";
}

export function bridgeChannels(): BridgeChannelConfig[] {
  return bridgeSpecs().map((spec) => {
    const id = `ch-br-${spec.slug}`;
    CHANNEL_META[id] = {
      panel: BRIDGE_PANEL[spec.slug] ?? "chem",
      weight: 0,
      outbound: 0,
      rate: 0.6,
      types: [],
    };
    return {
      kind: "bridge",
      id,
      label: `${spec.label} to middleware`,
      enabled: true,
      endpointAId: spec.endpointA.id,
      endpointBId: spec.endpointB.id,
      pairing: pairingFor(spec),
      pairingReceiver: spec.slug === "ci4100" ? "a" : null,
      bufferBytes: 262_144,
      retry: { initialMs: 1000, maxMs: 60_000, maxAttempts: null },
      tapFramer: BRIDGE_TAP[spec.slug] ?? "none",
      tapRetention: { days: 30, maxMessages: null },
    };
  });
}
