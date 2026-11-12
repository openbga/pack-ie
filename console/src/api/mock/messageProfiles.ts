import type { AckMode, ConfigDocument, EndpointKind, Protocol } from "../types";
import { CHANNEL_META } from "./channelMeta";
import { endpointTarget } from "./endpointInfo";
import type { PanelKey } from "./panels";

export interface DestinationInfo {
  destinationId: string;
  label: string;
  endpointLabel: string;
  endpointKind: EndpointKind;
  target: string;
  protocol: Protocol;
  /** A disabled destination is not in the active configuration for resends. */
  enabled: boolean;
  /** Whether a mapping changes the message; without one the original passes through. */
  hasMappings: boolean;
  ackMode: AckMode;
  /** AU only: class A has no acknowledgement, class B acknowledges every frame (spec §8). */
  auClass: "a" | "b" | null;
}

/**
 * What the message generator knows about one channel whose messages the store holds, frozen
 * when the mock starts. A `tap` profile is a bridge whose tap records the messages it sees.
 */
export interface MessageProfile {
  channelId: string;
  label: string;
  protocol: Protocol;
  panel: PanelKey;
  weight: number;
  /** Protocol message types, picked per message. */
  types: string[];
  destinations: DestinationInfo[];
  tap: boolean;
  sourceEndpointLabel: string;
  sourceEndpointId: string;
  /** Peers that send to the source, as the store shows them; empty for files. */
  peers: string[];
  sender: string;
  maxAttempts: number;
  initialMs: number;
  ackTimeoutMs: number;
  /** The configuration version at the time the profile was built. */
  configVersion: number;
}

function senderName(label: string): string {
  const tokens = label.split(/\s+/);
  const token = tokens.find((t) => /[A-Za-z]/.test(t) && /\d/.test(t)) ?? tokens[0] ?? "LAB";
  return token.replace(/[^A-Za-z0-9-]/g, "").slice(0, 10);
}

/** Message types of a tap profile, by what its framer recognises. */
const TAP_TYPES: Record<string, string[]> = { hl7_mllp: ["ORU^R01"], astm: ["astm"], au: ["au"] };

const TAP_WEIGHT = 0.15;

function peersOf(kind: EndpointKind | undefined, target: string, channelId: string): string[] {
  switch (kind) {
    case "tcp_listener": {
      const n = [...channelId].reduce((sum, ch) => (sum * 31 + ch.charCodeAt(0)) >>> 0, 7);
      return [`10.20.${n % 8}.${10 + (n % 40)}:${40_000 + (n % 20_000)}`];
    }
    case "tcp_client":
    case "serial":
      return [target];
    default:
      return [];
  }
}

type BridgeConfig = Extract<ConfigDocument["channels"][number], { kind: "bridge" }>;
type MessageConfig = Extract<ConfigDocument["channels"][number], { kind: "message" }>;

/** A bridge whose tap records messages, as the store shows its observed messages. */
function tapProfile(channel: BridgeConfig, document: ConfigDocument): MessageProfile | null {
  if (channel.tapFramer === "none") return null;
  const endpoint = document.endpoints.find((e) => e.id === channel.endpointAId);
  const label = endpoint?.label ?? channel.endpointAId;
  return {
    channelId: channel.id,
    label: channel.label,
    protocol: channel.tapFramer,
    panel: CHANNEL_META[channel.id]?.panel ?? "chem",
    weight: TAP_WEIGHT,
    types: TAP_TYPES[channel.tapFramer] ?? [],
    destinations: [],
    tap: true,
    sourceEndpointLabel: label,
    sourceEndpointId: channel.endpointAId,
    peers: peersOf(endpoint?.kind, endpoint ? endpointTarget(endpoint) : label, channel.id),
    sender: senderName(label),
    maxAttempts: 1,
    initialMs: 1000,
    ackTimeoutMs: 10_000,
    configVersion: document.version,
  };
}

function destinationInfos(channel: MessageConfig, document: ConfigDocument): DestinationInfo[] {
  return channel.destinations.map((destination) => {
    const endpoint = document.endpoints.find((e) => e.id === destination.endpointId);
    return {
      destinationId: destination.id,
      label: destination.label,
      endpointLabel: endpoint?.label ?? destination.endpointId,
      endpointKind: endpoint?.kind ?? "tcp_client",
      target: endpoint ? endpointTarget(endpoint) : destination.endpointId,
      protocol: destination.protocol,
      enabled: destination.enabled,
      hasMappings: destination.mappings.length > 0,
      ackMode: destination.settings.ackMode ?? "original",
      auClass: destination.settings.au?.framingClass ?? null,
    };
  });
}

function messageProfile(channel: MessageConfig, document: ConfigDocument): MessageProfile {
  const meta = CHANNEL_META[channel.id];
  const source = document.endpoints.find((e) => e.id === channel.source.endpointId);
  const sourceLabel = source?.label ?? channel.source.endpointId;
  return {
    channelId: channel.id,
    label: channel.label,
    protocol: channel.source.protocol,
    panel: meta?.panel ?? "chem",
    weight: Math.max(meta?.weight ?? 1, 1),
    types: meta?.types.length ? meta.types : ["Raw frame"],
    destinations: destinationInfos(channel, document),
    tap: false,
    sourceEndpointLabel: sourceLabel,
    sourceEndpointId: channel.source.endpointId,
    peers: peersOf(source?.kind, source ? endpointTarget(source) : sourceLabel, channel.id),
    sender: senderName(sourceLabel),
    maxAttempts: channel.destinations[0]?.retry.maxAttempts ?? 5,
    initialMs: channel.destinations[0]?.retry.initialMs ?? 2000,
    ackTimeoutMs: 10_000,
    configVersion: document.version,
  };
}

export function buildProfiles(document: ConfigDocument): MessageProfile[] {
  return document.channels.flatMap((channel) => {
    const profile =
      channel.kind === "bridge" ? tapProfile(channel, document) : messageProfile(channel, document);
    return profile === null ? [] : [profile];
  });
}
