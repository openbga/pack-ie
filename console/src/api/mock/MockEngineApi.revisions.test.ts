import { describe, expect, it } from "vitest";
import type { AuditEvent, MessageDetail } from "../types";
import type { MockEngineApi } from "./index";
import { mockApiFactory, rejection } from "./mockTestSupport";

const create = mockApiFactory({ messageCount: 2000, seed: 7 });

/** Every message of the history that a newer revision supersedes. */
async function superseded(api: MockEngineApi, count: number): Promise<MessageDetail[]> {
  const found: MessageDetail[] = [];
  for (let i = 0; i < count; i++) {
    const detail = await api.getMessage(`msg-${i}`);
    if (detail.revisedBy !== null) found.push(detail);
  }
  return found;
}

async function resendEvents(api: MockEngineApi): Promise<AuditEvent[]> {
  return (
    await api.getAuditEvents({ filter: { action: "message_resend" }, beforeSeq: null, limit: 4 })
  ).events;
}

describe("revision chains", () => {
  it("lay chains of two or three messages over the history, the same ones in every run", async () => {
    const first = await superseded(create(), 2000);
    expect(first.length).toBeGreaterThanOrEqual(10);
    const second = await superseded(create(), 2000);
    expect(second.map((d) => [d.summary.id, d.revisedBy])).toEqual(
      first.map((d) => [d.summary.id, d.revisedBy]),
    );
  });

  it("have easy-to-find superseded messages in small and in default histories", async () => {
    for (const messageCount of [60, 300, 412_000]) {
      const api = create({ messageCount });
      const ids = Array.from({ length: Math.min(messageCount, 400) }, (_, i) => `msg-${i}`);
      const details = await Promise.all(ids.map((id) => api.getMessage(id)));
      expect(details.some((d) => d.revisedBy !== null)).toBe(true);
    }
  });

  it("link each superseded message to the newest revision, which is later and links back", async () => {
    const api = create();
    for (const old of (await superseded(api, 2000)).slice(0, 12)) {
      const newest = await api.getMessage(old.revisedBy ?? "");
      expect(newest.summary.receivedAt > old.summary.receivedAt).toBe(true);
      expect(newest.summary.channelId).toBe(old.summary.channelId);
      expect(newest.revisedBy).toBeNull();
      expect(newest.revisionOf).not.toBeNull();
      const before = await api.getMessage(newest.revisionOf ?? "");
      expect(before.summary.receivedAt < newest.summary.receivedAt).toBe(true);
      expect(before.revisedBy).toBe(newest.summary.id);
    }
  });

  it("keep msg-40 superseded by msg-15 for the default seed, whatever the history size: the journeys open them", async () => {
    for (const messageCount of [200, 2000]) {
      const api = create({ messageCount, seed: 20_260_611 });
      expect((await api.getMessage("msg-40")).revisedBy).toBe("msg-15");
      expect((await api.getMessage("msg-15")).revisionOf).toBe("msg-40");
    }
  });

  it("let msg-69 be revised late by msg-61 for the default seed: the journey of a refusal uses them", async () => {
    const api = create({ messageCount: 2000, seed: 20_260_611 });
    expect((await api.getMessage("msg-69")).revisedBy).toBeNull();
    expect(api.reviseMessage("msg-69")).toBe("msg-61");
    expect(api.reviseMessage("msg-69")).toBeNull();
  });

  it("do not revise what cannot be revised late", async () => {
    const api = create({ messageCount: 2000, seed: 20_260_611 });
    // msg-77 is invalid: no corrected result replaces a message that never parsed.
    expect(api.reviseMessage("msg-77")).toBeNull();
  });

  it("make some chains of three: the middle message revises one and is revised by another", async () => {
    const api = create();
    const middle = (await superseded(api, 2000)).filter((d) => d.revisionOf !== null);
    expect(middle.length).toBeGreaterThan(0);
  });
});

describe("resending a superseded message", () => {
  async function oldest(api: MockEngineApi): Promise<MessageDetail> {
    const found = (await superseded(api, 2000)).find((d) => d.revisionOf === null);
    if (found === undefined) throw new Error("no superseded message");
    return found;
  }

  it("is refused as a whole, naming the newer message and when it was received", async () => {
    const api = create();
    const old = await oldest(api);
    const newer = await api.getMessage(old.revisedBy ?? "");
    const error = await rejection(
      api.resendMessage({
        messageId: old.summary.id,
        destinations: "all",
        mode: "new_message",
        confirmDuplicateRisk: true,
      }),
    );
    expect(error).toMatchObject({ code: "conflict", rejection: "superseded" });
    expect(error.message).toBe(
      `A newer revision of this message exists (${newer.summary.id}, received ${newer.summary.receivedAt}). Resending this older version may reissue a corrected result; confirm that it is intended to resend it anyway.`,
    );
    expect((await api.getMessage(old.summary.id)).deliveries).toEqual(old.deliveries);
  });

  it("goes ahead once confirmed, and the audit log records the confirmation", async () => {
    const api = create();
    const old = await oldest(api);
    const result = await api.resendMessage({
      messageId: old.summary.id,
      destinations: "all",
      mode: "new_message",
      confirmDuplicateRisk: true,
      confirmSuperseded: true,
    });
    expect(result.newMessageId).not.toBeNull();
    expect((await resendEvents(api))[0]).toMatchObject({
      outcome: "succeeded",
      objectId: old.summary.id,
      details: { mode: "new_message", confirm_superseded: true, confirm_duplicate_risk: true },
    });
  });

  it("files a refusal as rejected, with confirm_superseded false", async () => {
    const api = create();
    const old = await oldest(api);
    await rejection(
      api.resendMessage({
        messageId: old.summary.id,
        destinations: "all",
        mode: "new_message",
        confirmDuplicateRisk: true,
      }),
    );
    expect((await resendEvents(api))[0]).toMatchObject({
      outcome: "rejected",
      details: { confirm_superseded: false },
    });
  });

  it("is not asked of a message nothing supersedes", async () => {
    const api = create();
    const newest = await api.getMessage((await oldest(api)).revisedBy ?? "");
    const result = await api.resendMessage({
      messageId: newest.summary.id,
      destinations: "all",
      mode: "new_message",
      confirmDuplicateRisk: true,
    });
    expect(result.destinations.length).toBeGreaterThan(0);
  });

  it("is refused after a newer revision arrives, which reviseMessage does for a journey", async () => {
    const api = create();
    const plain = (
      await api.listMessages({
        filter: { states: ["routed"] },
        sort: { field: "receivedAt", descending: true },
        offset: 60,
        limit: 1,
      })
    ).rows[0];
    const id = plain?.id ?? "";
    expect((await api.getMessage(id)).revisedBy).toBeNull();
    const newer = api.reviseMessage(id);
    expect(newer).not.toBeNull();
    expect((await api.getMessage(id)).revisedBy).toBe(newer);
    expect((await api.getMessage(newer ?? "")).revisionOf).toBe(id);
    expect(
      await rejection(
        api.resendMessage({
          messageId: id,
          destinations: "all",
          mode: "new_message",
          confirmDuplicateRisk: true,
        }),
      ),
    ).toMatchObject({ rejection: "superseded" });
  });
});
