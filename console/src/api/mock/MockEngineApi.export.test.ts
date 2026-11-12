import { describe, expect, it, vi } from "vitest";
import type { AuditEvent, MessageFilter, MessageSummary } from "../types";
import type { MockEngineApi } from "./index";
import { mockApiFactory, rejection } from "./mockTestSupport";

const downloads: { name: string; text: string }[] = [];
vi.mock("./download", () => ({
  downloadText: (name: string, text: string) => downloads.push({ name, text }),
}));

const create = mockApiFactory({ messageCount: 3000, seed: 7 });

const HEADER =
  "message_id,received_at,channel_id,direction,state,protocol,message_type,control_id,sample_id,patient_id,byte_length,deliveries";

/** Every message the filter selects, oldest first. */
async function matching(api: MockEngineApi, filter: MessageFilter): Promise<MessageSummary[]> {
  const rows: MessageSummary[] = [];
  for (;;) {
    const page = await api.listMessages({
      filter,
      sort: { field: "receivedAt", descending: false },
      offset: rows.length,
      limit: 1000,
    });
    rows.push(...page.rows);
    if (page.rows.length === 0 || rows.length >= page.total) return rows;
  }
}

/** The audit events of an action, newest first. */
async function audited(api: MockEngineApi, action: AuditEvent["action"]): Promise<AuditEvent[]> {
  return (
    await api.getAuditEvents({
      filter: { action },
      beforeSeq: null,
      limit: 10,
    })
  ).events;
}

/** The last file the mock handed to the browser. */
const saved = () => downloads.at(-1) ?? { name: "", text: "" };

describe("exportMessages", () => {
  it("writes the messages the filter selects, oldest first, under a header, without content", async () => {
    const api = create();
    const filter: MessageFilter = { states: ["routed"], deliveryStates: ["failed"] };
    const expected = await matching(api, filter);
    expect(expected.length).toBeGreaterThan(3);
    const result = await api.exportMessages({ filter, format: "csv", includeBodies: false });
    expect(result).toEqual({ savedTo: "packie-messages.csv", rows: expected.length });
    const lines = saved().text.trimEnd().split("\r\n");
    expect(lines[0]).toBe(HEADER);
    expect(lines.slice(1).map((line) => line.split(",")[0])).toEqual(expected.map((m) => m.id));
    const times = lines.slice(1).map((line) => line.split(",")[1] ?? "");
    expect(times).toEqual([...times].sort());
  });

  it("writes each delivery as destination:state:ack_level, separated by semicolons", async () => {
    const api = create();
    const [first] = await matching(api, { deliverySummaries: ["attention"] });
    expect(first?.deliveries.length).toBeGreaterThan(0);
    await api.exportMessages({
      filter: { deliverySummaries: ["attention"] },
      format: "csv",
      includeBodies: false,
    });
    const row = saved().text.split("\r\n")[1] ?? "";
    expect(
      row.endsWith(
        first?.deliveries.map((d) => `${d.destinationId}:${d.state}:${d.ackLevel}`).join(";") ?? "",
      ),
    ).toBe(true);
  });

  it("includes the original body only when asked, and quotes it", async () => {
    const api = create();
    const filter: MessageFilter = { states: ["routed"], text: "ORU" };
    await api.exportMessages({ filter, format: "csv", includeBodies: true });
    const header = saved().text.split("\r\n")[0];
    expect(header).toBe(`${HEADER},body`);
    // The file holds the original bytes (HL7 segments end with a carriage return), quoted.
    expect(saved().text).toContain(`"MSH|`);
  });

  it("writes JSON Lines with a deliveries array, and a body field only with content", async () => {
    const api = create();
    const filter: MessageFilter = { states: ["routed"] };
    await api.exportMessages({ filter, format: "jsonl", includeBodies: false });
    expect(saved().name).toBe("packie-messages.jsonl");
    const first = JSON.parse(saved().text.split("\n")[0] ?? "{}");
    expect(Object.keys(first)).toEqual(HEADER.split(","));
    expect(Array.isArray(first.deliveries)).toBe(true);
    await api.exportMessages({ filter, format: "jsonl", includeBodies: true });
    expect(JSON.parse(saved().text.split("\n")[0] ?? "{}")).toHaveProperty("body");
  });

  it("stops at the mock's limit but records every match", async () => {
    const api = create({ messageCount: 6000 });
    const result = await api.exportMessages({ filter: {}, format: "jsonl", includeBodies: false });
    expect(result?.rows).toBe(5000);
    const [event] = await audited(api, "message_export");
    expect(event?.details).toMatchObject({ count: 6000 });
  });

  it("records the export once: filter, format, content choice and matching count", async () => {
    const api = create();
    const filter: MessageFilter = { directions: ["inbound"], text: "ORU" };
    const total = (await matching(api, filter)).length;
    const before = (await audited(api, "message_export")).length;
    await api.exportMessages({ filter, format: "csv", includeBodies: true });
    const events = await audited(api, "message_export");
    expect(events).toHaveLength(before + 1);
    expect(events[0]).toMatchObject({
      outcome: "succeeded",
      details: { filter, format: "csv", include_bodies: true, count: total },
    });
  });

  it("is refused while the audit log cannot be written, and nothing is saved", async () => {
    const api = create();
    const count = downloads.length;
    api.setAuditAvailable(false);
    expect(
      await rejection(api.exportMessages({ filter: {}, format: "csv", includeBodies: false })),
    ).toMatchObject({ code: "unavailable", message: "The audit log cannot be written." });
    expect(downloads).toHaveLength(count);
  });

  it("refuses a filter the list would refuse", async () => {
    const api = create();
    expect(
      await rejection(
        api.exportMessages({ filter: { text: "ab" }, format: "csv", includeBodies: false }),
      ),
    ).toMatchObject({ code: "validation" });
  });

  it("writes only the header for a filter nothing matches", async () => {
    const api = create();
    const result = await api.exportMessages({
      filter: { identifier: { value: "NO-SUCH-ID", prefix: false } },
      format: "csv",
      includeBodies: false,
    });
    expect(result?.rows).toBe(0);
    expect(saved().text).toBe(`${HEADER}\r\n`);
  });
});
