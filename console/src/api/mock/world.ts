import type { Unsubscribe } from "../engine";
import type {
  Alert,
  ChannelDetail,
  EndpointDetail,
  ErrorCode,
  FieldError,
  LogPage,
  LogQuery,
  Snapshot,
  TrafficSegment,
} from "../types";
import { alertConditions } from "./alertRules";
import { type AlertCondition, AlertStore, buildAlertHistory } from "./alerts";
import { MockAudit } from "./audit";
import { MockBackup } from "./backup";
import { buildInitialDocument, emptyDocument } from "./channelCatalog";
import { metaFor } from "./channelMeta";
import { ConfigChanges } from "./configChanges";
import { ConfigStore } from "./configStore";
import { channelDetail, endpointDetail } from "./details";
import { findChannel } from "./endpointInfo";
import { buildLogChannels } from "./logGenerator";
import { LogStore } from "./logStore";
import { MessageStore } from "./messageStore";
import { MockNotifications } from "./notifications";
import { hashString, Rng } from "./prng";
import { Runtime } from "./runtime";
import type { CommandKind } from "./runtimeFactory";
import type { DerivedStates } from "./runtimeState";
import { applyScenario } from "./scenario";
import { SecretStore } from "./settings";
import { buildSnapshot, type SnapshotInputs } from "./snapshot";
import { startTraffic, styleOf, styleOfProtocol } from "./traffic";
import { DAY, HOUR, MINUTE, makeError } from "./util";

export interface WorldOptions {
  seed: number;
  messageCount: number;
  logRingSize: number;
  now: () => number;
  /** No endpoints, channels, messages or alerts, as on a fresh install. */
  empty?: boolean;
  /** No endpoint fails by chance (for repeatable journeys). */
  steady?: boolean;
}

const SNAPSHOT_INTERVAL_MS = 1000;
// Verbatim from the catalog in docs/spec/diagnostics.md section 6.
const AUDIT_UNAVAILABLE_NEXT_ACTION =
  "Check free disk space and the permissions of the audit directory (<state>/audit). Until the audit log can be written, commands that are audited (configuration changes, channel commands, resends, content views) are refused; message traffic continues.";
const BACKUP_STALE_NEXT_ACTION =
  "Check the backup directory (free space, permissions, mounted) and the last backup error (Settings → Backup or `packie backup status`), then run `packie backup create`. The alert clears after the next successful backup.";
const TRANSCRIPT_IN_USE = "Transcript in use by another client";

/** The synchronous core of the mock engine: all state lives here; `MockEngineApi` adds the transport. */
export class MockWorld {
  online = true;
  readonly config: ConfigStore;
  readonly runtime: Runtime;
  readonly alerts = new AlertStore();
  readonly logs: LogStore;
  readonly messages: MessageStore;
  readonly secrets = new SecretStore();
  readonly audit: MockAudit;
  readonly backup: MockBackup;
  readonly notifications: MockNotifications;
  readonly changes = new ConfigChanges(this);
  private readonly startedAtMs: number;
  private readonly errorIds: Rng;
  private readonly subscribers = new Set<(snapshot: Snapshot) => void>();
  private ticker: ReturnType<typeof setInterval> | null = null;
  private tapCounter = 0;
  private readonly stopTaps = new Set<Unsubscribe>();

  constructor(private readonly options: WorldOptions) {
    const { now, seed } = options;
    const initial = options.empty ? emptyDocument() : buildInitialDocument();
    this.startedAtMs = now() - 3 * DAY - 4 * HOUR;
    this.errorIds = new Rng(seed ^ 0xc0ffee);
    this.config = new ConfigStore(initial, now());
    this.runtime = new Runtime(now, seed, options.steady ?? false);
    this.runtime.init(initial);
    if (!options.empty) applyScenario(this.runtime, initial, now());
    this.runtime.settle(initial);
    if (!options.empty) this.alerts.seedHistory(buildAlertHistory(this.alerts, now()));
    this.logs = new LogStore(
      new Rng(seed ^ 0x10b),
      now,
      options.logRingSize,
      buildLogChannels(initial),
      options.messageCount,
    );
    this.messages = new MessageStore(seed, options.messageCount, initial, now, this.fail);
    this.audit = new MockAudit(seed, now, this.fail);
    this.backup = new MockBackup(
      now,
      options.messageCount,
      () => this.config.version,
      () => this.audit.headSeq,
      this.fail,
    );
    this.notifications = new MockNotifications(now);
    this.refresh(true);
  }

  now(): number {
    return this.options.now();
  }

  /** Rejects with a full `EngineError`. */
  readonly fail = (code: ErrorCode, message: string, fieldErrors: FieldError[] = []): never => {
    throw makeError(code, message, `c-${this.errorIds.hex(6)}`, fieldErrors);
  };

  // ── State and snapshots ─────────────────────────────────────────────────────────────

  /** Re-derives states and brings the alerts in line with them. */
  private refresh(initial = false): DerivedStates {
    const document = this.config.live;
    const now = this.now();
    const derived = this.runtime.derive(document);
    let conditions = [
      ...alertConditions(document, this.runtime, derived, this.alerts, now),
      ...this.nodeConditions(now),
    ];
    if (initial)
      conditions = conditions.map((c) => ({ ...c, sinceMs: c.sinceMs ?? now - 25 * MINUTE }));
    const change = this.alerts.reconcile(conditions, now);
    if (!initial) this.logAlerts(change.opened, change.cleared);
    return derived;
  }

  /** The audit log, backups and notifications raise node-level alerts (docs/spec/diagnostics.md section 6). */
  private nodeConditions(now: number): AlertCondition[] {
    const conditions: AlertCondition[] = [];
    if (!this.audit.available) {
      conditions.push({
        key: "audit_unavailable",
        kind: "audit_unavailable",
        severity: "critical",
        title: "The audit log cannot be written",
        detail: "",
        channelId: null,
        endpointId: null,
        nextAction: AUDIT_UNAVAILABLE_NEXT_ACTION,
      });
    }
    const stale = this.backup.staleDetail();
    if (stale !== null) {
      conditions.push({
        key: "backup_stale",
        kind: "backup_stale",
        severity: "warning",
        title:
          stale.newestSuccessAt === null
            ? `No backup has succeeded in the ${stale.limitHours} hours since the engine started`
            : `The newest successful backup is from ${stale.newestSuccessAt}, more than ${stale.limitHours} hours ago`,
        detail: `newest success at: ${stale.newestSuccessAt ?? "none"}, limit hours: ${stale.limitHours}`,
        channelId: null,
        endpointId: null,
        nextAction: BACKUP_STALE_NEXT_ACTION,
        sinceMs: now,
      });
    }
    const notifications = this.notifications.condition();
    if (notifications !== null) conditions.push(notifications);
    return conditions;
  }

  private logAlerts(opened: Alert[], cleared: Alert[]): void {
    for (const alert of opened) {
      this.logs.append({
        level: alert.severity === "critical" ? "error" : "warn",
        channelId: alert.channelId,
        endpointId: alert.endpointId,
        messageId: null,
        message: `alert opened: ${alert.title}`,
        fields: { alert_id: alert.id, kind: alert.kind },
      });
    }
    for (const alert of cleared) {
      this.logs.append({
        level: "info",
        channelId: alert.channelId,
        endpointId: alert.endpointId,
        messageId: null,
        message: `alert cleared: ${alert.title}`,
        fields: { alert_id: alert.id, kind: alert.kind },
      });
    }
  }

  private inputs(derived: DerivedStates): SnapshotInputs {
    return {
      document: this.config.live,
      runtime: this.runtime,
      derived,
      openAlerts: this.alerts.openAlerts(),
      nowMs: this.now(),
      startedAtMs: this.startedAtMs,
      messageCount: this.messages.count,
      pendingConfigVersion: this.changes.pendingVersion,
      node: { id: "lab-pc-01", name: "Main Lab Interface PC", version: "0.1.0" },
    };
  }

  snapshot(): Snapshot {
    return buildSnapshot(this.inputs(this.refresh()));
  }

  subscribe(onSnapshot: (snapshot: Snapshot) => void): Unsubscribe {
    this.subscribers.add(onSnapshot);
    queueMicrotask(() => {
      if (this.subscribers.has(onSnapshot) && this.online) onSnapshot(this.snapshot());
    });
    this.ticker ??= setInterval(() => this.tick(), SNAPSHOT_INTERVAL_MS);
    return () => {
      this.subscribers.delete(onSnapshot);
      if (this.subscribers.size === 0 && this.ticker !== null) {
        clearInterval(this.ticker);
        this.ticker = null;
      }
    };
  }

  private tick(): void {
    if (!this.online) return;
    for (const event of this.runtime.tick(this.config.live)) this.logs.append(event);
    this.publish();
  }

  publish(): void {
    if (!this.online || this.subscribers.size === 0) return;
    const snapshot = this.snapshot();
    for (const subscriber of [...this.subscribers]) subscriber(snapshot);
  }

  // ── Channels, endpoints, commands, traffic ──────────────────────────────────────────

  channelDetail(id: string): ChannelDetail {
    const config = findChannel(this.config.live, id);
    const detail = config ? channelDetail(config, this.inputs(this.refresh())) : null;
    return detail ?? this.fail("not_found", `Channel "${id}" does not exist.`);
  }

  endpointDetail(id: string): EndpointDetail {
    const detail = endpointDetail(id, this.inputs(this.refresh()));
    return detail ?? this.fail("not_found", `Endpoint "${id}" does not exist.`);
  }

  command(kind: CommandKind, id: string): void {
    const outcome = this.runtime.command(kind, id, this.config.live);
    if (!outcome.ok) this.fail(outcome.code, outcome.message);
    else for (const event of outcome.events) this.logs.append(event);
    this.publish();
  }

  /**
   * Turns the transcript on for a channel: one viewer at a time. What the tap shows is booked
   * on the channel's counters, and nothing flows while the channel is not running.
   */
  traffic(channelId: string, onSegments: (segments: TrafficSegment[]) => void): Unsubscribe {
    const config = findChannel(this.config.live, channelId);
    const runtime = this.runtime.channels.get(channelId);
    if (config === undefined || runtime === undefined) return () => {};
    if (runtime.transcriptViewers > 0) this.fail("conflict", TRANSCRIPT_IN_USE);
    runtime.transcriptViewers = 1;
    this.tapCounter += 1;
    const stopTimer = startTraffic({
      style:
        config.kind === "bridge"
          ? styleOf(config.tapFramer)
          : styleOfProtocol(config.source.protocol),
      message:
        config.kind === "message"
          ? {
              sourceEndpointId: config.source.endpointId,
              destinations: config.destinations
                .filter((destination) => destination.enabled)
                .map((destination) => ({
                  id: destination.id,
                  endpointId: destination.endpointId,
                  style: styleOfProtocol(destination.protocol),
                })),
            }
          : null,
      panel: metaFor(channelId).panel,
      rng: new Rng(this.options.seed ^ hashString(channelId) ^ this.tapCounter),
      now: () => this.now(),
      isLive: () => this.isMoving(channelId),
      onTraffic: (delta) => this.runtime.creditTraffic(config, delta, this.now()),
      onSegments,
    });
    let stopped = false;
    const stop: Unsubscribe = () => {
      if (stopped) return;
      stopped = true;
      stopTimer();
      runtime.transcriptViewers = 0;
      this.stopTaps.delete(stop);
    };
    this.stopTaps.add(stop);
    return stop;
  }

  /** Bytes move while the channel runs, or runs degraded on the message side. */
  private isMoving(channelId: string): boolean {
    const config = findChannel(this.config.live, channelId);
    const state = this.runtime.derive(this.config.live).channels.get(channelId)?.state;
    return state === "running" || (state === "degraded" && config?.kind === "message");
  }

  listAlerts(state: "open" | "cleared" | "all"): Alert[] {
    this.refresh();
    return this.alerts.list(state);
  }

  // ── Logs ────────────────────────────────────────────────────────────────────────────

  queryLogs(query: LogQuery, page: { before: number | null; limit: number }): LogPage {
    return this.logs.query(query, page);
  }

  // ── Lifecycle ───────────────────────────────────────────────────────────────────────

  dispose(): void {
    if (this.ticker !== null) clearInterval(this.ticker);
    this.ticker = null;
    this.subscribers.clear();
    for (const stop of [...this.stopTaps]) stop();
    this.logs.dispose();
    this.messages.dispose();
  }
}
