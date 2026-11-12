import { describe, expect, it } from "vitest";
import type { Delivery } from "../types";
import type { MockEngineApi } from "./index";
import { DISPLAY_LIMIT_BYTES, hexDump, renderBody, visibleText } from "./messageRender";
import { mockApiFactory, originalText, rowsOf, withDelivery } from "./mockTestSupport";

const create = mockApiFactory({ messageCount: 5000, seed: 7 });

const NEEDS_ATTENTION = new Set(["failed", "rejected", "timed_out", "outcome_unknown"]);

function expectAttemptsOrdered(d: Delivery) {
  d.attemptLog.forEach((a, i) => {
    expect(a.attemptNo).toBe(i + 1);
    if (a.endedAt !== null) expect(a.endedAt >= a.startedAt).toBe(true);
  });
  expect(d.attempts).toBeLessThanOrEqual(d.totalAttempts);
  expect(d.totalAttempts).toBe(d.attemptLog.length);
}

/** What the state of a delivery requires of its attempts, level and reason. */
function expectStateFitsAttempts(d: Delivery) {
  if (d.state === "acknowledged") {
    expect(d.attemptLog.at(-1)?.result).toBe("acknowledged");
    expect(d.ackLevel).not.toBe("none");
  }
  if (d.state === "cancelled") expect(d.closedReason !== null || d.reasonCode !== null).toBe(true);
  if (d.reasonCode === "transform_error") {
    expect(d).toMatchObject({ state: "failed", attempts: 0, transformedHash: null });
  } else if (NEEDS_ATTENTION.has(d.state)) {
    expect(d.attemptLog.length).toBeGreaterThan(0);
  }
}

interface DestinationProtocol {
  protocol: string;
  ackMode?: string | undefined;
}

/** The protocol and acknowledgement mode of every destination of a channel, once. */
async function learnDestinations(
  api: MockEngineApi,
  channelId: string,
  known: Map<string, DestinationProtocol>,
) {
  const config = (await api.getChannel(channelId)).config;
  if (config.kind !== "message" || known.has(`${channelId}/${config.destinations[0]?.id}`)) return;
  for (const x of config.destinations) {
    known.set(`${channelId}/${x.id}`, { protocol: x.protocol, ackMode: x.settings.ackMode });
  }
}

/** The highest acknowledgement a destination can prove (message-channels.md section 8). */
function highestLevel(destination: DestinationProtocol | undefined): string {
  switch (destination?.protocol) {
    case "delimited_file":
      return "handed_off";
    case "astm":
    case "au":
      return "received";
    default:
      return destination?.ackMode === "none" ? "handed_off" : "accepted";
  }
}

describe("the bodies as the engine renders them", () => {
  it("shows control characters by name and breaks the line after CR and LF", () => {
    expect(visibleText("\x02MSH|^~\\&\rPID\x03")).toBe("<STX>MSH|^~\\&<CR>\nPID<ETX>");
    expect(visibleText("a\r\nb\x7f")).toBe("a<CR>\n<LF>\nb<DEL>");
  });

  it("dumps hex with offsets, 16 bytes a line, and the printable text", () => {
    expect(hexDump("ABC\r")).toBe(`00000000  41 42 43 0d${" ".repeat(38)}ABC.\n`);
    expect(hexDump("0123456789abcdefg").split("\n")).toEqual([
      "00000000  30 31 32 33 34 35 36 37 38 39 61 62 63 64 65 66  0123456789abcdef",
      `00000010  67${" ".repeat(47)}g`,
      "",
    ]);
    expect(hexDump("")).toBe("");
  });

  it("cuts a body at the display limit and says so", () => {
    const body = "A".repeat(DISPLAY_LIMIT_BYTES + 10);
    const rendered = renderBody(body, "text");
    expect(rendered.truncated).toBe(true);
    expect(rendered.content).toHaveLength(DISPLAY_LIMIT_BYTES);
    expect(renderBody("short", "text")).toEqual({ content: "short", truncated: false });
  });
});

describe("getMessage and getMessageBody", () => {
  it("round-trips with the list: the same row, chips and size", async () => {
    const api = create();
    for (const row of (await rowsOf(api, {})).slice(0, 400)) {
      const detail = await api.getMessage(row.id);
      expect(detail.summary).toEqual(row);
      expect(detail.deliveries.map((d) => d.id)).toEqual(row.deliveries.map((c) => c.deliveryId));
      const body = await api.getMessageBody({
        messageId: row.id,
        which: { kind: "original" },
        as: "text",
      });
      expect(body.byteLength).toBe(row.sizeBytes);
      expect(body.hash).toBe(detail.contentHash);
    }
  });

  it("renders protocol text for each protocol, as text and as hex", async () => {
    const api = create();
    const rows = await rowsOf(api, { states: ["routed"] });
    const pick = (type: string) =>
      rows.find((r) => r.messageType === type && r.origin === "received");

    const oru = await originalText(api, pick("ORU^R01")?.id ?? "");
    expect(oru.startsWith("MSH|^~\\&|")).toBe(true);
    expect(oru.split("<CR>\n").map((s) => s.slice(0, 3))).toEqual(
      expect.arrayContaining(["MSH", "PID", "OBR", "OBX"]),
    );
    expect(oru).toMatch(/PID\|1\|\|PT-\d{6}\^\^\^MRN\|\|[A-Z]+\^[A-Z]+/);
    expect(oru).not.toContain("\r");

    const astm = await originalText(api, pick("astm")?.id ?? "");
    expect(astm.startsWith("<STX>")).toBe(true);
    for (const record of ["H|\\^&", "P|1", "O|1", "R|1", "L|1|N", "<ETX>", "<LF>"]) {
      expect(astm).toContain(record);
    }

    expect((await originalText(api, pick("au")?.id ?? "")).startsWith("<STX>")).toBe(true);
    expect((await originalText(api, pick("delimited")?.id ?? "")).split("\n")[0]).toContain(
      "sample_id,patient_id",
    );

    const hex = await originalText(api, pick("ORU^R01")?.id ?? "", "hex");
    expect(hex).toMatch(/^00000000 {2}4d 53 48 7c /);
    expect(hex.endsWith("\n")).toBe(true);
    const bytes = pick("ORU^R01")?.sizeBytes ?? 0;
    expect(hex.split("\n")).toHaveLength(Math.ceil(bytes / 16) + 1);
  });

  it("gives an invalid message its reason, its negative response and bytes that do not parse", async () => {
    const api = create();
    const rows = await rowsOf(api, { states: ["invalid"] });
    const hl7 = rows.find((r) => r.protocol === "hl7_mllp");
    const detail = await api.getMessage(hl7?.id ?? "");
    expect(detail).toMatchObject({
      responseCode: "AR",
      abortReason: null,
      deliveries: [],
      duplicateOf: null,
      sourceIdentity: null,
    });
    expect(detail.reasonCode).toMatch(/^(parse_error|missing_required_field)$/);
    expect(detail.respondedAt).not.toBeNull();
    const body = await originalText(api, detail.summary.id);
    expect(body.length).toBeGreaterThan(0);
    expect(body.startsWith("ZZZ|") || body.startsWith("MSH|^~\\&|")).toBe(true);
  });

  it("gives an aborted transfer its abort reason, no response and the frames it received", async () => {
    const api = create();
    const aborted = (await rowsOf(api, { states: ["aborted"] }))[0];
    const detail = await api.getMessage(aborted?.id ?? "");
    expect(detail.abortReason).toMatch(
      /^(receive_timeout|retry_limit_exceeded|connection_lost|empty_transfer|incomplete_record)$/,
    );
    expect(detail).toMatchObject({ responseCode: null, respondedAt: null, deliveries: [] });
    const body = await originalText(api, detail.summary.id);
    expect(body).toContain("<STX>");
    expect(body).not.toContain("L|1|N");
  });

  it("links a duplicate to an original that has the same content", async () => {
    const api = create();
    const row = (await rowsOf(api, { states: ["duplicate"] }))[0];
    const duplicate = await api.getMessage(row?.id ?? "");
    expect(duplicate).toMatchObject({ deliveries: [], responseCode: expect.any(String) });
    expect(duplicate.duplicateOf).toMatch(/^msg-\d+$/);
    const original = await api.getMessage(duplicate.duplicateOf ?? "");
    expect(original.summary.channelId).toBe(duplicate.summary.channelId);
    expect(original.summary.controlId).toBe(duplicate.summary.controlId);
    expect(original.summary.sampleId).toBe(duplicate.summary.sampleId);
    expect(original.contentHash).toBe(duplicate.contentHash);
    expect(original.sourceIdentity).toBe(duplicate.sourceIdentity);
    expect(await originalText(api, duplicate.summary.id, "hex")).toBe(
      await originalText(api, original.summary.id, "hex"),
    );
    expect(["received", "routed", "filtered"]).toContain(original.summary.state);
  });

  it("links a resent message to its original and gives it no response", async () => {
    const api = create();
    const row = (await rowsOf(api, {})).find((r) => r.origin === "resend");
    const resent = await api.getMessage(row?.id ?? "");
    expect(resent).toMatchObject({ responseCode: null, respondedAt: null, sourceIdentity: null });
    const original = await api.getMessage(resent.summary.resentFrom ?? "");
    expect(original.contentHash).toBe(resent.contentHash);
    expect(await originalText(api, resent.summary.id)).toBe(
      await originalText(api, original.summary.id),
    );
    expect(original.summary.origin).toBe("received");
  });

  it("describes the source: a peer, or a file with its rows", async () => {
    const api = create();
    const rows = await rowsOf(api, { states: ["routed"] });
    const csv = rows.find((r) => r.protocol === "delimited_file" && r.origin === "received");
    const file = (await api.getMessage(csv?.id ?? "")).sourceFile;
    expect(file).toMatchObject({
      name: expect.stringMatching(/\.csv$/),
      firstRow: expect.any(Number),
    });
    expect(file?.fileHash).toHaveLength(64);
    const hl7 = rows.find((r) => r.protocol === "hl7_mllp" && r.origin === "received");
    const detail = await api.getMessage(hl7?.id ?? "");
    expect(detail.sourceFile).toBeNull();
    expect(detail.sourceEndpointLabel.length).toBeGreaterThan(0);
    expect(detail.sourceIdentity).toContain(detail.summary.controlId);
    expect(detail.committedAt).not.toBeNull();
    expect(detail.contentHash).toMatch(/^[0-9a-f]{64}$/);
  });

  it("derives deliveries in every state with the attempts that explain them", async () => {
    const api = create();
    const seen = new Map<Delivery["state"], Delivery>();
    for (const row of await rowsOf(api, {})) {
      for (const chip of row.deliveries) {
        if (seen.has(chip.state)) continue;
        const detail = await api.getMessage(row.id);
        const found = detail.deliveries.find((d) => d.id === chip.deliveryId);
        if (found) seen.set(found.state, found);
      }
    }
    for (const state of [
      "queued",
      "sending",
      "acknowledged",
      "rejected",
      "timed_out",
      "outcome_unknown",
      "failed",
      "cancelled",
    ] as const) {
      expect(seen.has(state), state).toBe(true);
    }
    expect(seen.get("rejected")?.attemptLog.at(-1)).toMatchObject({
      result: "rejected",
      ackLevel: "none",
    });
    expect(seen.get("rejected")?.attemptLog.at(-1)?.ackCode).toMatch(/^(AE|AR|CE)$/);
    expect(seen.get("timed_out")?.attemptLog.at(-1)).toMatchObject({
      result: "ack_timeout",
      errorCode: "ack_timeout",
    });
    expect(seen.get("sending")?.attemptLog.at(-1)).toMatchObject({ result: null, endedAt: null });
  });

  it("keeps each delivery consistent: attempt numbers, times, levels and reasons", async () => {
    const api = create();
    for (const row of (await rowsOf(api, {})).slice(0, 600)) {
      for (const d of (await api.getMessage(row.id)).deliveries) {
        expect(d.messageId).toBe(row.id);
        expectAttemptsOrdered(d);
        expectStateFitsAttempts(d);
      }
    }
  });

  it("proves no more than the destination's protocol can", async () => {
    const api = create();
    const destinations = new Map<string, DestinationProtocol>();
    const levels = new Set<string>();
    for (const row of (await rowsOf(api, { states: ["routed"] })).slice(0, 600)) {
      for (const d of (await api.getMessage(row.id)).deliveries) {
        if (d.state !== "acknowledged") continue;
        await learnDestinations(api, d.channelId, destinations);
        expect(d.ackLevel).toBe(
          highestLevel(destinations.get(`${d.channelId}/${d.destinationId}`)),
        );
        levels.add(d.ackLevel);
      }
    }
    expect([...levels].sort()).toEqual(["accepted", "handed_off", "received"]);
  });

  it("reads each delivery's transformed body, and none for a failed mapping", async () => {
    const api = create();
    const { detail, delivery } = await withDelivery(api, "acknowledged", { states: ["routed"] });
    const body = await api.getMessageBody({
      messageId: detail.summary.id,
      which: { kind: "transformed", deliveryId: delivery.id },
      as: "text",
    });
    expect(body).toMatchObject({ hash: delivery.transformedHash, truncated: false });
    expect(body.byteLength).toBeGreaterThan(0);
    const failed = await withDelivery(
      api,
      "failed",
      { deliverySummaries: ["attention"] },
      (d) => d.reasonCode === "transform_error",
    );
    await expect(
      api.getMessageBody({
        messageId: failed.detail.summary.id,
        which: { kind: "transformed", deliveryId: failed.delivery.id },
        as: "text",
      }),
    ).rejects.toMatchObject({ code: "not_found" });
    await expect(
      api.getMessageBody({
        messageId: detail.summary.id,
        which: { kind: "transformed", deliveryId: "dlv-none" },
        as: "text",
      }),
    ).rejects.toMatchObject({ code: "not_found" });
  });

  it("answers not_found for a message that does not exist", async () => {
    const api = create();
    await expect(api.getMessage("msg-99999")).rejects.toMatchObject({ code: "not_found" });
    await expect(api.getMessage("garbage")).rejects.toMatchObject({ code: "not_found" });
    await expect(
      api.getMessageBody({ messageId: "msg-99999", which: { kind: "original" }, as: "text" }),
    ).rejects.toMatchObject({ code: "not_found" });
  });
});
