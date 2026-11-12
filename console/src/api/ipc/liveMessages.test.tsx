// Opt-in check of the Messages screens against a RUNNING engine whose message store works. Not
// part of the normal suite: it runs only when PACKIE_LIVE_IPC names the engine's socket, like
// `live.test.tsx`, and it needs a message channel to feed:
//
//   PACKIE_LIVE_IPC=$PWD/../state/packie.sock PACKIE_LIVE_HL7_PORT=2575 \
//     npx vitest run src/api/ipc/liveMessages.test.tsx          (in console/)
//
// `PACKIE_LIVE_HL7_PORT` is the port of the `tcp_listener` endpoint of an HL7 over MLLP message
// channel (as in tests/scenarios/msg-resend.json: the source answers `original` ACKs, and the
// destination is an HL7 LIS). `PACKIE_LIVE_HL7_CHANNEL` names that channel when the engine has
// several; by default it is the first message channel whose protocol is `hl7_mllp`.
//
// The test sends synthetic HL7 messages (no patient data) over MLLP, then expects the console
// to show them as the engine stored them: in the grid with their deliveries, in the record with
// bodies opened on request, and through resend and close. If the engine answers
// `not_implemented` to a store query, or reports the store unavailable, the test says so and
// skips: that is not a failure. The deliveries it leaves unresolved are closed at the end with a
// reason; the messages stay (retention removes them).
//
// A small bridge (test/liveHarness.ts) plays the part of the Tauri shell over the real socket, so
// everything above it, `IpcEngineApi` and the pages, is the production code.
import { screen, within } from "@testing-library/react";
import { describe, expect, it, type TestContext } from "vitest";
import type { Delivery, EngineError, MessageDetail, MessageSummary } from "@/api/types";
import { MessagePage } from "@/features/messages/MessagePage";
import { MessagesPage } from "@/features/messages/MessagesPage";
import { mockGridLayout } from "@/lib/test/layout";
import { renderRoutes } from "@/lib/test/router";
import type { IpcEngineApi } from "./IpcEngineApi";
import { connect, liveEnv, SOCKET, until } from "./test/liveHarness";
import { ackCode, result, sendMllp } from "./test/liveMllp";

const HL7_PORT = Number(liveEnv("PACKIE_LIVE_HL7_PORT") ?? "");
const HL7_CHANNEL = liveEnv("PACKIE_LIVE_HL7_CHANNEL");

// ── Reading what the engine stored ───────────────────────────────────────────────────────

const NEEDS_ATTENTION = new Set(["failed", "rejected", "timed_out", "outcome_unknown"]);
const MAY_BE_PROCESSED = new Set(["timed_out", "outcome_unknown", "acknowledged"]);

/** Why the store cannot be used, in words, or null when it can. */
async function storeProblem(api: IpcEngineApi): Promise<string | null> {
  const store = (await api.getSnapshot()).node.store;
  if (store?.state === "unavailable" || store?.state === "not_configured") {
    return `the engine reports its message store ${store.state}`;
  }
  try {
    await api.listMessages({
      filter: {},
      sort: { field: "receivedAt", descending: true },
      offset: 0,
      limit: 1,
    });
    return null;
  } catch (error) {
    if ((error as EngineError).code === "not_implemented") {
      return `the engine answers not_implemented to store queries: ${(error as EngineError).message}`;
    }
    throw error;
  }
}

/** The message channel to feed: the named one, or the first HL7 one. */
async function hl7Channel(api: IpcEngineApi): Promise<string> {
  const channels = (await api.getSnapshot()).channels.filter((c) => c.kind === "message");
  const found = HL7_CHANNEL
    ? channels.find((c) => c.id === HL7_CHANNEL)
    : channels.find((c) => c.protocol === "hl7_mllp");
  if (found === undefined) throw new Error("the engine has no HL7 message channel to feed");
  return found.id;
}

/** Connects and finds the channel to feed; skips the test unless the store can be read. */
async function openFeed(context: TestContext) {
  const { api, close } = await connect("packie-console live messages test");
  try {
    const problem = await storeProblem(api);
    if (problem !== null) context.skip(`the message store cannot be read: ${problem}`);
    return { api, close, channelId: await hl7Channel(api) };
  } catch (error) {
    close();
    throw error;
  }
}

/** The stored message with this control id on the channel, once it is committed. */
async function stored(
  api: IpcEngineApi,
  channelId: string,
  control: string,
  accept: (row: MessageSummary) => boolean = () => true,
): Promise<MessageSummary> {
  const page = await until(
    `message ${control} in the store`,
    () =>
      api.listMessages({
        filter: { channelIds: [channelId], identifier: { value: control, prefix: false } },
        sort: { field: "receivedAt", descending: true },
        offset: 0,
        limit: 10,
      }),
    (answer) => answer.rows.some(accept),
  );
  const row = page.rows.find(accept);
  if (row === undefined) throw new Error(`message ${control} is not stored`);
  return row;
}

/** Closes what a message left unresolved, so the engine is left without work this test made. */
async function closeUnresolved(api: IpcEngineApi, ids: string[]) {
  for (const id of ids) {
    const record = await api.getMessage(id).catch(() => null);
    for (const delivery of record?.deliveries ?? []) {
      if (delivery.state === "queued" || NEEDS_ATTENTION.has(delivery.state)) {
        await api.closeDelivery(delivery.id, "live test: cleaning up").catch(() => undefined);
      }
    }
  }
}

/** The grid shows the message with a chip per destination, and the record opens on a click. */
async function expectInGrid(api: IpcEngineApi, row: MessageSummary, control: string) {
  mockGridLayout();
  const page = renderRoutes(
    [
      { path: "/messages", element: <MessagesPage /> },
      { path: "/messages/:id", element: <MessagePage /> },
    ],
    "/messages",
    { api },
  );
  const search = await screen.findByRole("searchbox", { name: "Identifier" });
  await page.user.type(search, `${control}{Enter}`);
  const cell = await screen.findByRole("gridcell", { name: control }, { timeout: 15_000 });
  const gridRow = cell.closest<HTMLElement>("[role=row]");
  expect(gridRow).not.toBeNull();
  const chips = within(gridRow as HTMLElement).getByRole("list", { name: "Deliveries" });
  expect(within(chips).getAllByRole("listitem")).toHaveLength(row.deliveries.length);
  await page.user.click(cell);
  await screen.findByRole("heading", { level: 2, name: "Deliveries" }, { timeout: 10_000 });
  expect(screen.getByRole("heading", { level: 1, name: control })).toBeTruthy();

  // The original message opens only on request, as text and as hex.
  expect(screen.queryByRole("region", { name: /Original message as/ })).toBeNull();
  await page.user.click(screen.getByRole("button", { name: "Show original message" }));
  const text = await screen.findByRole("region", { name: "Original message as text" });
  expect(text.textContent).toContain("MSH|^~\\&|SIMLAB|SIMFAC|PACKIE|PACKIEFAC");
  expect(text.textContent).toContain(`|${control}|`);
  expect(text.textContent).toContain("<CR>");
  await page.user.click(screen.getByRole("radio", { name: "Hex" }));
  const hex = await screen.findByRole("region", { name: "Original message as hex" });
  expect(hex.textContent).toMatch(/^00000000 {2}4d 53 48 7c /);
  page.unmount();
}

/** The record: identifiers, the response sent to the sender, and each delivery with its body. */
async function expectRecord(api: IpcEngineApi, row: MessageSummary, answer: string[]) {
  const record = await api.getMessage(row.id);
  expect(record.responseCode).toBe(ackCode(answer));
  expect(record.respondedAt).not.toBeNull();
  expect(record.committedAt).not.toBeNull();
  expect(record.contentHash).toMatch(/^[0-9a-f]{64}$/);
  for (const delivery of record.deliveries) {
    expect(delivery.messageId).toBe(row.id);
    expect(["handed_off", "received", "accepted", "none"]).toContain(delivery.ackLevel);
    if (delivery.transformedHash === null) continue;
    const transformed = await api.getMessageBody({
      messageId: row.id,
      which: { kind: "transformed", deliveryId: delivery.id },
      as: "text",
    });
    expect(transformed.hash).toBe(delivery.transformedHash);
    expect(transformed.byteLength).toBeGreaterThan(0);
  }
  return record;
}

/** One delivery resent as the same message: the engine judges it as message-channels.md §9. */
async function expectSameMessageResend(api: IpcEngineApi, id: string, delivery: Delivery) {
  const request = {
    messageId: id,
    destinations: [delivery.destinationId],
    mode: "same_message" as const,
    confirmDuplicateRisk: false,
  };
  const outcome = (await api.resendMessage(request)).destinations[0];
  expect(outcome?.destinationId).toBe(delivery.destinationId);
  if (delivery.state === "queued" || delivery.state === "sending") {
    expect(outcome?.rejected?.code).toBe("delivery_in_progress");
  } else if (MAY_BE_PROCESSED.has(delivery.state)) {
    expect(outcome?.rejected?.code).toBe("duplicate_risk_unconfirmed");
    const confirmed = await api.resendMessage({ ...request, confirmDuplicateRisk: true });
    expect(confirmed.destinations[0]?.rejected).toBeNull();
  } else {
    expect(outcome?.rejected).toBeNull();
  }
}

/** A new message carries the same bytes, with the origin and the link of a resend. */
async function expectNewMessage(api: IpcEngineApi, record: MessageDetail, made: string[]) {
  const fresh = await api.resendMessage({
    messageId: record.summary.id,
    destinations: "all",
    mode: "new_message",
    confirmDuplicateRisk: true,
  });
  expect(fresh.newMessageId).not.toBeNull();
  const newId = fresh.newMessageId ?? "";
  made.push(newId);
  const created = await until(
    "the resent message",
    () => api.getMessage(newId),
    (m) => m.summary.state !== "received",
  );
  expect(created.summary).toMatchObject({ origin: "resend", resentFrom: record.summary.id });
  expect(created.contentHash).toBe(record.contentHash);
  expect(created.responseCode).toBeNull();
}

/** Closing needs a reason; a delivery that needs attention or waits is closed with it. */
async function expectClose(api: IpcEngineApi, id: string) {
  const open = (await api.getMessage(id)).deliveries.find(
    (d) => d.state === "queued" || NEEDS_ATTENTION.has(d.state),
  );
  if (open === undefined) return;
  await expect(api.closeDelivery(open.id, "")).rejects.toBeDefined();
  const reason = "live test: closed on purpose";
  expect(await api.closeDelivery(open.id, reason)).toMatchObject({
    state: "cancelled",
    closedReason: reason,
  });
  await expect(api.closeDelivery(open.id, "again")).rejects.toMatchObject({ code: "conflict" });
}

describe.skipIf(!SOCKET)("the Messages screens against a running engine", () => {
  it("shows a message sent over MLLP with its deliveries, opens its bodies, and resends and closes", async (context) => {
    if (!HL7_PORT) {
      context.skip(
        "PACKIE_LIVE_HL7_PORT is not set: it names the MLLP listener port of an HL7 message channel",
      );
    }
    const { api, close, channelId } = await openFeed(context);
    const made: string[] = [];
    try {
      const control = `LIVE-${Date.now()}`;
      const sample = `SID-${control.slice(-6)}`;

      // The engine commits the message and answers the sender (msg-hl7-in-ack.json).
      const answer = await sendMllp(HL7_PORT, result(control, sample));
      expect(["AA", "CA"]).toContain(ackCode(answer));
      const row = await stored(api, channelId, control, (r) => r.state !== "received");
      made.push(row.id);
      expect(row).toMatchObject({
        channelId,
        protocol: "hl7_mllp",
        origin: "received",
        direction: "inbound",
        state: "routed",
        messageType: "ORU^R01",
        controlId: control,
        sampleId: sample,
        patientId: "PID-TEST-LIVE",
        resentFrom: null,
      });
      expect(row.sizeBytes).toBeGreaterThan(100);
      expect(row.deliveries.length).toBeGreaterThan(0);
      expect(["pending", "attention", "complete"]).toContain(row.deliverySummary);

      await expectInGrid(api, row, control);
      const record = await expectRecord(api, row, answer);
      for (const delivery of record.deliveries) {
        await expectSameMessageResend(api, row.id, delivery);
      }
      const unknown = await api.resendMessage({
        messageId: row.id,
        destinations: ["no-such-destination"],
        mode: "same_message",
        confirmDuplicateRisk: true,
      });
      expect(unknown.destinations[0]?.rejected?.code).toBe("destination_not_found");
      await expectNewMessage(api, record, made);
      await expectClose(api, row.id);
    } finally {
      await closeUnresolved(api, made);
      close();
    }
  }, 120_000);

  it("stores an invalid message and a duplicate as such, with their reasons", async (context) => {
    if (!HL7_PORT) {
      context.skip("PACKIE_LIVE_HL7_PORT is not set");
    }
    const { api, close, channelId } = await openFeed(context);
    const made: string[] = [];
    try {
      // An invalid message is committed and answered negatively; it is never dropped.
      const before = (
        await api.listMessages({
          filter: { channelIds: [channelId], states: ["invalid"] },
          sort: { field: "receivedAt", descending: true },
          offset: 0,
          limit: 1,
        })
      ).total;
      const refused = await sendMllp(HL7_PORT, [`ZZZ|this is not an HL7 message ${Date.now()}`]);
      expect(ackCode(refused)).toBe("AR");
      const invalid = await until(
        "the invalid message",
        () =>
          api.listMessages({
            filter: { channelIds: [channelId], states: ["invalid"] },
            sort: { field: "receivedAt", descending: true },
            offset: 0,
            limit: 1,
          }),
        (page) => page.total > before,
      );
      const bad = invalid.rows[0];
      expect(bad).toMatchObject({
        state: "invalid",
        deliverySummary: "none",
        controlId: null,
        messageType: null,
      });
      expect(bad?.deliveries).toEqual([]);
      const detail = await api.getMessage(bad?.id ?? "");
      expect(detail.responseCode).toBe("AR");
      expect(detail.reasonCode).not.toBeNull();
      const body = await api.getMessageBody({
        messageId: bad?.id ?? "",
        which: { kind: "original" },
        as: "text",
      });
      expect(body.content).toContain("this is not an HL7 message");
      // Only a new message can be made from it (message-channels.md §9).
      const same = await api.resendMessage({
        messageId: bad?.id ?? "",
        destinations: "all",
        mode: "same_message",
        confirmDuplicateRisk: false,
      });
      for (const outcome of same.destinations) {
        expect(outcome.rejected?.code).toBe("invalid_state");
      }

      // The same message twice: the second is a duplicate of the first (msg-duplicate.json).
      const control = `LIVE-DUP-${Date.now()}`;
      const segments = result(control, `SID-${control.slice(-6)}`);
      expect(ackCode(await sendMllp(HL7_PORT, segments))).toMatch(/AA|CA/);
      const original = await stored(api, channelId, control);
      made.push(original.id);
      expect(ackCode(await sendMllp(HL7_PORT, segments))).toMatch(/AA|CA/);
      const duplicate = await stored(api, channelId, control, (r) => r.state === "duplicate");
      made.push(duplicate.id);
      const link = await api.getMessage(duplicate.id);
      expect(link.duplicateOf).toBe(original.id);
      expect(link.deliveries).toEqual([]);
      expect(link.summary.deliverySummary).toBe("none");
    } finally {
      await closeUnresolved(api, made);
      close();
    }
  }, 120_000);

  it("keeps the page's filters to what the engine can answer", async (context) => {
    const { api, close } = await connect("packie-console live messages test");
    try {
      const problem = await storeProblem(api);
      if (problem !== null) context.skip(`the message store cannot be read: ${problem}`);
      const ask = (filter: Parameters<IpcEngineApi["listMessages"]>[0]["filter"]) =>
        api.listMessages({
          filter,
          sort: { field: "receivedAt", descending: true },
          offset: 0,
          limit: 1,
        });
      // Every filter and quick filter of the page is a query the engine accepts.
      const total = (await ask({})).total;
      for (const filter of [
        { deliverySummaries: ["attention" as const] },
        { deliveryStates: ["queued" as const] },
        { states: ["duplicate" as const] },
        { states: ["invalid" as const, "aborted" as const] },
        { directions: ["inbound" as const] },
        { since: new Date(Date.now() - 86_400_000).toISOString() },
      ]) {
        const answer = await ask(filter);
        expect(answer.total).toBeLessThanOrEqual(total);
      }
    } finally {
      close();
    }
  }, 60_000);
});
