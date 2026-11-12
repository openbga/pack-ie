import { describe, expect, it } from "vitest";
import type { TransmissionReport } from "../types";
import { mockApiFactory, rejection } from "./mockTestSupport";
import { TRANSMISSION_ROW_LIMIT } from "./transmission";

const create = mockApiFactory({ messageCount: 40_000, seed: 7 });

const DAY = 86_400_000;
/** A busy channel: a month of it holds more than the report lists. */
const BUSY = "ch-msg-au680";
const STA = "ch-msg-sta";

const ago = (days: number) => new Date(Date.now() - days * DAY).toISOString();

/** What the totals say about the rows: per destination, the rows counted the engine's way. */
function counted(report: TransmissionReport) {
  return report.destinations.map((each) => {
    const rows = report.rows.filter((row) => row.destinationId === each.destinationId);
    const states = (...wanted: string[]) => rows.filter((row) => wanted.includes(row.state)).length;
    const acked = (level: string) =>
      rows.filter((row) => row.state === "acknowledged" && row.ackLevel === level).length;
    return {
      destinationId: each.destinationId,
      deliveries: rows.length,
      accepted: acked("accepted"),
      received: acked("received"),
      handedOff: acked("handed_off"),
      unresolved: states("rejected", "failed", "timed_out", "outcome_unknown"),
      closed: states("cancelled"),
      pending: states("queued", "sending"),
    };
  });
}

describe("verifyTransmission", () => {
  it("reports the messages of the channel and period, oldest first, with their totals", async () => {
    const api = create();
    const report = await api.verifyTransmission({ channelId: STA, since: ago(2) });
    expect(report.channelId).toBe(STA);
    expect(report.messages).toBeGreaterThan(0);
    expect(report.rows.length).toBeGreaterThan(0);
    expect(report.truncated).toBe(false);
    const received = report.rows.map((row) => row.receivedAt);
    expect(received).toEqual([...received].sort());
    // Every destination of the channel's deliveries is listed once, and the totals are the rows'.
    expect(report.destinations.length).toBeGreaterThan(1);
    const rows = counted(report);
    expect(report.destinations.map(({ label: _label, ...totals }) => totals)).toEqual(rows);
    expect(report.destinations.reduce((sum, each) => sum + each.deliveries, 0)).toBe(
      report.rows.length,
    );
  });

  it("describes each delivery: both hashes, the acknowledgement and the attempts", async () => {
    const api = create();
    const report = await api.verifyTransmission({ channelId: STA, since: ago(2) });
    for (const row of report.rows) {
      expect(row.contentHash).toMatch(/^[0-9a-f]{64}$/);
      expect(row.attempts).toBeGreaterThanOrEqual(0);
      if (row.state === "acknowledged") {
        expect(row.acknowledgedAt).not.toBeNull();
        expect(row.firstAttemptAt).not.toBeNull();
        expect(row.ackLevel).not.toBe("none");
      } else {
        expect(row.acknowledgedAt).toBeNull();
      }
    }
    expect(report.rows.some((row) => row.ackCode !== null)).toBe(true);
    // Equal content hashes are equal originals: a sent hash differs from the original's.
    const sent = report.rows.filter((row) => row.sentHash !== null);
    expect(sent.length).toBeGreaterThan(0);
    for (const row of sent) expect(row.sentHash).not.toBe(row.contentHash);
  });

  it("leaves the sent hash empty for a pass-through or unsent delivery", async () => {
    const api = create();
    const channels = (await api.getSnapshot()).channels.filter((c) => c.kind === "message");
    const rows = [];
    for (const channel of channels.slice(0, 8)) {
      rows.push(...(await api.verifyTransmission({ channelId: channel.id, since: ago(1) })).rows);
    }
    expect(rows.some((row) => row.sentHash === null)).toBe(true);
    expect(rows.some((row) => row.sentHash !== null)).toBe(true);
  });

  it("respects the period: since is inclusive, until is exclusive, and a longer period holds more", async () => {
    const api = create();
    const day = await api.verifyTransmission({ channelId: STA, since: ago(1) });
    const week = await api.verifyTransmission({ channelId: STA, since: ago(7) });
    expect(week.messages).toBeGreaterThan(day.messages);
    expect(week.rows.length).toBeGreaterThan(day.rows.length);
    for (const row of day.rows)
      expect(Date.parse(row.receivedAt)).toBeGreaterThanOrEqual(Date.parse(day.since));

    const middle = await api.verifyTransmission({ channelId: STA, since: ago(3), until: ago(2) });
    expect(middle.messages).toBeGreaterThan(0);
    for (const row of middle.rows) {
      const at = Date.parse(row.receivedAt);
      expect(at).toBeGreaterThanOrEqual(Date.parse(middle.since));
      expect(at).toBeLessThan(Date.parse(middle.until));
    }
    // The engine's default end is now.
    expect(Math.abs(Date.parse(day.until) - Date.now())).toBeLessThan(5000);
  });

  it("answers an empty period with no messages, no totals and no rows", async () => {
    const api = create();
    const report = await api.verifyTransmission({
      channelId: STA,
      since: "2020-01-01T00:00:00Z",
      until: "2020-01-02T00:00:00Z",
    });
    expect(report).toMatchObject({ messages: 0, destinations: [], rows: [], truncated: false });
  });

  it("lists at most 10,000 deliveries and says so, while the totals cover the whole period", async () => {
    // A month of a busy channel needs the full-size history.
    const api = create({ messageCount: 412_000 });
    const report = await api.verifyTransmission({ channelId: BUSY, since: ago(30) });
    expect(TRANSMISSION_ROW_LIMIT).toBe(10_000);
    expect(report.truncated).toBe(true);
    expect(report.rows).toHaveLength(TRANSMISSION_ROW_LIMIT);
    const deliveries = report.destinations.reduce((sum, each) => sum + each.deliveries, 0);
    expect(deliveries).toBeGreaterThan(TRANSMISSION_ROW_LIMIT);
    // Every delivery of the period is in exactly one of its destination's categories.
    for (const each of report.destinations) {
      expect(
        each.accepted +
          each.received +
          each.handedOff +
          each.unresolved +
          each.closed +
          each.pending,
      ).toBe(each.deliveries);
    }
    // The totals do not depend on where the rows stop: two halves of the period add up to it.
    const middle = new Date(
      (Date.parse(report.since) + Date.parse(report.until)) / 2,
    ).toISOString();
    const first = await api.verifyTransmission({
      channelId: BUSY,
      since: report.since,
      until: middle,
    });
    const second = await api.verifyTransmission({
      channelId: BUSY,
      since: middle,
      until: report.until,
    });
    const sum = (r: typeof report, key: "deliveries" | "accepted" | "unresolved") =>
      r.destinations.reduce((total, each) => total + each[key], 0);
    for (const key of ["deliveries", "accepted", "unresolved"] as const) {
      expect(sum(first, key) + sum(second, key)).toBe(sum(report, key));
    }
    expect(first.messages + second.messages).toBe(report.messages);
    // The rows stop early; the first rows are the oldest deliveries of the period.
    expect(report.rows[0]?.messageId).toBe(first.rows[0]?.messageId);
  });

  it("refuses a channel it does not know, a bad time and a period that ends before it starts", async () => {
    const api = create();
    expect(
      await rejection(api.verifyTransmission({ channelId: "ch-nope", since: ago(1) })),
    ).toMatchObject({ code: "not_found" });
    expect(
      await rejection(api.verifyTransmission({ channelId: STA, since: "yesterday" })),
    ).toMatchObject({ code: "validation" });
    expect(
      await rejection(api.verifyTransmission({ channelId: STA, since: ago(1), until: ago(2) })),
    ).toMatchObject({ code: "validation" });
  });

  it("is audited as transmission_verify: requested, then the outcome with the message count", async () => {
    const api = create();
    const since = ago(1);
    const report = await api.verifyTransmission({ channelId: STA, since });
    const page = await api.getAuditEvents({
      filter: { action: "transmission_verify" },
      beforeSeq: null,
      limit: 10,
    });
    const [outcome, requested] = page.events;
    expect(requested).toMatchObject({
      action: "transmission_verify",
      objectType: "channel",
      objectId: STA,
      outcome: "requested",
      details: { since, until: null },
    });
    expect(outcome).toMatchObject({
      outcome: "succeeded",
      objectId: STA,
      details: { since, messages: report.messages },
    });
  });

  it("records a refused report as rejected with its error code", async () => {
    const api = create();
    await rejection(api.verifyTransmission({ channelId: "ch-nope", since: ago(1) }));
    const page = await api.getAuditEvents({
      filter: { action: "transmission_verify" },
      beforeSeq: null,
      limit: 10,
    });
    expect(page.events[0]).toMatchObject({
      outcome: "rejected",
      objectId: "ch-nope",
      details: { error_code: "not_found" },
    });
  });

  it("does not run while the audit log cannot be written", async () => {
    const api = create();
    api.setAuditAvailable(false);
    expect(
      await rejection(api.verifyTransmission({ channelId: STA, since: ago(1) })),
    ).toMatchObject({ code: "unavailable" });
  });
});
