import type { ConfigDocument, EndpointConfig } from "../types";
import { overflowError } from "./buffers";
import { channelsUsing } from "./endpointInfo";
import { hashString, type Rng } from "./prng";
import { resetInstance } from "./runtimeFactory";
import {
  type ChannelRuntime,
  type EndpointFault,
  type EndpointRuntime,
  type ErrorInfo,
  pushError,
  RECENT_ERRORS,
} from "./runtimeState";
import { HOUR, MINUTE, SECOND } from "./util";

interface EndpointScenario {
  id: string;
  state: EndpointFault["state"];
  agoMs: number;
  summary: string;
  nextAction: string;
}

/** Endpoints that are unhealthy when the mock starts, with the error the operator sees. */
const ENDPOINT_FAULTS: EndpointScenario[] = [
  {
    id: "ep-lis-backup",
    state: "error",
    agoMs: 47 * MINUTE,
    summary: "connection refused (10.0.4.21:2575)",
    nextAction:
      "Check that the standby LIS interface is running on 10.0.4.21, or remove this destination.",
  },
  {
    id: "ep-mw-2",
    state: "retrying",
    agoMs: 6 * MINUTE,
    summary: "connect timeout after 5000 ms (10.0.4.12:3002)",
    nextAction:
      "Check that the middleware server is reachable. The channel keeps retrying with backoff.",
  },
  {
    id: "ep-a-acltop",
    state: "error",
    agoMs: 2 * HOUR,
    summary: "COM9: port not found",
    nextAction:
      "Check the USB-serial adapter of the ACL TOP 550 and the port number in Windows Device Manager.",
  },
  {
    id: "ep-reflab-sftp",
    state: "error",
    agoMs: 3 * HOUR,
    summary: "SFTP authentication failed for user labxfer",
    nextAction:
      "Re-enter the password under Settings, Secrets (reflab-sftp-password), then restart the channel.",
  },
  {
    id: "ep-xn2000",
    state: "error",
    agoMs: 12 * MINUTE,
    summary: "cannot accept connections: 10.0.4.5:5003 is already in use",
    nextAction:
      "Find the program that uses port 5003 on this computer and stop it, then restart the channel.",
  },
  {
    id: "ep-dashboard",
    state: "retrying",
    agoMs: 90 * SECOND,
    summary: "connection reset by peer (10.0.5.20:8081)",
    nextAction: "No action needed unless this persists; check the dashboard service.",
  },
];

interface ScenarioTarget {
  endpoints: Map<string, EndpointRuntime>;
  channels: Map<string, ChannelRuntime>;
}

/**
 * Failed attempts so far for a fault that began `agoMs` ago, one every ~30 s, and their errors
 * newest first: the retry history channels using the endpoint show.
 */
function retryHistory(
  runtime: EndpointRuntime,
  error: ErrorInfo,
  agoMs: number,
  now: number,
): ErrorInfo[] {
  runtime.failureStreak = Math.min(99, Math.max(1, Math.ceil(agoMs / (30 * SECOND))));
  const jitter = hashString(error.summary);
  runtime.lastAttemptMs = now - 2000 - (jitter % 10_000);
  runtime.nextRetryMs = now + 1500 + (jitter % 6000);
  const count = Math.min(runtime.failureStreak, RECENT_ERRORS);
  const step = Math.min(60 * SECOND, agoMs / count);
  const history: ErrorInfo[] = [];
  for (let k = 0; k < count; k++) {
    history.push({ ...error, atMs: Math.max(error.atMs, runtime.lastAttemptMs - k * step) });
  }
  return history;
}

function seedFaults(target: ScenarioTarget, document: ConfigDocument, now: number): void {
  for (const item of ENDPOINT_FAULTS) {
    const runtime = target.endpoints.get(item.id);
    if (runtime === undefined) continue;
    const error = { atMs: now - item.agoMs, summary: item.summary, nextAction: item.nextAction };
    runtime.fault = { state: item.state, error, untilMs: null };
    runtime.lastError = error;
    runtime.lastState = item.state;
    runtime.stateSince = error.atMs;
    const history = retryHistory(runtime, error, item.agoMs, now);
    for (const channel of channelsUsing(document, item.id)) {
      const channelRuntime = target.channels.get(channel.id);
      if (channelRuntime === undefined) continue;
      channelRuntime.errors = [...history];
      channelRuntime.lastError = error;
    }
  }
}

/** One bridge is blocked on its A to B buffer: an overflow episode under way. */
function seedOverflow(runtime: ChannelRuntime | undefined, now: number): void {
  if (runtime === undefined) return;
  const startedAt = now - 3 * MINUTE;
  runtime.bufferA = 0.99;
  runtime.overflow = { side: "a", untilMs: now + 45 * SECOND };
  runtime.overflowCount = 1;
  const error = overflowError("a", startedAt);
  pushError(runtime.errors, error);
  runtime.lastError = error;
}

/** Puts the mock into its demo state: some channels failing, stopped or starting. */
export function applyScenario(target: ScenarioTarget, document: ConfigDocument, now: number): void {
  seedFaults(target, document, now);
  const channel = (id: string) => target.channels.get(id);

  setAttention(channel("ch-msg-sta"), 14, 1200);
  setAttention(channel("ch-msg-reflab"), 22, 40);
  setAttention(channel("ch-msg-dashboard"), 0, 18);
  const retryBuffer = channel("ch-br-c503");
  if (retryBuffer !== undefined) retryBuffer.bufferA = 0.55;
  seedOverflow(channel("ch-br-im1600"), now);

  const paused = channel("ch-msg-vitros350");
  if (paused !== undefined) paused.stop = "paused";
  stopWithError(channel("ch-msg-outreach"), "configuration", {
    atMs: now - 25 * MINUTE,
    summary: "cannot listen on 10.0.4.5:2590: address already in use",
    nextAction:
      "Free port 2590 or change the listener port in the endpoint, then apply the configuration.",
  });
  stopWithError(channel("ch-br-d100"), "fatal", {
    atMs: now - 4 * HOUR,
    summary: "internal error in the session supervisor; the channel was stopped",
    nextAction:
      "Restart the channel. If it stops again, send the log lines of this channel to support.",
  });
  const starting = channel("ch-br-u411");
  if (starting !== undefined) {
    resetInstance(starting, now, document.version);
    starting.startingUntil = now + 35 * SECOND;
  }
}

function setAttention(
  runtime: ChannelRuntime | undefined,
  needsAttention: number,
  depth: number,
): void {
  if (runtime === undefined) return;
  runtime.needsAttention = needsAttention;
  runtime.queueBase = depth;
  runtime.queueDepth = depth;
}

function stopWithError(
  runtime: ChannelRuntime | undefined,
  reason: "configuration" | "fatal",
  error: ErrorInfo,
): void {
  if (runtime === undefined) return;
  runtime.stop = reason;
  runtime.stopError = error;
  runtime.lastError = error;
  pushError(runtime.errors, error);
}

/** A short-lived failure for a random healthy endpoint, to make the dashboard move. */
export function randomFault(endpoint: EndpointConfig, now: number, rng: Rng): EndpointFault {
  const untilMs = now + rng.int(20, 60) * SECOND;
  const make = (state: EndpointFault["state"], summary: string, nextAction: string) => ({
    state,
    untilMs,
    error: { atMs: now, summary, nextAction },
  });
  switch (endpoint.kind) {
    case "serial":
      return make(
        "error",
        `${endpoint.port}: access denied (port in use by another program)`,
        "Close the other program that opened the port; the channel recovers by itself.",
      );
    case "tcp_client":
      return make(
        "retrying",
        `connection refused (${endpoint.host}:${endpoint.port})`,
        "Check that the remote service is running. The channel keeps retrying with backoff.",
      );
    case "tcp_listener":
      return make(
        "disconnected",
        "peer connection lost: connection reset",
        "Check the sending instrument and its network cable. The listener accepts a new connection.",
      );
    case "folder":
      return make(
        "error",
        `cannot read folder ${endpoint.path}: access denied`,
        "Check the permissions of the service account on this folder.",
      );
    case "sftp":
      return make(
        "retrying",
        `SFTP connection to ${endpoint.host} timed out`,
        "No action needed unless this persists; check the network path to the partner.",
      );
  }
}
