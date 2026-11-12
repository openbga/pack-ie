import type {
  ApplyPlan,
  ChannelConfig,
  ChannelOutcome,
  ConfigDocument,
  EndpointConfig,
  PlanItem,
} from "../types";
import { channelEndpointIds } from "./endpointInfo";

export interface ConfigDiff {
  addedEndpoints: EndpointConfig[];
  removedEndpoints: EndpointConfig[];
  changedEndpoints: EndpointConfig[];
  addedChannels: ChannelConfig[];
  removedChannels: ChannelConfig[];
  thresholdsChanged: boolean;
}

/** JSON with sorted keys, so two documents compare equal whatever order a form wrote them in. */
function stable(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(stable).join(",")}]`;
  if (value !== null && typeof value === "object") {
    const entries = Object.entries(value as Record<string, unknown>)
      .filter(([, v]) => v !== undefined)
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([k, v]) => `${JSON.stringify(k)}:${stable(v)}`);
    return `{${entries.join(",")}}`;
  }
  return JSON.stringify(value) ?? "null";
}

function diffById<T extends { id: string }>(before: T[], after: T[]) {
  const previous = new Map(before.map((item) => [item.id, item]));
  const next = new Map(after.map((item) => [item.id, item]));
  return {
    added: after.filter((item) => !previous.has(item.id)),
    removed: before.filter((item) => !next.has(item.id)),
    changed: after.filter((item) => {
      const old = previous.get(item.id);
      return old !== undefined && stable(old) !== stable(item);
    }),
  };
}

export function diffDocuments(before: ConfigDocument, after: ConfigDocument): ConfigDiff {
  const endpoints = diffById(before.endpoints, after.endpoints);
  const channels = diffById(before.channels, after.channels);
  return {
    addedEndpoints: endpoints.added,
    removedEndpoints: endpoints.removed,
    changedEndpoints: endpoints.changed,
    addedChannels: channels.added,
    removedChannels: channels.removed,
    thresholdsChanged: stable(before.alertThresholds) !== stable(after.alertThresholds),
  };
}

/**
 * What the engine does to a channel that exists on both sides (configuration.md section 10,
 * "In place"): a change confined to the label, or for a message channel to its filters, its
 * destinations' filters, mappings, code tables and retry policy, and retention, is taken in
 * place; any other change, and any change of an endpoint it uses, restarts it.
 */
function existingChannelChange(
  before: ChannelConfig,
  after: ChannelConfig,
  changedEndpoints: ReadonlySet<string>,
): "changed" | "updated" | "unchanged" {
  if (channelEndpointIds(after).some((id) => changedEndpoints.has(id))) return "changed";
  if (stable(before) === stable(after)) return "unchanged";
  return stable(withoutInPlaceSettings(before)) === stable(withoutInPlaceSettings(after))
    ? "updated"
    : "changed";
}

/** The channel with the settings an apply takes in place blanked: what a restart depends on. */
function withoutInPlaceSettings(channel: ChannelConfig): ChannelConfig {
  if (channel.kind === "bridge") return { ...channel, label: "" };
  return {
    ...channel,
    label: "",
    filters: [],
    retention: { days: null, maxMessages: null },
    destinations: channel.destinations.map((destination) => ({
      ...destination,
      filters: [],
      mappings: [],
      codeTables: {},
      retry: { initialMs: 0, maxMs: 0, maxAttempts: null },
    })),
  };
}

/**
 * The plan for applying `after` over `before`, as the engine reports it for a valid document:
 * items in the new document's order, then the removed ones. A channel is `changed` (restarted)
 * when it was edited beyond what is taken in place or uses an edited endpoint, `updated` when
 * only in-place settings changed, and `stopped` instead of `added` or `changed` when the new
 * document disables it.
 */
export function planDocument(before: ConfigDocument, after: ConfigDocument): ApplyPlan {
  const diff = diffDocuments(before, after);
  const changedEndpoints = new Set(diff.changedEndpoints.map((e) => e.id));
  const previous = new Map(before.channels.map((channel) => [channel.id, channel]));
  const channels: PlanItem[] = after.channels.map((channel) => {
    const old = previous.get(channel.id);
    const change = old ? existingChannelChange(old, channel, changedEndpoints) : "added";
    // A channel the new document disables is stopped, not started or restarted.
    const stopped = !channel.enabled && (change === "added" || change === "changed");
    return { id: channel.id, label: channel.label, change: stopped ? "stopped" : change };
  });
  for (const { id, label } of diff.removedChannels) channels.push({ id, label, change: "removed" });
  const addedEndpoints = new Set(diff.addedEndpoints.map((e) => e.id));
  const endpoints: PlanItem[] = after.endpoints.map(({ id, label }) => ({
    id,
    label,
    change: addedEndpoints.has(id) ? "added" : changedEndpoints.has(id) ? "changed" : "unchanged",
  }));
  for (const { id, label } of diff.removedEndpoints) {
    endpoints.push({ id, label, change: "removed" });
  }
  return { channels, endpoints, alertsChanged: diff.thresholdsChanged };
}

const OUTCOME_OF_CHANGE = {
  added: "started",
  changed: "restarted",
  updated: "updated",
  removed: "removed",
  unchanged: "unchanged",
  stopped: "stopped",
} as const;

/** What an apply of this plan does to every channel: the engine's per-channel outcomes. */
export function outcomesOfPlan(plan: ApplyPlan): ChannelOutcome[] {
  return plan.channels.map(({ id, change }) => ({
    channelId: id,
    outcome: OUTCOME_OF_CHANGE[change],
  }));
}
