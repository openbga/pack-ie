// Hand-made data and a stub engine for this feature's tests.
import { vi } from "vitest";
import type { EngineApi } from "@/api/engine";
import type {
  ApplyPlan,
  ConfigDocument,
  ConfigIssue,
  EndpointConfig,
  EngineError,
  Snapshot,
  ValidationResult,
} from "@/api/types";

/** Configuration version 7: three endpoints, one bridge, one message channel. */
export function sampleConfig(): ConfigDocument {
  return {
    version: 7,
    endpoints: [
      {
        kind: "tcp_listener",
        id: "analyzer-in",
        label: "Analyzer link",
        enabled: true,
        bindAddress: "0.0.0.0",
        port: 5001,
        allowlist: ["10.0.4.20"],
        allowAnyPeer: false,
        idleTimeoutSeconds: 0,
        keepalive: true,
      },
      {
        kind: "tcp_client",
        id: "lis-out",
        label: "LIS host",
        enabled: true,
        host: "lis.example.test",
        port: 3001,
        connectTimeoutMs: 5000,
        idleTimeoutSeconds: 0,
        keepalive: true,
      },
      {
        kind: "serial",
        id: "chem-serial",
        label: "Chemistry serial",
        enabled: true,
        port: "COM3",
        baudRate: 9600,
        dataBits: 8,
        parity: "none",
        stopBits: 1,
        flowControl: "none",
      },
    ],
    channels: [
      {
        kind: "bridge",
        id: "chem-bridge",
        label: "Chemistry bridge",
        enabled: true,
        endpointAId: "chem-serial",
        endpointBId: "analyzer-in",
        pairing: "parallel",
        pairingReceiver: null,
        bufferBytes: 65536,
        retry: { initialMs: 1000, maxMs: 30000, maxAttempts: 10 },
        tapFramer: "none",
        tapRetention: { days: 30, maxMessages: null },
      },
      {
        kind: "message",
        id: "results",
        label: "Results to LIS",
        enabled: true,
        source: {
          id: "source",
          endpointId: "analyzer-in",
          protocol: "astm",
          settings: { astmRole: "computer" },
          duplicateDetection: true,
        },
        filters: [{ id: "f1", path: "R.3", operator: "exists", value: "", ignoreCase: false }],
        destinations: [
          {
            id: "d1",
            label: "LIS",
            endpointId: "lis-out",
            protocol: "hl7_mllp",
            settings: { ackMode: "original" },
            enabled: true,
            filters: [],
            mappings: [
              { id: "m1", op: "copy", from: "R.3", to: "OBX.3", default: "", required: false },
              { id: "m2", op: "constant", value: "LAB", to: "MSH.4" },
            ],
            codeTables: {},
            retry: { initialMs: 1000, maxMs: 60000, maxAttempts: 5 },
          },
        ],
        retention: { days: 30, maxMessages: null },
      },
    ],
    alertThresholds: {
      endpointDownSeconds: 120,
      queueDepthWarning: 500,
      bufferPressurePercent: 85,
      diskFreeMinMib: 2048,
    },
  };
}

/** A password-authenticated SFTP endpoint, which `sampleConfig` leaves out. */
export function sampleSftp(): EndpointConfig {
  return {
    kind: "sftp",
    id: "partner-sftp",
    label: "Partner SFTP",
    enabled: true,
    host: "sftp.example.test",
    port: 22,
    username: "labxfer",
    authMethod: "password",
    secretName: "partner-password",
    passphraseSecretName: "",
    hostKeyFingerprint: "SHA256:uNiVztksCsDhcc0u9e8BujQXVUpKZIDTMczCvj3tD2s",
    remotePath: "/outbound",
    pollIntervalSec: 60,
    filePattern: "*",
  };
}

/** A folder endpoint, which `sampleConfig` leaves out. */
function sampleFolder(): EndpointConfig {
  return {
    kind: "folder",
    id: "incoming-folder",
    label: "Incoming folder",
    enabled: true,
    path: "/data/incoming",
    mode: "watch",
    pollIntervalSec: 30,
    filePattern: "*.csv",
  };
}

/** `sampleConfig` with the sample folder and SFTP endpoints added. */
export function configWithFiles(): ConfigDocument {
  const doc = sampleConfig();
  doc.endpoints.push(sampleFolder(), sampleSftp());
  return doc;
}

export function sampleSnapshot(): Snapshot {
  return {
    takenAt: "2026-11-06T10:00:00Z",
    node: {
      nodeId: "node-1",
      nodeName: "Lab computer",
      version: "0.1.0",
      startedAt: "2026-11-06T08:00:00Z",
      state: "running",
      hosting: "service",
      configVersion: 7,
      pendingConfigVersion: null,
      totals: null,
      store: {
        state: "ok",
        messageCount: 1234,
        databaseBytes: 5_242_880,
        diskFreeBytes: 50 * 1024 ** 3,
        diskTotalBytes: 200 * 1024 ** 3,
      },
    },
    channels: [],
    endpoints: [
      {
        id: "analyzer-in",
        label: "Analyzer link",
        kind: "tcp_listener",
        enabled: true,
        state: "listening",
        summary: "tcp://0.0.0.0:5001",
        counters: { bytesIn: 0, bytesOut: 0, messagesIn: 0, messagesOut: 0, retries: 0 },
        lastActivityAt: null,
        stateSince: "2026-11-06T08:00:00Z",
        lastError: null,
        channelIds: ["chem-bridge", "results"],
        heldBy: null,
        activePeer: null,
        consecutiveFailureCount: 0,
        nextRetryAt: null,
      },
    ],
    openAlerts: [],
  };
}

export function engineFailure(
  code: EngineError["code"],
  message: string,
  fieldErrors: EngineError["fieldErrors"] = [],
): EngineError {
  return { code, message, fieldErrors, correlationId: "test-ref" };
}

/** What the engine plans for a document with errors, or one that changes nothing. */
export const NO_PLAN: ApplyPlan = { channels: [], endpoints: [], alertsChanged: false };

const VALID: ValidationResult = { ok: true, issues: [], plan: NO_PLAN };

/** Engine overrides whose validation refuses the document with these findings. */
export function validationFails(...issues: ConfigIssue[]): Partial<EngineApi> {
  return { validateConfig: vi.fn(async () => ({ ok: false, issues, plan: NO_PLAN })) };
}

/** A stub engine; `overrides` replace any method. Config methods are spies. */
export function stubApi(overrides: Partial<EngineApi> = {}) {
  const spies = {
    capabilities: { messages: "engine", secretsWritable: true, alertHistory: true } as const,
    getConfig: vi.fn(async () => sampleConfig()),
    validateConfig: vi.fn(async (_document: ConfigDocument) => VALID),
    applyConfig: vi.fn(async (document: ConfigDocument, _note: string) => ({
      version: document.version + 1,
      outcomes: document.channels.map((channel) => ({
        channelId: channel.id,
        outcome: "unchanged" as const,
      })),
      restartedChannelIds: [] as string[],
      unchangedChannelCount: document.channels.length,
      failures: [] as { channelId: string; reason: string }[],
    })),
    getSnapshot: vi.fn(async () => sampleSnapshot()),
    listSecrets: vi.fn(async () => []),
  };
  return { ...spies, ...overrides } as unknown as EngineApi & typeof spies;
}

/** A stub engine whose configuration also has a folder and an SFTP endpoint. */
export const stubApiWithFiles = () => stubApi({ getConfig: async () => configWithFiles() });
