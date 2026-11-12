import type {
  AuditOutcome,
  Delivery,
  DestinationTransmission,
  MessageDetail,
  MessageFilter,
  MessageSummary,
  TransmissionQuery,
  TransmissionReport,
  TransmissionRow,
} from "../types";
import { iso } from "./util";
import type { MockWorld } from "./world";

/** Delivery rows the engine lists in one report (docs/spec/operator-console.md section 6). */
export const TRANSMISSION_ROW_LIMIT = 10_000;

const PAGE = 1000;

function parseTime(world: MockWorld, name: string, value: string): number {
  const ms = Date.parse(value);
  if (Number.isNaN(ms)) world.fail("validation", `${name} is not a valid time.`);
  return ms;
}

/** Whether the channel is configured, or is one whose messages the store holds. */
function knowsChannel(world: MockWorld, channelId: string): boolean {
  const configured = world.config.live.channels.some(
    (channel) => channel.kind === "message" && channel.id === channelId,
  );
  return (
    configured ||
    world.messages.universe.profiles.some(
      (profile) => profile.channelId === channelId && !profile.tap,
    )
  );
}

/** The destination's totals count this delivery once, under what it proved or how it ended. */
function tally(totals: DestinationTransmission, delivery: Pick<Delivery, "state" | "ackLevel">) {
  totals.deliveries += 1;
  switch (delivery.state) {
    case "acknowledged":
      if (delivery.ackLevel === "accepted") totals.accepted += 1;
      else if (delivery.ackLevel === "received") totals.received += 1;
      else if (delivery.ackLevel === "handed_off") totals.handedOff += 1;
      else totals.unresolved += 1;
      break;
    case "queued":
    case "sending":
      totals.pending += 1;
      break;
    case "cancelled":
      totals.closed += 1;
      break;
    default:
      totals.unresolved += 1;
  }
}

function rowOf(detail: MessageDetail, delivery: Delivery): TransmissionRow {
  const { summary } = detail;
  const log = delivery.attemptLog;
  return {
    messageId: summary.id,
    receivedAt: summary.receivedAt,
    messageState: summary.state,
    controlId: summary.controlId,
    sourceEndpointId: detail.sourceEndpointId,
    sourcePeer: summary.sourcePeer,
    configVersion: summary.configVersion,
    mappingVersion: delivery.mappingVersion,
    contentHash: detail.contentHash,
    destinationId: delivery.destinationId,
    state: delivery.state,
    ackLevel: delivery.ackLevel,
    ackCode: log.findLast((attempt) => attempt.ackCode !== null)?.ackCode ?? null,
    ackControlId: log.findLast((attempt) => attempt.ackCode !== null)?.ackControlId ?? null,
    sentHash: delivery.transformedHash,
    attempts: delivery.totalAttempts,
    firstAttemptAt: log[0]?.startedAt ?? null,
    acknowledgedAt:
      delivery.state === "acknowledged"
        ? (log.findLast((attempt) => attempt.result === "acknowledged")?.endedAt ?? null)
        : null,
  };
}

function totalsOf(
  totals: Map<string, DestinationTransmission>,
  delivery: { destinationId: string; destinationLabel: string },
): DestinationTransmission {
  const found = totals.get(delivery.destinationId);
  if (found !== undefined) return found;
  const created = {
    destinationId: delivery.destinationId,
    label: delivery.destinationLabel,
    deliveries: 0,
    accepted: 0,
    received: 0,
    handedOff: 0,
    unresolved: 0,
    closed: 0,
    pending: 0,
  };
  totals.set(delivery.destinationId, created);
  return created;
}

/** The period of a query as times; a period that cannot be asked for is refused. */
function periodOf(
  world: MockWorld,
  query: TransmissionQuery,
): { sinceMs: number; untilMs: number } {
  const sinceMs = parseTime(world, "The start of the period", query.since);
  const untilMs =
    query.until === undefined
      ? world.now()
      : parseTime(world, "The end of the period", query.until);
  if (untilMs <= sinceMs) world.fail("validation", "The period must end after it starts.");
  return { sinceMs, untilMs };
}

/**
 * Walks the channel's messages of the period, oldest first, a page at a time. Like the engine,
 * `messages` and the destination totals cover the whole period, and only the rows stop at the
 * limit: a delivery beyond it is counted from its grid chip (state and acknowledgement level) and
 * not listed, which `truncated` says.
 */
function collect(
  world: MockWorld,
  filter: MessageFilter,
  totals: Map<string, DestinationTransmission>,
  rows: TransmissionRow[],
): { messages: number; truncated: boolean } {
  let truncated = false;
  for (let offset = 0; ; offset += PAGE) {
    const page = world.messages.list({
      filter,
      sort: { field: "receivedAt", descending: false },
      offset,
      limit: PAGE,
    });
    for (const summary of page.rows) {
      if (summary.deliveries.length === 0) continue;
      if (addMessage(world, summary, totals, rows)) truncated = true;
    }
    if (offset + PAGE >= page.total) return { messages: page.total, truncated };
  }
}

/** Counts a message's deliveries and lists those that fit; true when one did not. */
function addMessage(
  world: MockWorld,
  summary: MessageSummary,
  totals: Map<string, DestinationTransmission>,
  rows: TransmissionRow[],
): boolean {
  if (rows.length >= TRANSMISSION_ROW_LIMIT) {
    for (const chip of summary.deliveries) tally(totalsOf(totals, chip), chip);
    return true;
  }
  let omitted = false;
  const detail = world.messages.get(summary.id);
  for (const delivery of detail.deliveries) {
    if (rows.length === TRANSMISSION_ROW_LIMIT) omitted = true;
    else rows.push(rowOf(detail, delivery));
    tally(totalsOf(totals, delivery), delivery);
  }
  return omitted;
}

function build(world: MockWorld, query: TransmissionQuery): TransmissionReport {
  if (!knowsChannel(world, query.channelId)) {
    world.fail("not_found", `Channel ${query.channelId} was not found.`);
  }
  const { sinceMs, untilMs } = periodOf(world, query);
  const totals = new Map<string, DestinationTransmission>();
  const rows: TransmissionRow[] = [];
  const { messages, truncated } = collect(
    world,
    { channelIds: [query.channelId], since: iso(sinceMs), until: iso(untilMs) },
    totals,
    rows,
  );
  return {
    channelId: query.channelId,
    since: iso(sinceMs),
    until: iso(untilMs),
    generatedAt: iso(world.now()),
    messages,
    destinations: [...totals.values()],
    rows,
    truncated,
    rowsLimit: TRANSMISSION_ROW_LIMIT,
    periodRowsTotal: [...totals.values()].reduce((sum, each) => sum + each.deliveries, 0),
  };
}

/**
 * The transmission verification report for one channel and period, audited as
 * `transmission_verify`: `requested`, then its outcome with the message count or the error code
 * (docs/spec/audit.md section 3). Nothing runs while the audit log cannot be written.
 */
export function transmissionReport(world: MockWorld, query: TransmissionQuery): TransmissionReport {
  const event = {
    action: "transmission_verify",
    objectType: "channel",
    objectId: query.channelId,
  } as const;
  const details = { since: query.since, until: query.until ?? null };
  world.audit.record({ ...event, outcome: "requested", details });
  const outcome = (kind: AuditOutcome, more: Record<string, unknown>) => {
    if (world.audit.available)
      world.audit.record({ ...event, outcome: kind, details: { ...details, ...more } });
  };
  try {
    const report = build(world, query);
    outcome("succeeded", { messages: report.messages });
    return report;
  } catch (error) {
    const code = (error as { code?: string }).code ?? "internal";
    outcome(code === "unavailable" || code === "internal" ? "failed" : "rejected", {
      error_code: code,
    });
    throw error;
  }
}
