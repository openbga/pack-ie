import type { ConfigDocument } from "../types";
import { channelsUsing } from "./endpointInfo";
import type { Rng } from "./prng";
import { event, type RuntimeEvent, retryDelayMs } from "./runtimeFactory";
import {
  type ChannelRuntime,
  type DerivedStates,
  type EndpointRuntime,
  pushError,
} from "./runtimeState";

export interface RetryContext {
  document: ConfigDocument;
  derived: DerivedStates;
  endpoints: Map<string, EndpointRuntime>;
  channels: Map<string, ChannelRuntime>;
  rng: Rng;
  now: number;
}

/** Whether some channel that uses the endpoint is running (or starting) it. */
function isRun(ctx: RetryContext, endpointId: string): boolean {
  return channelsUsing(ctx.document, endpointId).some(
    (channel) => ctx.derived.raw.get(channel.id) !== "stopped",
  );
}

/** A failed attempt lands in the history of every running channel that uses the endpoint. */
function recordAttempt(ctx: RetryContext, endpointId: string, runtime: EndpointRuntime): void {
  const error = runtime.fault?.error;
  if (error === undefined) return;
  for (const channel of channelsUsing(ctx.document, endpointId)) {
    const state = ctx.channels.get(channel.id);
    if (state === undefined || ctx.derived.raw.get(channel.id) === "stopped") continue;
    pushError(state.errors, { ...error, atMs: ctx.now });
    state.counters.retries += 1;
  }
}

function settleHealthy(runtime: EndpointRuntime, listenerLost: boolean): void {
  runtime.nextRetryMs = null;
  runtime.failureStreak = listenerLost ? Math.max(runtime.failureStreak, 1) : 0;
}

/**
 * Keeps failure streaks and retry times of faulty endpoints current: while an endpoint is
 * `retrying` or `error` a retry is scheduled a few seconds ahead; once its time has passed
 * the attempt fails again, the streak grows and the next one is scheduled. A listener that
 * lost its peer has a streak of one and no retry (it waits). Anything else clears.
 */
export function advanceRetries(ctx: RetryContext): RuntimeEvent[] {
  const events: RuntimeEvent[] = [];
  for (const endpoint of ctx.document.endpoints) {
    const runtime = ctx.endpoints.get(endpoint.id);
    const state = ctx.derived.endpoints.get(endpoint.id);
    if (runtime === undefined || state === undefined) continue;
    const failing = runtime.fault !== null && (state === "retrying" || state === "error");
    if (!failing) {
      settleHealthy(runtime, runtime.fault !== null && isRun(ctx, endpoint.id));
      continue;
    }
    if (runtime.nextRetryMs === null) {
      runtime.failureStreak = Math.max(runtime.failureStreak, 1);
      runtime.nextRetryMs = ctx.now + retryDelayMs(runtime.failureStreak, ctx.rng);
    } else if (runtime.nextRetryMs <= ctx.now) {
      runtime.failureStreak += 1;
      runtime.lastAttemptMs = ctx.now;
      runtime.nextRetryMs = ctx.now + retryDelayMs(runtime.failureStreak, ctx.rng);
      recordAttempt(ctx, endpoint.id, runtime);
      events.push(
        event("warn", null, endpoint.id, `retry failed: ${runtime.fault?.error.summary ?? ""}`, {
          retry_count: String(runtime.failureStreak),
        }),
      );
    }
  }
  return events;
}
