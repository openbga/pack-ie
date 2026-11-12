import type { ChannelConfig, ChannelState, ConfigDocument, EndpointState } from "../types";
import { driftBuffers, tickOverflow } from "./buffers";
import { metaFor } from "./channelMeta";
import { channelEndpointIds } from "./endpointInfo";
import { clearFault, expireFaults, maybeStartFault } from "./faults";
import { hashString, Rng } from "./prng";
import { advanceRetries } from "./retries";
import {
  addCounters,
  type CommandKind,
  type CommandOutcome,
  event,
  NOT_FOUND,
  newChannelRuntime,
  newEndpointRuntime,
  RESTART_MS,
  type RuntimeEvent,
  refuse,
  resetInstance,
  zeroCounters,
} from "./runtimeFactory";
import {
  type ChannelRuntime,
  type DerivedStates,
  deriveStates,
  type EndpointRuntime,
  type TrafficDelta,
} from "./runtimeState";
import { bridgeDelta, creditEndpoints, settleReceived } from "./trafficBooks";
import { clamp, HOUR } from "./util";

/** The mutable, time-driven part of the synthetic engine: states, counters, faults. */
export class Runtime {
  readonly channels = new Map<string, ChannelRuntime>();
  readonly endpoints = new Map<string, EndpointRuntime>();
  readonly disk = { totalBytes: 512e9, freeBytes: 65e9, databaseBytes: 3.4e9 };
  private readonly rng: Rng;
  /** Retries are scheduled on whenever a snapshot is taken, so they get a generator of their own. */
  private readonly retryRng: Rng;
  private queued: RuntimeEvent[] = [];

  constructor(
    private readonly now: () => number,
    seed: number,
    /** No endpoint ever fails by chance: only what the scenario put there happens. */
    private readonly steady = false,
  ) {
    this.rng = new Rng(seed ^ 0x51ed);
    this.retryRng = new Rng(seed ^ 0x7e7);
  }

  /** Builds runtime records for a whole document, with counter history. */
  init(document: ConfigDocument): void {
    const now = this.now();
    for (const endpoint of document.endpoints) {
      const runtime = newEndpointRuntime(now, this.seededFor(endpoint.id));
      runtime.lastActivityMs = now - this.rng.int(1, 60) * 1000;
      this.endpoints.set(endpoint.id, runtime);
    }
    for (const channel of document.channels) {
      const runtime = newChannelRuntime(
        channel,
        this.seededFor(channel.id),
        true,
        now,
        document.version,
      );
      runtime.lastActivityMs = now - this.rng.int(1, 60) * 1000;
      this.channels.set(channel.id, runtime);
      creditEndpoints(this.endpoints, channel, runtime.counters, now, false);
    }
  }

  private seededFor(id: string): Rng {
    return new Rng(hashString(id) ^ 0xa5a5);
  }

  /** Brings runtime records in line with an applied document. */
  sync(document: ConfigDocument, restartedIds: string[], changedEndpointIds: string[]): void {
    const now = this.now();
    const endpointIds = new Set(document.endpoints.map((e) => e.id));
    const channelIds = new Set(document.channels.map((c) => c.id));
    for (const id of [...this.endpoints.keys()])
      if (!endpointIds.has(id)) this.endpoints.delete(id);
    for (const id of [...this.channels.keys()]) if (!channelIds.has(id)) this.channels.delete(id);
    for (const endpoint of document.endpoints) {
      const existing = this.endpoints.get(endpoint.id);
      if (existing === undefined)
        this.endpoints.set(endpoint.id, newEndpointRuntime(now, this.rng));
      else if (changedEndpointIds.includes(endpoint.id)) clearFault(existing);
    }
    for (const channel of document.channels) {
      if (this.channels.has(channel.id)) continue;
      const runtime = newChannelRuntime(channel, this.rng, false, now, document.version);
      runtime.startingUntil = now + RESTART_MS;
      this.channels.set(channel.id, runtime);
    }
    for (const id of restartedIds) this.restartRuntime(id, now, document.version);
  }

  /** A restart from an applied configuration: a new instance, unless the channel is paused. */
  private restartRuntime(id: string, now: number, version: number): void {
    const runtime = this.channels.get(id);
    if (runtime === undefined) return;
    if (runtime.stop === "configuration" || runtime.stop === "fatal") {
      runtime.stop = null;
      runtime.stopError = null;
    }
    this.startInstance(runtime, now, version);
  }

  /** A new instance starts from the active definition and reports `starting` for a moment. */
  private startInstance(runtime: ChannelRuntime, now: number, version: number): void {
    resetInstance(runtime, now, version);
    runtime.startingUntil = now + RESTART_MS;
  }

  /**
   * Records the current states as the starting point: endpoints keep their `stateSince`, and
   * anything that is not running last saw activity before it went quiet.
   */
  settle(document: ConfigDocument): void {
    const now = this.now();
    const derived = this.deriveAt(document, now);
    for (const [id, state] of derived.endpoints) this.settleEndpoint(id, state, now);
    for (const channel of document.channels) {
      const state = derived.channels.get(channel.id)?.state;
      if (state !== undefined && state !== "running") this.quieten(channel, now);
    }
  }

  private settleEndpoint(id: string, state: EndpointState, now: number): void {
    const runtime = this.endpoints.get(id);
    if (runtime === undefined) return;
    if (runtime.fault === null) runtime.lastState = state;
    if (state !== "connected" && state !== "listening") {
      runtime.lastActivityMs = (runtime.fault?.error.atMs ?? now - 3 * HOUR) - 4000;
    }
  }

  /** A channel that is not running last saw traffic shortly before its first problem. */
  private quieten(channel: ChannelConfig, now: number): void {
    const runtime = this.channels.get(channel.id);
    if (runtime === undefined) return;
    const times = channelEndpointIds(channel)
      .map((id) => this.endpoints.get(id)?.fault?.error.atMs)
      .filter((at): at is number => at !== undefined);
    if (runtime.stopError !== null) times.push(runtime.stopError.atMs);
    runtime.lastActivityMs = (times.length > 0 ? Math.min(...times) : now - 3 * HOUR) - 4000;
  }

  private deriveAt(document: ConfigDocument, now: number): DerivedStates {
    return deriveStates(document.channels, document.endpoints, this.channels, this.endpoints, now);
  }

  /** States right now; also records when an endpoint's state changed. */
  derive(document: ConfigDocument): DerivedStates {
    const now = this.now();
    const derived = this.deriveAt(document, now);
    for (const [id, state] of derived.endpoints) {
      const runtime = this.endpoints.get(id);
      if (runtime !== undefined && runtime.lastState !== state) {
        runtime.lastState = state;
        runtime.stateSince = now;
      }
    }
    this.queued.push(
      ...advanceRetries({
        document,
        derived,
        endpoints: this.endpoints,
        channels: this.channels,
        rng: this.retryRng,
        now,
      }),
    );
    return derived;
  }

  command(kind: CommandKind, id: string, document: ConfigDocument): CommandOutcome {
    const config = document.channels.find((channel) => channel.id === id);
    const runtime = this.channels.get(id);
    if (config === undefined || runtime === undefined) {
      return { ok: false, code: "not_found", message: NOT_FOUND };
    }
    const current = this.derive(document).channels.get(id);
    const refusal = refuse(kind, config, current?.state ?? "running", current?.stopReason ?? null);
    if (refusal !== null) return { ok: false, code: "conflict", message: refusal };
    const now = this.now();
    if (kind === "pause") this.pauseRuntime(runtime);
    else if (kind === "resume") this.resumeRuntime(runtime, now, document.version);
    else this.restartRuntime(id, now, document.version);
    const verb = { pause: "paused", resume: "resumed", restart: "restarted" }[kind];
    return {
      ok: true,
      events: [event("info", id, null, `channel ${verb} by operator`, { command: kind })],
    };
  }

  /** The session closes, the buffers go and no retry is scheduled: states derive from `stop`. */
  private pauseRuntime(runtime: ChannelRuntime): void {
    runtime.stop = "paused";
    runtime.overflow = null;
  }

  private resumeRuntime(runtime: ChannelRuntime, now: number, version: number): void {
    runtime.stop = null;
    this.startInstance(runtime, now, version);
  }

  /** Advances the simulation by one second of activity. Returns events worth logging. */
  tick(document: ConfigDocument): RuntimeEvent[] {
    const now = this.now();
    const derived = this.derive(document);
    const events = this.queued.splice(0);
    for (const channel of document.channels) {
      const runtime = this.channels.get(channel.id);
      const state = derived.channels.get(channel.id)?.state;
      if (runtime !== undefined && state !== undefined) {
        events.push(...this.tickChannel(channel, runtime, state, document, derived, now));
      }
    }
    for (const endpoint of document.endpoints) {
      if (endpoint.kind === "tcp_listener") this.tickPeer(endpoint.id);
    }
    this.disk.freeBytes = Math.max(0, this.disk.freeBytes - this.rng.int(0, 120_000));
    this.disk.databaseBytes += this.rng.int(0, 120_000);
    events.push(
      ...expireFaults(document, this.endpoints, now),
      ...(this.steady
        ? []
        : maybeStartFault(
            {
              document,
              derived,
              endpoints: this.endpoints,
              channels: this.channels,
              rng: this.rng,
            },
            now,
          )),
    );
    return events;
  }

  /** Instruments attach quickly and stay attached for minutes. */
  private tickPeer(endpointId: string): void {
    const runtime = this.endpoints.get(endpointId);
    if (runtime === undefined) return;
    if (this.rng.chance(runtime.peerConnected ? 0.004 : 0.03)) {
      runtime.peerConnected = !runtime.peerConnected;
    }
  }

  private tickChannel(
    channel: ChannelConfig,
    runtime: ChannelRuntime,
    state: ChannelState,
    document: ConfigDocument,
    derived: DerivedStates,
    now: number,
  ): RuntimeEvent[] {
    // While a console watches the transcript, the tap is the source of traffic.
    const ingesting =
      (state === "running" || (state === "degraded" && channel.kind === "message")) &&
      runtime.transcriptViewers === 0;
    if (ingesting) {
      const rate = metaFor(channel.id).rate;
      const count = Math.floor(rate * (0.5 + this.rng.float()) + this.rng.float());
      if (count > 0) this.addTraffic(channel, runtime, count, now);
    }
    if (channel.kind === "message") {
      this.driftQueue(runtime, document.alertThresholds.queueDepthWarning);
      return [];
    }
    const faulty = channelEndpointIds(channel).some((id) => {
      const endpoint = derived.endpoints.get(id);
      return endpoint === "retrying" || endpoint === "error";
    });
    driftBuffers(runtime, state, faulty, this.rng, now);
    return tickOverflow(channel, runtime, state, this.rng, now);
  }

  private addTraffic(channel: ChannelConfig, runtime: ChannelRuntime, count: number, now: number) {
    const bytes = count * this.rng.int(300, 900);
    if (channel.kind === "bridge") {
      this.creditTraffic(channel, bridgeDelta(channel, runtime, count, bytes, this.rng), now);
      return;
    }
    this.creditTraffic(
      channel,
      {
        messagesIn: count,
        bytesIn: bytes,
        messagesOut: count * channel.destinations.length,
        bytesOut: Math.floor(bytes * 0.9),
      },
      now,
    );
  }

  /**
   * Books traffic on a channel and its endpoints: the background simulation and the
   * transcript tap both come through here, so counters match what a viewer sees. A bridge
   * without a tap framer counts bytes only.
   */
  creditTraffic(channel: ChannelConfig, traffic: TrafficDelta, now: number): void {
    const runtime = this.channels.get(channel.id);
    if (runtime === undefined) return;
    const framed = channel.kind === "message" || channel.tapFramer !== "none";
    const delta = framed ? traffic : { ...traffic, messagesIn: 0, messagesOut: 0 };
    addCounters(runtime.counters, delta);
    runtime.lastActivityMs = now;
    if (channel.kind === "message") settleReceived(channel, runtime, delta.messagesIn, this.rng);
    creditEndpoints(this.endpoints, channel, { ...zeroCounters(), ...delta }, now, true);
  }

  private driftQueue(runtime: ChannelRuntime, depthLimit: number): void {
    const noise = this.rng.float() - 0.5;
    const target = runtime.queueBase;
    const next =
      runtime.queueDepth + (target - runtime.queueDepth) * 0.2 + noise * Math.max(6, target * 0.02);
    runtime.queueDepth = clamp(Math.round(next), 0, depthLimit * 2);
  }
}
