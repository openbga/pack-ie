// Synthetic reports for the Transmission report tests: no real identifiers, hashes or times.
import type {
  DestinationTransmission,
  EngineError,
  TransmissionReport,
  TransmissionRow,
} from "@/api/types";

export const CHANNEL = { id: "ch-msg-sta", label: "STA Compact results" } as const;

const hashOf = (digit: string) => digit.repeat(64);

function destination(
  id: string,
  label: string,
  totals: Partial<DestinationTransmission> = {},
): DestinationTransmission {
  return {
    destinationId: id,
    label,
    deliveries: 0,
    accepted: 0,
    received: 0,
    handedOff: 0,
    unresolved: 0,
    closed: 0,
    pending: 0,
    ...totals,
  };
}

/** One delivery; `n` sets the message id, control id and time (one minute apart). */
export function row(n: number, overrides: Partial<TransmissionRow> = {}): TransmissionRow {
  const at = Date.UTC(2026, 10, 6, 8, n, 0);
  return {
    messageId: `msg-${n}`,
    receivedAt: new Date(at).toISOString(),
    messageState: "routed",
    controlId: `CTL-${n}`,
    sourceEndpointId: "ep-analyzer",
    sourcePeer: "10.0.4.17:41234",
    configVersion: 12,
    mappingVersion: 12,
    contentHash: hashOf("a"),
    destinationId: "dest-lis",
    state: "acknowledged",
    ackLevel: "accepted",
    ackCode: "AA",
    ackControlId: `ACK-${n}`,
    sentHash: hashOf("b"),
    attempts: 1,
    firstAttemptAt: new Date(at + 1000).toISOString(),
    acknowledgedAt: new Date(at + 2000).toISOString(),
    ...overrides,
  };
}

/**
 * Five deliveries of three messages: the LIS accepted two, took one only to a file (written,
 * not confirmed) and timed out on one; the archive received one.
 */
export function report(overrides: Partial<TransmissionReport> = {}): TransmissionReport {
  return {
    channelId: CHANNEL.id,
    since: "2026-11-06T00:00:00.000Z",
    until: "2026-11-07T00:00:00.000Z",
    generatedAt: "2026-11-07T08:30:00.000Z",
    messages: 3,
    destinations: [
      destination("dest-lis", "LIS (HL7)", {
        deliveries: 4,
        accepted: 2,
        handedOff: 1,
        unresolved: 1,
      }),
      destination("dest-archive", "Archive file", { deliveries: 1, received: 1 }),
    ],
    rows: [
      row(1),
      row(1, {
        destinationId: "dest-archive",
        ackLevel: "received",
        ackCode: "CA",
        sentHash: null,
      }),
      row(2, { ackLevel: "handed_off", ackCode: null }),
      row(3, {
        controlId: null,
        state: "timed_out",
        ackLevel: "none",
        ackCode: null,
        attempts: 3,
        acknowledgedAt: null,
      }),
      row(4),
    ],
    truncated: false,
    rowsLimit: 10_000,
    periodRowsTotal: 5,
    ...overrides,
  };
}

export function engineError(code: EngineError["code"], message: string): EngineError {
  return { code, message, fieldErrors: [], correlationId: "corr-1" };
}
