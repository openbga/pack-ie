import { describe, expect, it } from "vitest";
import type { MessageSortField, MessageSummary } from "../types";
import type { MockEngineApi } from "./index";
import { mockApiFactory, query } from "./mockTestSupport";

const create = mockApiFactory({ messageCount: 5000, seed: 7 });

/** Every message of the 5,000 in newest-first order. */
async function everything(api: MockEngineApi): Promise<MessageSummary[]> {
  const rows: MessageSummary[] = [];
  for (let offset = 0; offset < 5000; offset += 1000) {
    rows.push(...(await api.listMessages(query({}, { offset, limit: 1000 }))).rows);
  }
  return rows;
}

describe("listMessages", () => {
  it("pages newest first, and answers with the page it was asked for and the total", async () => {
    const api = create();
    const first = await api.listMessages(query());
    expect(first).toMatchObject({ total: 5000, offset: 0, limit: 50 });
    expect(first.rows).toHaveLength(50);
    expect(first.rows.map((r) => r.id).slice(0, 3)).toEqual(["msg-0", "msg-1", "msg-2"]);
    for (let i = 1; i < first.rows.length; i++) {
      expect((first.rows[i]?.receivedAt ?? "") < (first.rows[i - 1]?.receivedAt ?? "")).toBe(true);
    }
    const second = await api.listMessages(query({}, { offset: 50, limit: 25 }));
    expect(second).toMatchObject({ offset: 50, limit: 25 });
    expect(second.rows[0]?.id).toBe("msg-50");
    expect(second.rows).toHaveLength(25);
    const tail = await api.listMessages(query({}, { offset: 4990, limit: 50 }));
    expect(tail.rows).toHaveLength(10);
    expect((await api.listMessages(query({}, { offset: 6000 }))).rows).toEqual([]);
  });

  it("refuses a page the engine would refuse, and a text search that is too short", async () => {
    const api = create();
    await expect(api.listMessages(query({}, { limit: 0 }))).rejects.toMatchObject({
      code: "validation",
    });
    await expect(api.listMessages(query({}, { limit: 1001 }))).rejects.toMatchObject({
      code: "validation",
    });
    await expect(api.listMessages(query({ text: "ab" }))).rejects.toMatchObject({
      code: "validation",
      message: expect.stringContaining("3 characters"),
    });
    expect((await api.listMessages(query({ text: "  " }))).total).toBe(5000);
  });

  it("has a realistic mix of message states and delivery summaries", async () => {
    const rows = await everything(create());
    const states = new Set(rows.map((r) => r.state));
    for (const state of ["routed", "filtered", "duplicate", "invalid", "aborted", "observed"]) {
      expect(states, state).toContain(state);
    }
    const summaries = new Set(rows.map((r) => r.deliverySummary));
    for (const summary of ["complete", "attention", "pending", "none"]) {
      expect(summaries, summary).toContain(summary);
    }
    // The newest message is still arriving, and the next few are being delivered.
    expect(["receiving", "received"]).toContain(rows[0]?.state);
    expect(rows.slice(1, 4).map((r) => r.deliverySummary)).toEqual([
      "pending",
      "pending",
      "pending",
    ]);
  });

  it("gives messages that did not parse, and observed ones, the fields they have in the engine", async () => {
    const rows = await everything(create());
    const of = (state: MessageSummary["state"]) => rows.filter((r) => r.state === state);
    for (const row of [...of("invalid"), ...of("aborted")]) {
      expect(row).toMatchObject({
        messageType: null,
        controlId: null,
        sampleId: null,
        patientId: null,
        deliverySummary: "none",
        deliveries: [],
      });
    }
    for (const row of of("aborted")) expect(["astm", "au"]).toContain(row.protocol);
    for (const row of of("observed")) {
      expect(row).toMatchObject({ origin: "tap", deliveries: [], deliverySummary: "none" });
      expect(["a_to_b", "b_to_a"]).toContain(row.direction);
      expect(row.channelId).toMatch(/^ch-br-/);
    }
    for (const row of rows.filter((r) => r.state !== "observed")) {
      expect(row.direction).toBe("inbound");
    }
    for (const row of of("routed")) expect(row.deliveries.length).toBeGreaterThan(0);
  });

  it("has messages created by a resend, linked to the message they came from", async () => {
    const rows = await everything(create());
    const resent = rows.filter((r) => r.origin === "resend");
    expect(resent.length).toBeGreaterThan(10);
    for (const row of resent) {
      expect(row.resentFrom).toMatch(/^msg-\d+$/);
      expect(row.sourcePeer).toBeNull();
      expect(rows.some((r) => r.id === row.resentFrom)).toBe(true);
    }
    expect(rows.filter((r) => r.origin !== "resend").every((r) => r.resentFrom === null)).toBe(
      true,
    );
  });

  it("mixes protocols, types, channels and source peers", async () => {
    const rows = await everything(create());
    expect(rows.every((r) => r.configVersion >= 1)).toBe(true);
    const types = new Set(rows.map((r) => r.messageType));
    for (const type of ["ORU^R01", "ORM^O01", "astm", "au", "delimited"]) {
      expect(types).toContain(type);
    }
    expect(new Set(rows.map((r) => r.channelId)).size).toBeGreaterThan(15);
    expect(rows.filter((r) => r.sourcePeer !== null).length).toBeGreaterThan(rows.length / 2);
  });

  it("derives the delivery summary from the chips of the row", async () => {
    const rows = await everything(create());
    const attention = new Set(["failed", "rejected", "timed_out", "outcome_unknown"]);
    for (const row of rows) {
      const states = row.deliveries.map((d) => d.state);
      const expected =
        states.length === 0
          ? "none"
          : states.some((s) => attention.has(s))
            ? "attention"
            : states.some((s) => s === "queued" || s === "sending")
              ? "pending"
              : "complete";
      expect(row.deliverySummary, row.id).toBe(expected);
    }
  });

  it("filters on the server and reports the total of the whole query", async () => {
    const api = create();
    const attention = await api.listMessages(
      query({ deliverySummaries: ["attention"] }, { limit: 1000 }),
    );
    expect(attention.total).toBe(attention.rows.length);
    expect(attention.rows.every((r) => r.deliverySummary === "attention")).toBe(true);

    const channelIds = ["ch-msg-xn1000", "ch-msg-au680"];
    const channels = await api.listMessages(query({ channelIds }, { limit: 1000 }));
    expect(channels.total).toBeGreaterThan(100);
    expect(new Set(channels.rows.map((r) => r.channelId))).toEqual(new Set(channelIds));
    const one = await api.listMessages(query({ channelIds: [channelIds[0] ?? ""] }));
    expect(one.total).toBeLessThan(channels.total);

    const states = await api.listMessages(
      query({ states: ["invalid", "aborted"] }, { limit: 1000 }),
    );
    expect(states.rows.every((r) => r.state === "invalid" || r.state === "aborted")).toBe(true);
    expect(states.total).toBeGreaterThan(10);

    const directions = await api.listMessages(query({ directions: ["a_to_b"] }, { limit: 1000 }));
    expect(directions.total).toBeGreaterThan(5);
    expect(directions.rows.every((r) => r.direction === "a_to_b")).toBe(true);

    const queued = await api.listMessages(query({ deliveryStates: ["queued"] }, { limit: 1000 }));
    expect(queued.total).toBeGreaterThan(0);
    expect(queued.rows.every((r) => r.deliveries.some((d) => d.state === "queued"))).toBe(true);
    const either = await api.listMessages(
      query({ deliveryStates: ["rejected", "outcome_unknown"] }, { limit: 1000 }),
    );
    expect(
      either.rows.every((r) =>
        r.deliveries.some((d) => d.state === "rejected" || d.state === "outcome_unknown"),
      ),
    ).toBe(true);

    const both = await api.listMessages(
      query({ channelIds, deliverySummaries: ["complete"], states: ["routed"] }, { limit: 1000 }),
    );
    expect(both.total).toBeGreaterThan(0);
    expect(
      both.rows.every(
        (r) =>
          channelIds.includes(r.channelId) &&
          r.deliverySummary === "complete" &&
          r.state === "routed",
      ),
    ).toBe(true);
    expect((await api.listMessages(query({ channelIds: ["ch-unknown"] }))).total).toBe(0);
  });

  it("reads the time range as since inclusive and until exclusive", async () => {
    const api = create();
    const all = await api.listMessages(query({}, { limit: 1000, offset: 2000 }));
    const since = all.rows[200]?.receivedAt ?? "";
    const newest = all.rows[100]?.receivedAt ?? "";
    const range = await api.listMessages(query({ since, until: newest }, { limit: 1000 }));
    expect(range.total).toBe(100);
    expect((range.rows[0]?.receivedAt ?? "") < newest).toBe(true);
    expect(range.rows[99]?.receivedAt).toBe(since);
    expect((await api.listMessages(query({ since: newest, until: since }))).total).toBe(0);
  });

  it("matches identifiers by exact value or prefix, and text in the original", async () => {
    const api = create();
    const rows = await everything(api);
    const sample = rows[1234];
    if (sample?.sampleId == null) throw new Error("no sample");
    const exact = await api.listMessages(
      query({ identifier: { value: sample.sampleId, prefix: false } }),
    );
    expect(exact.rows.map((r) => r.id)).toContain(sample.id);
    expect(exact.total).toBeLessThanOrEqual(12);
    expect(exact.rows.every((r) => r.sampleId === sample.sampleId)).toBe(true);

    const start = sample.sampleId.slice(0, 9);
    const prefix = await api.listMessages(query({ identifier: { value: start, prefix: true } }));
    expect(prefix.total).toBeGreaterThan(exact.total);
    expect(prefix.rows.every((r) => r.sampleId?.startsWith(start))).toBe(true);
    const short = await api.listMessages(query({ identifier: { value: start, prefix: false } }));
    expect(short.total).toBe(0);

    const hl7 = rows.find((r) => r.protocol === "hl7_mllp" && r.controlId !== null);
    if (hl7?.controlId == null || hl7.patientId == null) throw new Error("no hl7 message");
    const byControl = await api.listMessages(
      query({ identifier: { value: hl7.controlId.toLowerCase(), prefix: false } }),
    );
    expect(byControl.rows.map((r) => r.id)).toContain(hl7.id);
    const byPatient = await api.listMessages(
      query({ identifier: { value: hl7.patientId, prefix: true } }),
    );
    expect(byPatient.rows.map((r) => r.id)).toContain(hl7.id);
    const byText = await api.listMessages(query({ text: hl7.controlId }));
    expect(byText.rows.map((r) => r.id)).toContain(hl7.id);
    expect(byText.total).toBeLessThan(10);
    const byType = await api.listMessages(query({ text: "ORU^R01" }));
    expect(byType.total).toBeGreaterThan(100);
    const body = await api.getMessageBody({
      messageId: byText.rows[0]?.id ?? "",
      which: { kind: "original" },
      as: "text",
    });
    expect(body.content).toContain(hl7.controlId);
    expect((await api.listMessages(query({ text: "no-such-text-xyz" }))).total).toBe(0);
    expect(
      (await api.listMessages(query({ identifier: { value: "ZZZ-", prefix: true } }))).total,
    ).toBe(0);
  });

  it("finds a message by the start of its stored id, and refuses what the engine refuses", async () => {
    const api = create();
    // The mock numbers messages; the engine's UUID form carries the number in its last group.
    const one = await api.listMessages(
      query({ messageIdPrefix: "00000000-0000-4000-8000-0000000004D2" }),
    );
    expect(one.rows.map((r) => r.id)).toEqual(["msg-1234"]);
    // Dashes are optional, case does not matter, and a shorter prefix matches several.
    const head = "000000000000" + "4000" + "8000";
    const sixteen = await api.listMessages(
      query({ messageIdPrefix: ` ${head}${"0".repeat(11)} ` }, { limit: 1000 }),
    );
    expect(sixteen.total).toBe(16);
    expect(sixteen.rows.every((r) => /^msg-\d{1,2}$/.test(r.id))).toBe(true);
    expect(
      (await api.listMessages(query({ messageIdPrefix: `${head}${"f".repeat(9)}` }))).total,
    ).toBe(0);
    const created = await api.listMessages(query({ messageIdPrefix: "00000000-0000-4000-8001" }));
    expect(created.total).toBe(0);
    for (const bad of ["abc", "zzzzzzzz", "0".repeat(33)]) {
      await expect(api.listMessages(query({ messageIdPrefix: bad }))).rejects.toMatchObject({
        code: "validation",
      });
    }
    // Blank means no criterion.
    expect((await api.listMessages(query({ messageIdPrefix: "  " }))).total).toBe(5000);
  });

  it("searches the bodies of invalid and aborted messages too", async () => {
    const api = create();
    const invalid = await api.listMessages(query({ states: ["invalid"] }, { limit: 1000 }));
    const hl7 = invalid.rows.find((r) => r.protocol === "hl7_mllp");
    const body = await api.getMessageBody({
      messageId: hl7?.id ?? "",
      which: { kind: "original" },
      as: "text",
    });
    expect(body.content.length).toBeGreaterThan(0);
    const found = await api.listMessages(
      query({ text: body.content.slice(0, 5), states: ["invalid"] }),
    );
    expect(found.rows.map((r) => r.id)).toContain(hl7?.id);
  });

  const fields: MessageSortField[] = ["receivedAt", "channel", "state", "messageType", "sizeBytes"];

  it.each(fields)("sorts by %s in both directions", async (field) => {
    const api = create();
    const key = (row: MessageSummary): string | number => {
      switch (field) {
        case "receivedAt":
          return row.receivedAt;
        case "channel":
          return row.channelLabel;
        case "state":
          return row.state;
        case "messageType":
          return row.messageType ?? "";
        case "sizeBytes":
          return row.sizeBytes;
      }
    };
    const up = await api.listMessages(
      query({}, { sort: { field, descending: false }, limit: 1000 }),
    );
    const down = await api.listMessages(
      query({}, { sort: { field, descending: true }, limit: 1000 }),
    );
    expect(up.total).toBe(5000);
    expect(down.total).toBe(5000);
    const compare = (a: MessageSummary, b: MessageSummary) => {
      const x = key(a);
      const y = key(b);
      if (typeof x === "number" && typeof y === "number") return x - y;
      return String(x) < String(y) ? -1 : String(x) > String(y) ? 1 : 0;
    };
    for (let i = 1; i < up.rows.length; i++) {
      expect(
        compare(up.rows[i - 1] as MessageSummary, up.rows[i] as MessageSummary),
      ).toBeLessThanOrEqual(0);
      expect(
        compare(down.rows[i - 1] as MessageSummary, down.rows[i] as MessageSummary),
      ).toBeGreaterThanOrEqual(0);
    }
  });

  it("sorts by size with the largest first and pages through the whole sorted list", async () => {
    const api = create();
    const sort = { field: "sizeBytes" as const, descending: true };
    const sizes: number[] = [];
    for (let offset = 0; offset < 5000; offset += 1000) {
      const page = await api.listMessages(query({}, { sort, offset, limit: 1000 }));
      sizes.push(...page.rows.map((r) => r.sizeBytes));
    }
    expect(sizes).toHaveLength(5000);
    for (let i = 1; i < sizes.length; i++) expect(sizes[i]).toBeLessThanOrEqual(sizes[i - 1] ?? 0);
  });
});
