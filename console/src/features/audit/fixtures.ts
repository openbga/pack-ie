// Synthetic events and a stub engine for the Audit screen's tests.
import { type Mock, vi } from "vitest";
import type { EngineApi } from "@/api/engine";
import type {
  AuditEvent,
  AuditExportRequest,
  AuditExportResult,
  AuditPage,
  AuditQuery,
  AuditVerification,
} from "@/api/types";
import { spyAll } from "@/lib/test/spyAll";

const ZEROS = "0".repeat(64);

/** A 64-character hash that differs per seq, so the chain reads as one. */
export const hashOf = (seq: number) => seq.toString(16).padStart(64, "a");

/** One event; `seq` also sets its time, one minute apart, so newer events are later. */
export function auditEvent(seq: number, overrides: Partial<AuditEvent> = {}): AuditEvent {
  const at = new Date(Date.UTC(2026, 10, 6, 12, 0, 0) + seq * 60_000).toISOString();
  return {
    seq,
    at,
    actor: "LAB\\jdoe",
    actorId: "S-1-5-21-1001",
    origin: "console",
    action: "channel_pause",
    objectType: "channel",
    objectId: `ch-${seq}`,
    outcome: "succeeded",
    details: {},
    hash: hashOf(seq),
    prevHash: seq === 1 ? ZEROS : hashOf(seq - 1),
    ...overrides,
  };
}

/** Events from `newest` down to `oldest`, newest first, as the engine answers. */
export function newestFirst(newest: number, oldest: number): AuditEvent[] {
  return Array.from({ length: newest - oldest + 1 }, (_, index) => auditEvent(newest - index));
}

export function auditPage(events: AuditEvent[], nextBeforeSeq: number | null = null): AuditPage {
  return { events, nextBeforeSeq };
}

export function verification(overrides: Partial<AuditVerification> = {}): AuditVerification {
  return {
    ok: true,
    files: 2,
    events: 3,
    firstSeq: 1,
    lastSeq: 3,
    headHash: hashOf(3),
    problems: [],
    ...overrides,
  };
}

type Spied = "getAuditEvents" | "exportAudit" | "verifyAudit";
export type StubApi = Omit<EngineApi, Spied> & {
  getAuditEvents: Mock<(query: AuditQuery) => Promise<AuditPage>>;
  exportAudit: Mock<(request: AuditExportRequest) => Promise<AuditExportResult | null>>;
  verifyAudit: Mock<() => Promise<AuditVerification>>;
};

/** A stub engine with the calls the Audit screen makes; three events unless told otherwise. */
export function stubApi(overrides: Partial<Record<Spied, unknown>> = {}): StubApi {
  const base = {
    getAuditEvents: vi.fn(async () => auditPage(newestFirst(3, 1))),
    exportAudit: vi.fn(async () => ({ savedTo: "/tmp/audit.csv", rows: 3 })),
    verifyAudit: vi.fn(async () => verification()),
    ...overrides,
  };
  return spyAll(base) as unknown as StubApi;
}
