// Synthetic data and a stub engine for the Logs screens' tests.
import { type Mock, vi } from "vitest";
import type { EngineApi } from "@/api/engine";
import type {
  ChannelSnapshot,
  EndpointSnapshot,
  LogEvent,
  LogFollowNotice,
  LogPage,
  LogQuery,
  Snapshot,
} from "@/api/types";
import { spyAll } from "@/lib/test/spyAll";

const COUNTERS = { bytesIn: 0, bytesOut: 0, messagesIn: 0, messagesOut: 0, retries: 0 };

function channel(id: string, label: string): ChannelSnapshot {
  return {
    id,
    label,
    kind: "message",
    enabled: true,
    state: "running",
    stopReason: null,
    protocol: "hl7_mllp",
    endpoints: [],
    counters: COUNTERS,
    configVersion: 1,
    instanceStartedAt: null,
    nextRetryAt: null,
    allowedActions: null,
    recentErrors: [],
    buffers: [],
    bridge: null,
    source: null,
    queue: null,
    pressure: 0,
    lastActivityAt: null,
    lastError: null,
    tapActive: false,
  };
}

function endpoint(id: string, label: string): EndpointSnapshot {
  return {
    id,
    label,
    kind: "tcp_client",
    enabled: true,
    state: "connected",
    summary: "tcp://127.0.0.1:3001",
    counters: COUNTERS,
    lastActivityAt: null,
    stateSince: "2026-11-06T08:00:00Z",
    lastError: null,
    channelIds: [],
    heldBy: null,
    activePeer: null,
    consecutiveFailureCount: 0,
    nextRetryAt: null,
  };
}

/** A snapshot with two channels and two endpoints to filter by. */
const SNAPSHOT: Snapshot = {
  takenAt: "2026-11-06T12:00:00Z",
  node: {
    nodeId: "node-1",
    nodeName: "Synthetic lab node",
    version: "0.0.0",
    startedAt: "2026-11-06T08:00:00Z",
    state: "running",
    hosting: "console",
    configVersion: 1,
    pendingConfigVersion: null,
    totals: null,
    store: { state: "ok", messageCount: 0, databaseBytes: 0, diskFreeBytes: 1, diskTotalBytes: 2 },
  },
  channels: [channel("ch-chem", "Synthetic chemistry"), channel("ch-hema", "Synthetic hematology")],
  endpoints: [
    endpoint("ep-lis", "Synthetic LIS port"),
    endpoint("ep-analyzer", "Synthetic analyzer port"),
  ],
  openAlerts: [],
};

/** One event; `seq` also sets its time, one second apart, so newer events are later. */
export function event(seq: number, overrides: Partial<LogEvent> = {}): LogEvent {
  const at = new Date(Date.UTC(2026, 10, 6, 12, 0, 0) + seq * 1000).toISOString();
  return {
    seq,
    at,
    level: "info",
    target: "packie_routing::channel",
    event: null,
    channelId: "ch-chem",
    endpointId: "ep-lis",
    messageId: null,
    message: `Synthetic event ${seq}`,
    fields: {},
    ...overrides,
  };
}

/** Events from `newest` down to `oldest`, newest first, as the engine answers. */
export function newestFirst(newest: number, oldest: number): LogEvent[] {
  return Array.from({ length: newest - oldest + 1 }, (_, index) => event(newest - index));
}

export function page(events: LogEvent[], olderCursor: number | null = null): LogPage {
  return { events, olderCursor };
}

const CONNECTED = {
  state: "connected" as const,
  endpoint: "synthetic",
  detail: null,
  since: "2026-11-06T08:00:00Z",
};

/** A live subscription the test can push events into. */
interface Follow {
  query: LogQuery;
  push: (events: LogEvent[], notice?: LogFollowNotice) => void;
  stop: Mock<() => void>;
}

type Spied = "getSnapshot" | "queryLogs" | "followLogs" | "getConnection";
export type StubApi = Omit<EngineApi, Spied> & {
  [Name in Spied]: Mock<EngineApi[Name]>;
} & { follows: Follow[] };

/** A stub engine with the calls the Logs screen makes; every subscription is kept in `follows`. */
export function stubApi(overrides: Partial<Record<keyof EngineApi, unknown>> = {}): StubApi {
  const follows: Follow[] = [];
  const base = {
    getSnapshot: vi.fn(async () => SNAPSHOT),
    subscribeSnapshots: () => () => undefined,
    queryLogs: vi.fn(async () => page(newestFirst(3, 1))),
    getConnection: vi.fn(async () => CONNECTED),
    subscribeConnection: (onStatus: (status: typeof CONNECTED) => void) => {
      onStatus(CONNECTED);
      return () => undefined;
    },
    followLogs: vi.fn((query: LogQuery, onEvents: Follow["push"]) => {
      const stop = vi.fn();
      follows.push({ query, push: onEvents, stop });
      return stop;
    }),
    ...overrides,
  };
  const api = spyAll(base);
  return Object.assign(api, { follows }) as unknown as StubApi;
}
