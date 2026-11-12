import type { ConfigDocument, EndpointConfig } from "../types";
import { channelsUsing } from "./endpointInfo";
import type { Rng } from "./prng";
import { event, type RuntimeEvent } from "./runtimeFactory";
import {
  type ChannelRuntime,
  type DerivedStates,
  type EndpointRuntime,
  pushError,
} from "./runtimeState";
import { randomFault } from "./scenario";

export interface FaultContext {
  document: ConfigDocument;
  derived: DerivedStates;
  endpoints: Map<string, EndpointRuntime>;
  channels: Map<string, ChannelRuntime>;
  rng: Rng;
}

/** The endpoint works again: no fault, no streak, no retry waiting. */
export function clearFault(runtime: EndpointRuntime): void {
  runtime.fault = null;
  runtime.failureStreak = 0;
  runtime.nextRetryMs = null;
}

/** Ends short-lived faults whose time has come. */
export function expireFaults(
  document: ConfigDocument,
  endpoints: Map<string, EndpointRuntime>,
  now: number,
): RuntimeEvent[] {
  const events: RuntimeEvent[] = [];
  for (const endpoint of document.endpoints) {
    const runtime = endpoints.get(endpoint.id);
    const until = runtime?.fault?.untilMs;
    if (runtime === undefined || until === null || until === undefined || until > now) continue;
    clearFault(runtime);
    runtime.lastAttemptMs = null;
    events.push(event("info", null, endpoint.id, `endpoint recovered: ${endpoint.label}`, {}));
  }
  return events;
}

function canFault(endpoint: EndpointConfig, context: FaultContext): boolean {
  const { document, derived, endpoints } = context;
  const runtime = endpoints.get(endpoint.id);
  if (runtime === undefined || runtime.fault !== null || !endpoint.enabled) return false;
  const users = channelsUsing(document, endpoint.id);
  return users.length > 0 && users.every((c) => derived.raw.get(c.id) === "active");
}

/** Now and then a healthy endpoint fails for a while, to make the dashboard move. */
export function maybeStartFault(context: FaultContext, now: number): RuntimeEvent[] {
  const { document, endpoints, channels, rng } = context;
  const volatile = [...endpoints.values()].filter((e) => e.fault?.untilMs != null).length;
  if (volatile >= 2 || !rng.chance(0.02)) return [];
  const candidates = document.endpoints.filter((endpoint) => canFault(endpoint, context));
  if (candidates.length === 0) return [];
  const endpoint = rng.pick(candidates);
  const runtime = endpoints.get(endpoint.id);
  if (runtime === undefined) return [];
  runtime.fault = randomFault(endpoint, now, rng);
  runtime.lastError = runtime.fault.error;
  runtime.failureStreak = 1;
  runtime.nextRetryMs = null;
  for (const channel of channelsUsing(document, endpoint.id)) {
    const channelRuntime = channels.get(channel.id);
    if (channelRuntime === undefined) continue;
    channelRuntime.lastError = runtime.fault.error;
    pushError(channelRuntime.errors, runtime.fault.error);
  }
  const level = runtime.fault.state === "error" ? "error" : "warn";
  return [
    event(level, null, endpoint.id, runtime.fault.error.summary, { state: runtime.fault.state }),
  ];
}
