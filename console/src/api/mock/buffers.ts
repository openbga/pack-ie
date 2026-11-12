import type { BridgeChannelConfig, ChannelState } from "../types";
import type { Rng } from "./prng";
import { event, type RuntimeEvent } from "./runtimeFactory";
import { type ChannelRuntime, type ErrorInfo, pushError } from "./runtimeState";
import { clamp, SECOND } from "./util";

const FULL = 0.97;

export function overflowError(side: "a" | "b", atMs: number): ErrorInfo {
  return {
    atMs,
    summary: `buffer overflow ${side === "a" ? "A to B" : "B to A"}: the receiving side is not keeping up`,
    nextAction:
      "Check the receiving system. The sender is held back and no data is lost; the channel recovers when the buffer drains.",
  };
}

/** Moves one buffer toward a level, as a share of the distance, with a little noise. */
function approach(level: number, target: number, rate: number, noise: number): number {
  return clamp(level + (target - level) * rate + noise, 0, 0.99);
}

function drifted(
  level: number,
  blocked: "held" | "draining" | null,
  stuck: boolean,
  rng: Rng,
): number {
  if (blocked === "held") return FULL + rng.float() * 0.02;
  if (blocked === "draining") return approach(level, 0.1, 0.25, 0);
  const noise = (rng.float() - 0.5) * 0.04;
  return stuck ? approach(level, 0.85, 0.03, noise) : approach(level, 0.12, 0.15, noise);
}

/**
 * One second of buffer movement. Buffers fill while a side is failing, are pinned full during
 * an overflow episode, drain to nothing once the channel is stopped, and idle low otherwise.
 */
export function driftBuffers(
  runtime: ChannelRuntime,
  state: ChannelState,
  faulty: boolean,
  rng: Rng,
  now: number,
): void {
  const episode = runtime.overflow;
  const mode = (side: "a" | "b") => {
    if (episode?.side !== side) return null;
    return now < episode.untilMs ? "held" : "draining";
  };
  if (state === "stopped") {
    runtime.bufferA = approach(runtime.bufferA, 0, 0.3, 0);
    runtime.bufferB = approach(runtime.bufferB, 0, 0.3, 0);
    return;
  }
  runtime.bufferA = drifted(runtime.bufferA, mode("a"), faulty && state === "retrying", rng);
  runtime.bufferB = drifted(runtime.bufferB, mode("b"), false, rng);
}

/** Starts and ends overflow episodes (bridge-channels.md section 6). */
export function tickOverflow(
  channel: BridgeChannelConfig,
  runtime: ChannelRuntime,
  state: ChannelState,
  rng: Rng,
  now: number,
): RuntimeEvent[] {
  const episode = runtime.overflow;
  if (episode === null) {
    if (state !== "running" || !rng.chance(0.001)) return [];
    const side = rng.chance(0.7) ? "a" : "b";
    runtime.overflow = { side, untilMs: now + rng.int(20, 60) * SECOND };
    runtime.overflowCount += 1;
    const error = overflowError(side, now);
    pushError(runtime.errors, error);
    runtime.lastError = error;
    return [event("warn", channel.id, null, "buffer overflow detected", { side })];
  }
  const level = episode.side === "a" ? runtime.bufferA : runtime.bufferB;
  if (state === "stopped" || (now >= episode.untilMs && level <= 0.5)) {
    runtime.overflow = null;
    if (state === "stopped") return [];
    return [event("info", channel.id, null, "buffer overflow ended", { side: episode.side })];
  }
  return [];
}
