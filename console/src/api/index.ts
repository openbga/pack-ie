import type { EngineApi } from "./engine";
import { IpcEngineApi } from "./ipc/IpcEngineApi";
import { tauriBridge } from "./ipc/tauriBridge";
import { MockEngineApi, type MockOptions } from "./mock";

export { EngineProvider, useEngineApi } from "./context";
export type { EngineApi } from "./engine";

export type EngineKind = "ipc" | "mock" | "mock-offline";

/**
 * Which engine this console talks to: the real one inside the Tauri window, the mock in a
 * browser. `?mock=1` forces the mock, `?mock=offline` the mock started without a reachable
 * engine (to see that screen).
 */
export function selectEngine(search: string, insideTauri: boolean): EngineKind {
  const mock = new URLSearchParams(search).get("mock");
  if (mock === "offline") return "mock-offline";
  if (mock === "1" || !insideTauri) return "mock";
  return "ipc";
}

const MAX_MESSAGES = 2_000_000;

/**
 * The mock's own switches, for demos and the end-to-end journeys (`console/e2e`):
 * `messages=<n>` seeds that many messages (500000 for the responsiveness check),
 * `scenario=empty` (a fresh install), `scenario=errors` (every request fails),
 * `scenario=slow` (every request takes 1.5 s: the loading states), `steady=1` (no endpoint
 * fails by chance: repeatable states), `e2e=1` exposes
 * `window.__packieMock` to drive outages and log events from a test.
 */
export function mockOptions(search: string): MockOptions {
  const params = new URLSearchParams(search);
  const options: MockOptions = { offline: params.get("mock") === "offline" };
  const messages = Number.parseInt(params.get("messages") ?? "", 10);
  if (Number.isInteger(messages) && messages >= 0) {
    options.messageCount = Math.min(messages, MAX_MESSAGES);
  }
  if (params.get("steady") === "1") options.steady = true;
  switch (params.get("scenario")) {
    case "empty":
      options.empty = true;
      break;
    case "errors":
      options.failReads = true;
      break;
    case "slow":
      options.latencyMs = 1500;
      break;
  }
  return options;
}

/** What a journey may do to the mock from the page; present only with `?e2e=1`. */
interface MockHandle {
  simulateOutage(): void;
  endOutage(options?: { restarted?: boolean }): void;
  publishLogEvents(count: number): void;
  /** Another client applies the active configuration again: a new version the page has not read. */
  applyElsewhere(): Promise<void>;
  /** The audit log can no longer be written: audited commands are refused, `audit_unavailable` opens. */
  setAuditAvailable(available: boolean): void;
  /** Verification finds an edited event at `seq`. */
  breakAuditChain(seq: number): void;
  /** Retention removed the oldest `count` audit events. */
  pruneAudit(count: number): void;
  /** The newest successful backup is `hours` old. */
  makeBackupStale(hours: number): void;
  /** The next backup fails with `error`. */
  failNextBackup(error: string): void;
  /** Sends to the target fail with `error` (default "connection refused") and `notifications_failing` opens; null makes every target work again. */
  failNotificationTarget(target: "email" | "webhook" | null, error?: string): void;
  /** What the console reads of the bootstrap's notifications (`unknown`: unreadable) and which targets the engine has (`none`: no target). */
  setNotificationsConfigured(mode: "both" | "none" | "unknown"): void;
  /** How many bodies and parsed-field reads the console has asked the engine for. */
  contentReads(): { body: number; fields: number };
  /** The next apply or rollback is partial: `channelId` fails with `reason` and keeps its old definition. */
  failNextApply(channelId: string, reason: string): void;
  /** A newer revision of the message arrives, as from another client; its id, or null when none can. */
  reviseMessage(id: string): string | null;
  /** The engine cannot list the serial ports while `fail` is true. */
  failSerialPorts(fail: boolean): void;
}

declare global {
  interface Window {
    __packieMock?: MockHandle;
  }
}

let created: { key: string; api: EngineApi } | null = null;

/**
 * The console's engine; one per address (and per host: browser or Tauri window). React StrictMode calls the initializer of the app's
 * state twice in development and keeps the first result: a second engine would be the one
 * `window.__packieMock` drives while the page shows the other.
 */
export function createEngineApi(): EngineApi {
  const { search } = window.location;
  const insideTauri = "__TAURI_INTERNALS__" in window;
  const key = `${insideTauri}${search}`;
  if (created?.key === key) return created.api;
  const api = newEngineApi(search, insideTauri);
  created = { key, api };
  return api;
}

function newEngineApi(search: string, insideTauri: boolean): EngineApi {
  const kind = selectEngine(search, insideTauri);
  if (kind === "ipc") return new IpcEngineApi(tauriBridge);
  const api = new MockEngineApi(mockOptions(search));
  if (new URLSearchParams(search).get("e2e") === "1") {
    window.__packieMock = {
      simulateOutage: () => api.simulateOutage(),
      endOutage: (options) => api.endOutage(options),
      publishLogEvents: (count) => api.publishLogEvents(count),
      applyElsewhere: () => api.applyElsewhere(),
      setAuditAvailable: (available) => api.setAuditAvailable(available),
      breakAuditChain: (seq) => api.breakAuditChain(seq),
      pruneAudit: (count) => api.pruneAudit(count),
      makeBackupStale: (hours) => api.makeBackupStale(hours),
      failNextBackup: (error) => api.failNextBackup(error),
      failNotificationTarget: (target, error) => api.failNotificationTarget(target, error),
      setNotificationsConfigured: (mode) => api.setNotificationsConfigured(mode),
      contentReads: () => ({ ...api.contentReads }),
      failNextApply: (channelId, reason) => api.failNextApply(channelId, reason),
      reviseMessage: (id) => api.reviseMessage(id),
      failSerialPorts: (fail) => api.failSerialPorts(fail),
    };
  }
  return api;
}
