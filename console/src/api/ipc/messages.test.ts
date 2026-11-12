import { describe, expect, it } from "vitest";
import type { IpcEngineApi } from "./IpcEngineApi";
import { queryToIpc, summaryFromIpc } from "./messageMap";
import { IDS } from "./test/configFixtures";
import { base, emptyFilter, engine, RESEND_ALL, summary } from "./test/messageRecords";

describe("the grid row", () => {
  it("maps every field, keeping what the protocol did not provide as null", () => {
    expect(summaryFromIpc(summary())).toEqual({
      id: "msg-1",
      channelId: IDS.message,
      channelLabel: "Chemistry results",
      seq: 1,
      direction: "inbound",
      state: "routed",
      origin: "received",
      receivedAt: "2026-11-06T12:00:00Z",
      protocol: "astm",
      messageType: "astm",
      controlId: "C-1",
      sampleId: "S-1",
      patientId: "P-1",
      sizeBytes: 120,
      sourcePeer: "10.0.0.5:4000",
      configVersion: 7,
      resentFrom: null,
      deliverySummary: "complete",
      deliveries: [
        {
          deliveryId: "d-1",
          destinationId: IDS.hl7,
          destinationLabel: "LIS (HL7)",
          state: "acknowledged",
          ackLevel: "accepted",
        },
      ],
    });
    expect(
      summaryFromIpc(
        summary({
          state: "invalid",
          message_type: null,
          control_id: null,
          sample_id: null,
          patient_id: null,
          source_peer: null,
          delivery_summary: "none",
          deliveries: [],
          origin: "resend",
          resent_from: "msg-0",
          direction: "b_to_a",
        }),
      ),
    ).toMatchObject({
      state: "invalid",
      messageType: null,
      controlId: null,
      sampleId: null,
      patientId: null,
      sourcePeer: null,
      deliverySummary: "none",
      deliveries: [],
      origin: "resend",
      resentFrom: "msg-0",
      direction: "b_to_a",
    });
  });
});

describe("message queries", () => {
  it("sends an empty filter for an empty query", () => {
    expect(queryToIpc(base)).toEqual({
      filter: emptyFilter,
      sort: { field: "received_at", descending: true },
      offset: 0,
      limit: 100,
    });
  });

  it("translates every criterion", () => {
    const query = queryToIpc({
      filter: {
        since: "2026-11-01T00:00:00Z",
        until: "2026-11-02T00:00:00Z",
        channelIds: ["c1", "c2"],
        directions: ["inbound", "a_to_b"],
        states: ["invalid", "aborted"],
        deliveryStates: ["queued"],
        deliverySummaries: ["attention"],
        identifier: { value: " S-1 ", prefix: true },
        text: " glucose ",
        messageIdPrefix: " 0A1B2C3D ",
      },
      sort: { field: "sizeBytes", descending: false },
      offset: 200,
      limit: 5000,
    });
    expect(query).toEqual({
      filter: {
        since: "2026-11-01T00:00:00Z",
        until: "2026-11-02T00:00:00Z",
        channels: ["c1", "c2"],
        directions: ["inbound", "a_to_b"],
        states: ["invalid", "aborted"],
        delivery_states: ["queued"],
        delivery_summaries: ["attention"],
        identifier: { value: "S-1", prefix: true },
        text: "glucose",
        message_id_prefix: "0A1B2C3D",
      },
      sort: { field: "byte_length", descending: false },
      offset: 200,
      limit: 1000,
    });
  });

  it("keeps an exact identifier exact and drops blank ones", () => {
    expect(
      queryToIpc({ ...base, filter: { identifier: { value: "S-1", prefix: false } } }).filter
        .identifier,
    ).toEqual({ value: "S-1", prefix: false });
    const blank = queryToIpc({
      ...base,
      filter: { identifier: { value: "  ", prefix: true }, text: " ", messageIdPrefix: "  " },
    });
    expect(blank.filter.identifier).toBeNull();
    expect(blank.filter.text).toBeNull();
    expect(blank.filter.message_id_prefix).toBeNull();
  });

  it.each([
    ["receivedAt", "received_at"],
    ["channel", "channel"],
    ["state", "state"],
    ["messageType", "message_type"],
    ["sizeBytes", "byte_length"],
  ] as const)("sorts by %s as %s", (field, sent) => {
    expect(queryToIpc({ ...base, sort: { field, descending: true } }).sort.field).toBe(sent);
  });

  it("asks for at least one row and never a negative offset", () => {
    expect(queryToIpc({ ...base, limit: 0, offset: -5 })).toMatchObject({ limit: 1, offset: 0 });
  });
});

describe("messages through the engine", () => {
  it("lists a page", async () => {
    const { bridge, api } = engine();
    bridge.request("get_messages", () => ({
      type: "messages",
      messages: [summary()],
      total: 1234,
      offset: 100,
      limit: 50,
    }));
    const page = await api.listMessages({ ...base, offset: 100, limit: 50 });
    expect(page).toMatchObject({ total: 1234, offset: 100, limit: 50 });
    expect(page.rows[0]?.id).toBe("msg-1");
    expect(bridge.sent("get_messages")[0]).toEqual({
      type: "get_messages",
      filter: emptyFilter,
      sort: { field: "received_at", descending: true },
      offset: 100,
      limit: 50,
    });
  });

  it.each([
    ["listMessages", (api: IpcEngineApi) => api.listMessages(base), "get_messages"],
    ["getMessage", (api: IpcEngineApi) => api.getMessage("m"), "get_message"],
    [
      "getMessageBody",
      (api: IpcEngineApi) =>
        api.getMessageBody({ messageId: "m", which: { kind: "original" }, as: "text" }),
      "get_message_body",
    ],
    ["resendMessage", (api: IpcEngineApi) => api.resendMessage(RESEND_ALL), "resend"],
    ["closeDelivery", (api: IpcEngineApi) => api.closeDelivery("d", "reason"), "close_delivery"],
  ] as const)("surfaces not_implemented from the engine for %s", async (_name, call, type) => {
    const { bridge, api } = engine();
    bridge.request(type, () => {
      throw {
        kind: "ipc",
        error: {
          code: "not_implemented",
          message: "no store yet",
          field_errors: [],
          reason_code: null,
        },
      };
    });
    await expect(call(api)).rejects.toMatchObject({
      code: "not_implemented",
      message: "no store yet",
    });
  });
});
