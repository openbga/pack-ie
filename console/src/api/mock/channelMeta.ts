import type { PanelKey } from "./panels";

/** What the mock knows about a channel beyond its configuration: how its traffic looks. */
export interface ChannelMeta {
  panel: PanelKey;
  /** Share of the message store, relative to other message channels. */
  weight: number;
  /** Permille of messages whose direction is outbound. */
  outbound: number;
  /** Messages per second shown while ticking (a busy lab, not the retained history). */
  rate: number;
  /** Protocol message types, picked per message. */
  types: string[];
}

/** Per-channel mock behaviour, by channel id. Filled while the initial document is built. */
export const CHANNEL_META: Record<string, ChannelMeta> = {};

const DEFAULT_META: ChannelMeta = { panel: "chem", weight: 1, outbound: 20, rate: 0.3, types: [] };

export function metaFor(channelId: string): ChannelMeta {
  return CHANNEL_META[channelId] ?? DEFAULT_META;
}
