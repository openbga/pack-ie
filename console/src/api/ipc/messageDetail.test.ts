import { describe, expect, it } from "vitest";
import { IDS } from "./test/configFixtures";
import { delivery, engine, record } from "./test/messageRecords";

describe("a message through the engine", () => {
  it("reads the parsed fields on request, with their truncation and the reason when there are none", async () => {
    const { bridge, api } = engine();
    const fields = [
      { path: "MSH.9.1", value: "ORU" },
      { path: "PID.3", value: "MRN-00000" },
    ];
    bridge.request("get_message_fields", () => ({
      type: "message_fields",
      fields,
      truncated: true,
      unavailable: null,
    }));
    expect(await api.getMessageFields("msg-1")).toEqual({
      fields,
      truncated: true,
      unavailable: null,
    });
    expect(bridge.sent("get_message_fields")).toEqual([
      { type: "get_message_fields", message_id: "msg-1" },
    ]);
    bridge.request("get_message_fields", () => ({
      type: "message_fields",
      fields: [],
      truncated: false,
      unavailable: "Fields are listed for HL7 and ASTM messages.",
    }));
    expect(await api.getMessageFields("msg-1")).toEqual({
      fields: [],
      truncated: false,
      unavailable: "Fields are listed for HL7 and ASTM messages.",
    });
  });

  it("does not ask for the fields when it opens a message", async () => {
    const { bridge, api } = engine();
    bridge.request("get_message", () => ({ type: "message", message: record() }));
    await api.getMessage("msg-1");
    expect(bridge.sent("get_message_fields")).toEqual([]);
  });

  it.each([
    ["resendReason", "resend_reason", "Destination was down"],
    ["lastError", "last_error", "connection refused by 10.0.0.9:5402"],
  ] as const)("maps the %s of a delivery", async (field, wireField, value) => {
    const { bridge, api } = engine();
    bridge.request("get_message", () => ({
      type: "message",
      message: record({ deliveries: [{ ...delivery(), [wireField]: value }] }),
    }));
    expect((await api.getMessage("msg-1")).deliveries[0]?.[field]).toBe(value);
  });

  it("reads the newest revision that supersedes a message, and none when nothing does", async () => {
    const { bridge, api } = engine();
    bridge.request("get_message", () => ({
      type: "message",
      message: record({ revision_of: "msg-0", revised_by: "msg-9" }),
    }));
    expect(await api.getMessage("msg-1")).toMatchObject({
      revisionOf: "msg-0",
      revisedBy: "msg-9",
    });
    bridge.request("get_message", () => ({ type: "message", message: record() }));
    expect(await api.getMessage("msg-1")).toMatchObject({ revisionOf: null, revisedBy: null });
  });

  it("reads a message with its deliveries, without reading any body", async () => {
    const { bridge, api } = engine();
    bridge.request("get_message", () => ({
      type: "message",
      message: record({
        reason_code: "parse_error",
        duplicate_of: "msg-0",
        revision_of: "msg-00",
        abort_reason: null,
        source_file: { name: "r.csv", file_hash: "fh", first_row: 2, last_row: 4 },
        source_identity: "id",
      }),
    }));
    const detail = await api.getMessage("msg-1");
    expect(bridge.sent("get_message")).toEqual([{ type: "get_message", message_id: "msg-1" }]);
    expect(bridge.sent("get_message_body")).toEqual([]);
    expect(detail).toMatchObject({
      summary: { id: "msg-1" },
      committedAt: "2026-11-06T12:00:00Z",
      contentHash: "hash",
      sourceIdentity: "id",
      sourceEndpointId: IDS.listener,
      sourceEndpointLabel: "Chemistry listener",
      sourceFile: { name: "r.csv", fileHash: "fh", firstRow: 2, lastRow: 4 },
      responseCode: "ACK",
      respondedAt: "2026-11-06T12:00:00Z",
      abortReason: null,
      reasonCode: "parse_error",
      duplicateOf: "msg-0",
      revisionOf: "msg-00",
    });
    expect(detail.deliveries).toEqual([
      {
        id: "d-1",
        messageId: "msg-1",
        channelId: IDS.message,
        destinationId: IDS.hl7,
        destinationLabel: "LIS (HL7)",
        state: "acknowledged",
        ackLevel: "accepted",
        attempts: 2,
        totalAttempts: 2,
        createdAt: "2026-11-06T12:00:01Z",
        updatedAt: "2026-11-06T12:00:09Z",
        nextAttemptAt: null,
        reasonCode: null,
        closedReason: null,
        lastError: null,
        resendReason: null,
        mappingVersion: 7,
        transformedHash: "abc",
        attemptLog: [
          {
            attemptNo: 1,
            startedAt: "2026-11-06T12:00:02Z",
            endedAt: "2026-11-06T12:00:04Z",
            result: "ack_timeout",
            ackCode: null,
            ackControlId: null,
            errorCode: "ack_timeout",
            ackLevel: "none",
          },
          {
            attemptNo: 2,
            startedAt: "2026-11-06T12:00:06Z",
            endedAt: "2026-11-06T12:00:08Z",
            result: "acknowledged",
            ackCode: "AA",
            ackControlId: "ACK-77",
            errorCode: null,
            ackLevel: "accepted",
          },
        ],
      },
    ]);
  });

  it("falls back to the endpoint's id when the document does not name it", async () => {
    const { bridge, api } = engine();
    bridge.request("get_message", () => ({
      type: "message",
      message: record({ source_endpoint_id: "gone" }),
    }));
    expect((await api.getMessage("msg-1")).sourceEndpointLabel).toBe("gone");
  });

  it("reads a body in the format asked for, as the engine rendered it", async () => {
    const { bridge, api } = engine();
    bridge.request("get_message_body", (request) => {
      if (request.type !== "get_message_body") throw new Error("unexpected");
      return {
        type: "message_body",
        message_id: request.message_id,
        which: request.which,
        as: request.as,
        content: request.as === "hex" ? "00000000  41 42  AB\n" : "<STX>AB<ETX>\n",
        byte_length: 4,
        hash: "h",
        truncated: request.which.kind === "transformed",
      };
    });
    const original = await api.getMessageBody({
      messageId: "msg-1",
      which: { kind: "original" },
      as: "text",
    });
    expect(original).toEqual({
      messageId: "msg-1",
      which: { kind: "original" },
      as: "text",
      content: "<STX>AB<ETX>\n",
      byteLength: 4,
      hash: "h",
      truncated: false,
    });
    const transformed = await api.getMessageBody({
      messageId: "msg-1",
      which: { kind: "transformed", deliveryId: "d-1" },
      as: "hex",
    });
    expect(transformed).toMatchObject({
      which: { kind: "transformed", deliveryId: "d-1" },
      as: "hex",
      content: "00000000  41 42  AB\n",
      truncated: true,
    });
    expect(bridge.sent("get_message_body")).toEqual([
      { type: "get_message_body", message_id: "msg-1", which: { kind: "original" }, as: "text" },
      {
        type: "get_message_body",
        message_id: "msg-1",
        which: { kind: "transformed", delivery_id: "d-1" },
        as: "hex",
      },
    ]);
  });
});
