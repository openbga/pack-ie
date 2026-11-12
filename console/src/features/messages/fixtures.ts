// Synthetic data and stub engines for the Messages screens' tests. No real patient data.
import { type Mock, vi } from "vitest";
import type { EngineApi } from "@/api/engine";
import type {
  ChannelSnapshot,
  Delivery,
  DeliveryAttempt,
  DeliveryChip,
  MessageBody,
  MessageDetail,
  MessageFields,
  MessagePage,
  MessageSummary,
  Snapshot,
  StoreStatus,
} from "@/api/types";
import { spyAll } from "@/lib/test/spyAll";

export function channel(id: string, label: string, kind: "message" | "bridge"): ChannelSnapshot {
  return {
    id,
    label,
    kind,
    enabled: true,
    state: "running",
    stopReason: null,
    protocol: kind === "message" ? "hl7_mllp" : null,
    endpoints: [],
    counters: { bytesIn: 0, bytesOut: 0, messagesIn: 0, messagesOut: 0, retries: 0 },
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

const OK_STORE: StoreStatus = {
  state: "ok",
  messageCount: 1200,
  databaseBytes: 0,
  diskFreeBytes: 1,
  diskTotalBytes: 2,
};

export function snapshot(store: StoreStatus | null = OK_STORE): Snapshot {
  return {
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
      store,
    },
    channels: [
      channel("ch-chem", "Synthetic chemistry", "message"),
      channel("ch-hema", "Synthetic hematology", "message"),
      channel("br-1", "Synthetic bridge", "bridge"),
    ],
    endpoints: [],
    openAlerts: [],
  };
}

const SNAPSHOT: Snapshot = snapshot();

export function chip(label: string, overrides: Partial<DeliveryChip> = {}): DeliveryChip {
  return {
    deliveryId: `d-${label}`,
    destinationId: `dest-${label}`,
    destinationLabel: label,
    state: "acknowledged",
    ackLevel: "accepted",
    ...overrides,
  };
}

export function summary(index: number, overrides: Partial<MessageSummary> = {}): MessageSummary {
  const number = String(index).padStart(6, "0");
  return {
    id: `msg-${number}`,
    channelId: "ch-chem",
    channelLabel: "Synthetic chemistry",
    seq: index,
    direction: "inbound",
    state: "routed",
    origin: "received",
    receivedAt: "2026-11-06T11:30:00Z",
    protocol: "hl7_mllp",
    messageType: "ORU^R01",
    controlId: `CTRL-${number}`,
    sampleId: `SMP-${number}`,
    patientId: `PAT-TEST-${number}`,
    sizeBytes: 1536,
    sourcePeer: "10.0.0.5:4000",
    configVersion: 7,
    resentFrom: null,
    deliverySummary: "complete",
    deliveries: [chip("LIS"), chip("Archive")],
    ...overrides,
  };
}

/** `count` rows starting at message number `from`. */
export function rows(from: number, count: number): MessageSummary[] {
  return Array.from({ length: count }, (_, index) => summary(from + index));
}

export function page(messages: MessageSummary[], total: number = messages.length): MessagePage {
  return { rows: messages, total, offset: 0, limit: 100 };
}

export const HL7_TEXT =
  "<STX>MSH|^~\\&|SYN|LAB|||202611061130||ORU^R01|CTRL-000001|P|2.5<CR>\nPID|1||SMP-1<CR>\n";

export function messageBody(overrides: Partial<MessageBody> = {}): MessageBody {
  return {
    messageId: "msg-000001",
    which: { kind: "original" },
    as: "text",
    content: HL7_TEXT,
    byteLength: 78,
    hash: "0".repeat(64),
    truncated: false,
    ...overrides,
  };
}

export function attempt(no: number, overrides: Partial<DeliveryAttempt> = {}): DeliveryAttempt {
  return {
    attemptNo: no,
    startedAt: "2026-11-06T11:30:05Z",
    endedAt: "2026-11-06T11:30:06Z",
    result: "acknowledged",
    ackCode: "AA",
    ackControlId: "ACK-000001",
    errorCode: null,
    ackLevel: "accepted",
    ...overrides,
  };
}

export function delivery(label: string, overrides: Partial<Delivery> = {}): Delivery {
  return {
    id: `d-${label}`,
    messageId: "msg-000001",
    channelId: "ch-chem",
    destinationId: `dest-${label}`,
    destinationLabel: label,
    state: "acknowledged",
    ackLevel: "accepted",
    attempts: 1,
    totalAttempts: 1,
    createdAt: "2026-11-06T11:30:01Z",
    updatedAt: "2026-11-06T11:30:06Z",
    nextAttemptAt: null,
    reasonCode: null,
    closedReason: null,
    lastError: null,
    resendReason: null,
    mappingVersion: 7,
    transformedHash: "1".repeat(64),
    attemptLog: [attempt(1)],
    ...overrides,
  };
}

export function detail(overrides: Partial<MessageDetail> = {}): MessageDetail {
  const deliveries = overrides.deliveries ?? [delivery("LIS"), delivery("Archive")];
  return {
    summary: summary(1, {
      deliveries: deliveries.map((d) =>
        chip(d.destinationLabel, { state: d.state, ackLevel: d.ackLevel }),
      ),
    }),
    committedAt: "2026-11-06T11:30:00Z",
    contentHash: "2".repeat(64),
    sourceIdentity: "SYN|LAB|CTRL-000001",
    sourceEndpointId: "ep-1",
    sourceEndpointLabel: "Synthetic analyzer",
    sourceFile: null,
    responseCode: "AA",
    respondedAt: "2026-11-06T11:30:00Z",
    abortReason: null,
    reasonCode: null,
    duplicateOf: null,
    revisionOf: null,
    revisedBy: null,
    deliveries,
    ...overrides,
  };
}

/** The parsed fields the engine lists for the synthetic HL7 message. */
export function fieldsOf(overrides: Partial<MessageFields> = {}): MessageFields {
  return {
    fields: [
      { path: "MSH.9.1", value: "ORU" },
      { path: "MSH.9.2", value: "R01" },
      { path: "MSH.10", value: "CTRL-000001" },
      { path: "PID.3", value: "MRN-00000" },
      { path: "OBX.5", value: "5.5" },
      { path: "OBX[1].5", value: "140" },
    ],
    truncated: false,
    unavailable: null,
    ...overrides,
  };
}

type Spied =
  | "getSnapshot"
  | "listMessages"
  | "getMessage"
  | "getMessageFields"
  | "getMessageBody"
  | "resendMessage"
  | "closeDelivery"
  | "exportMessages";

/** The engine as the Messages screens use it, with the calls they make as spies. */
export type StubApi = Omit<EngineApi, Spied> & { [Name in Spied]: Mock<EngineApi[Name]> };

/** A stub engine with only the calls the Messages screens make; override any of them. */
export function stubApi(overrides: Partial<Record<keyof EngineApi, unknown>> = {}): StubApi {
  const base = {
    getSnapshot: vi.fn(async () => SNAPSHOT),
    subscribeSnapshots: () => () => undefined,
    capabilities: { messages: "engine", secretsWritable: true, alertHistory: true },
    listMessages: vi.fn(async () => page(rows(1, 3))),
    getMessage: vi.fn(async () => detail()),
    getMessageFields: vi.fn(async () => fieldsOf()),
    getMessageBody: vi.fn(async (request: { as: string }) =>
      messageBody({ as: request.as === "hex" ? "hex" : "text" }),
    ),
    resendMessage: vi.fn(async () => ({
      newMessageId: null,
      destinations: [
        {
          destinationId: "dest-LIS",
          destinationLabel: "LIS",
          deliveryId: "d-LIS",
          rejected: null,
        },
      ],
    })),
    closeDelivery: vi.fn(async () => delivery("LIS", { state: "cancelled", closedReason: "x" })),
    exportMessages: vi.fn(async () => ({ savedTo: "/home/lab/packie-messages.csv", rows: 3 })),
    ...overrides,
  };
  // Every call is a spy, so a test that overrides one can still read its calls.
  return spyAll(base) as unknown as StubApi;
}
