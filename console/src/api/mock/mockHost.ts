import type { Unsubscribe } from "../engine";
import type { ConnectionStatus } from "../types";
import { MOCK_ACTOR } from "./audit";
import type { NotificationsMode, NotificationTarget } from "./notifications";
import { Rng } from "./prng";
import { iso } from "./util";
import type { MockWorld } from "./world";

const SERVICE_START_MS = 1000;
const MOCK_ENDPOINT = "mock://engine";

/** How requests reach the mock engine. */
export interface MockLink {
  latency: "realistic" | "none";
  /** A fixed latency in milliseconds for every request, or null. */
  fixedLatencyMs: number | null;
  failReads: boolean;
  now: () => number;
  seed: number;
}

/**
 * The simulated link between the console and the mock engine: request latency, the connection
 * (starting the engine, outages), and the hooks tests and journeys use to make the engine misbehave.
 */
export abstract class MockHost {
  private since: string;
  private readonly connectionListeners = new Set<(status: ConnectionStatus) => void>();
  private outage = false;
  protected serialPortsFail = false;
  private readonly latencyRng: Rng;

  constructor(
    protected readonly world: MockWorld,
    private readonly link: MockLink,
  ) {
    this.latencyRng = new Rng(link.seed ^ 0x1a7e);
    this.since = iso(link.now());
  }

  /** Stops every timer the mock started. Tests call it; the app never needs to. */
  dispose(): void {
    this.connectionListeners.clear();
    this.world.dispose();
  }

  protected pause(ms: number): Promise<void> {
    if (this.link.latency === "none") return Promise.resolve();
    return new Promise((resolve) => setTimeout(resolve, this.link.fixedLatencyMs ?? ms));
  }

  /** Runs `run` after the simulated latency; rejects `engine_unavailable` while offline. */
  protected async request<T>(run: () => T | Promise<T>, whileOffline = false): Promise<T> {
    await this.pause(this.latencyRng.int(80, 300));
    if (!whileOffline && this.link.failReads) {
      this.world.fail("internal", "The mock engine was asked to fail every request.");
    }
    if (!whileOffline && this.outage) {
      this.world.fail("engine_unavailable", "The connection to the engine was lost.");
    }
    if (!whileOffline && !this.world.online) {
      this.world.fail("engine_unavailable", "The engine is not running.");
    }
    return run();
  }

  // Connection

  private connectionStatus(): ConnectionStatus {
    if (this.outage) {
      return {
        state: "reconnecting",
        endpoint: MOCK_ENDPOINT,
        detail: "The connection to the engine was lost.",
        since: this.since,
        actor: MOCK_ACTOR.name,
      };
    }
    if (this.world.online) {
      return {
        state: "connected",
        endpoint: MOCK_ENDPOINT,
        detail: null,
        since: this.since,
        actor: MOCK_ACTOR.name,
      };
    }
    return {
      state: "disconnected",
      endpoint: MOCK_ENDPOINT,
      detail: "No engine is running (demo data).",
      since: this.since,
      actor: null,
    };
  }

  getConnection(): Promise<ConnectionStatus> {
    return this.request(() => this.connectionStatus(), true);
  }

  subscribeConnection(onStatus: (status: ConnectionStatus) => void): Unsubscribe {
    this.connectionListeners.add(onStatus);
    onStatus(this.connectionStatus());
    return () => {
      this.connectionListeners.delete(onStatus);
    };
  }

  async startService(): Promise<string> {
    if (this.world.online) return "The engine is already running.";
    await this.pause(SERVICE_START_MS);
    if (this.world.online) return "The engine is already running.";
    this.world.online = true;
    this.since = iso(this.link.now());
    this.world.publish();
    const status = this.connectionStatus();
    for (const listener of [...this.connectionListeners]) listener(status);
    return "Started the demo engine.";
  }

  /** For journeys: the engine cannot list the serial ports (or can again). */
  failSerialPorts(fail: boolean): void {
    this.serialPortsFail = fail;
  }

  /** Test hook: the next apply or rollback ends `partial`, `channelId` failing with `reason`. */
  failNextApply(channelId: string, reason: string): void {
    this.world.changes.failNextApply(channelId, reason);
  }

  /**
   * For tests: the console loses the connection to the engine (state `reconnecting`, requests
   * fail). The engine keeps logging, and followers of the log miss what it logs meanwhile.
   */
  simulateOutage(): void {
    if (this.outage) return;
    this.outage = true;
    this.world.logs.disconnect();
    this.announceConnection();
  }

  /**
   * For tests: the connection returns. Followers are given the events they missed, once and
   * oldest first, with a notice; `restarted` makes the engine's log start again from `seq` 1.
   */
  endOutage(options: { restarted?: boolean } = {}): void {
    if (!this.outage) return;
    if (options.restarted) this.world.logs.restart();
    this.outage = false;
    this.world.logs.reconnect();
    this.announceConnection();
  }

  /** For tests: the engine logs `count` more events now, as it does between ticks. */
  publishLogEvents(count: number): void {
    this.world.logs.publish(count);
  }

  private announceConnection(): void {
    this.since = iso(this.link.now());
    const status = this.connectionStatus();
    for (const listener of [...this.connectionListeners]) listener(status);
  }

  /** Journey hooks: the audit log and backups going wrong, which the engine cannot be made to do on demand. */
  setAuditAvailable(available: boolean): void {
    this.world.audit.setAvailable(available);
    this.world.publish();
  }

  breakAuditChain(seq: number): void {
    this.world.audit.breakChain(seq);
  }

  pruneAudit(count: number): void {
    this.world.audit.prune(count);
  }

  makeBackupStale(hours: number): void {
    this.world.backup.ageNewestSuccess(hours);
    this.world.publish();
  }

  failNextBackup(error: string): void {
    this.world.backup.failNextBackup(error);
  }

  /** Sends to `target` fail with `error` and the alert opens at once; null makes every target work again (the alert clears with the next successful test). */
  failNotificationTarget(target: NotificationTarget | null, error?: string): void {
    this.world.notifications.breakTarget(target, error);
    this.world.publish();
  }

  /** What the console reads of the bootstrap's notifications, and which targets the engine has. */
  setNotificationsConfigured(mode: NotificationsMode): void {
    this.world.notifications.setMode(mode);
    this.world.publish();
  }

  /** A newer revision of the message arrives, as from another client: the id of the revision. */
  reviseMessage(id: string): string | null {
    return this.world.messages.reviseNow(id);
  }
}
