import type { AlertThresholds, ChannelConfig, ConfigDocument, EndpointConfig } from "../types";
import type { AlertCondition, AlertStore } from "./alerts";
import { channelsUsing } from "./endpointInfo";
import type { Runtime } from "./runtime";
import type { ChannelRuntime, DerivedStates } from "./runtimeState";
import { MIB, MINUTE } from "./util";

/** An open pressure alert stays open until the level falls this far below its limit. */
const HYSTERESIS = 0.1;

function endpointCondition(
  endpoint: EndpointConfig,
  runtime: Runtime,
  derived: DerivedStates,
  document: ConfigDocument,
): AlertCondition | null {
  const state = derived.endpoints.get(endpoint.id);
  const fault = runtime.endpoints.get(endpoint.id)?.fault;
  if (
    fault === null ||
    fault === undefined ||
    (state !== "error" && state !== "retrying" && state !== "disconnected")
  ) {
    return null;
  }
  const users = channelsUsing(document, endpoint.id);
  if (users.every((c) => derived.raw.get(c.id) === "stopped")) return null;
  const down = state === "error";
  return {
    key: `endpoint_down:${endpoint.id}`,
    kind: "endpoint_down",
    severity: down ? "critical" : "warning",
    title: down ? `${endpoint.label} is down` : `${endpoint.label} is reconnecting`,
    detail: fault.error.summary,
    channelId: users[0]?.id ?? null,
    endpointId: endpoint.id,
    nextAction: fault.error.nextAction,
    sinceMs: fault.error.atMs,
  };
}

type ChannelState = NonNullable<ReturnType<DerivedStates["channels"]["get"]>>;

function stateCondition(
  channel: ChannelConfig,
  rt: ChannelRuntime,
  state: ChannelState,
): AlertCondition | null {
  const stoppedBadly =
    state.state === "stopped" &&
    (state.stopReason === "fatal" || state.stopReason === "configuration");
  if (!stoppedBadly || rt.stopError === null) return null;
  return {
    channelId: channel.id,
    endpointId: null,
    key: `channel_failing:${channel.id}`,
    kind: "no_peer_connected",
    severity: "critical",
    title: `${channel.label} has stopped`,
    detail: rt.stopError.summary,
    nextAction: rt.stopError.nextAction,
    sinceMs: rt.stopError.atMs,
  };
}

function deliveriesCondition(channel: ChannelConfig, rt: ChannelRuntime): AlertCondition | null {
  if (channel.kind !== "message" || rt.needsAttention < 5) return null;
  return {
    channelId: channel.id,
    endpointId: null,
    key: `deliveries_need_attention:${channel.id}`,
    kind: "delivery_needs_attention",
    severity: rt.needsAttention >= 20 ? "critical" : "warning",
    title: `${channel.label}: deliveries failing`,
    detail: `${rt.needsAttention} deliveries need attention`,
    nextAction:
      "Open Messages filtered by needs attention, fix the cause, then resend or close them.",
  };
}

/** Buffer fill (0 to 1) of the fuller side of a bridge. */
function bufferFill(rt: ChannelRuntime): number {
  return Math.max(rt.bufferA, rt.bufferB);
}

function bufferCondition(
  channel: ChannelConfig,
  rt: ChannelRuntime,
  thresholds: AlertThresholds,
  stopped: boolean,
  wasOpen: boolean,
): AlertCondition | null {
  const limit = thresholds.bufferPressurePercent / 100;
  const fill = bufferFill(rt);
  if (channel.kind !== "bridge" || stopped || fill < (wasOpen ? limit - HYSTERESIS : limit)) {
    return null;
  }
  return {
    channelId: channel.id,
    endpointId: null,
    key: `buffer_pressure:${channel.id}`,
    kind: "bridge_buffer_pressure",
    severity: fill >= 0.95 ? "critical" : "warning",
    title: `${channel.label}: buffer is filling`,
    detail: `A buffer is ${Math.round(fill * 100)}% full`,
    nextAction: "Check the receiving side of the bridge; the buffer drains once it is connected.",
  };
}

function queueCondition(
  channel: ChannelConfig,
  rt: ChannelRuntime,
  thresholds: AlertThresholds,
  stopped: boolean,
  wasOpen: boolean,
): AlertCondition | null {
  const limit = thresholds.queueDepthWarning;
  if (channel.kind !== "message" || stopped || limit <= 0) return null;
  if (rt.queueDepth < (wasOpen ? limit * (1 - HYSTERESIS) : limit)) return null;
  return {
    channelId: channel.id,
    endpointId: null,
    key: `queue_pressure:${channel.id}`,
    kind: "delivery_backlog",
    severity: rt.queueDepth >= limit * 2 ? "critical" : "warning",
    title: `${channel.label}: queue is filling`,
    detail: `${rt.queueDepth} deliveries are waiting (warning at ${limit})`,
    nextAction: "Check that the destinations are acknowledging; the queue drains once they do.",
  };
}

function channelConditions(
  channel: ChannelConfig,
  runtime: Runtime,
  derived: DerivedStates,
  store: AlertStore,
  document: ConfigDocument,
): AlertCondition[] {
  const rt = runtime.channels.get(channel.id);
  const state = derived.channels.get(channel.id);
  if (rt === undefined || state === undefined) return [];
  const stopped = state.state === "stopped";
  const { alertThresholds } = document;
  const conditions = [
    stateCondition(channel, rt, state),
    deliveriesCondition(channel, rt),
    bufferCondition(
      channel,
      rt,
      alertThresholds,
      stopped,
      store.isOpen(`buffer_pressure:${channel.id}`),
    ),
    queueCondition(
      channel,
      rt,
      alertThresholds,
      stopped,
      store.isOpen(`queue_pressure:${channel.id}`),
    ),
  ];
  return conditions.filter((condition): condition is AlertCondition => condition !== null);
}

/** What is wrong right now, from endpoint and channel state and the disk. */
export function alertConditions(
  document: ConfigDocument,
  runtime: Runtime,
  derived: DerivedStates,
  store: AlertStore,
  nowMs: number,
): AlertCondition[] {
  const out: AlertCondition[] = [];
  for (const endpoint of document.endpoints) {
    const condition = endpointCondition(endpoint, runtime, derived, document);
    if (condition !== null) out.push(condition);
  }
  for (const channel of document.channels) {
    out.push(...channelConditions(channel, runtime, derived, store, document));
  }
  const freeMib = runtime.disk.freeBytes / MIB;
  if (freeMib < document.alertThresholds.diskFreeMinMib) {
    out.push({
      key: "disk_low",
      kind: "disk_low",
      severity: freeMib < document.alertThresholds.diskFreeMinMib / 4 ? "critical" : "warning",
      title: "Disk space is low",
      detail: `${Math.round(freeMib)} MiB of the data drive is free`,
      channelId: null,
      endpointId: null,
      nextAction: "Free disk space or lower message retention in the channel settings.",
      sinceMs: nowMs - 40 * MINUTE,
    });
  }
  return out;
}
