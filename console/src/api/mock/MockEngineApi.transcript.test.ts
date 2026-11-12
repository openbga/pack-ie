import { describe, expect, it, vi } from "vitest";
import type { EngineError, TrafficSegment } from "../types";
import type { MockEngineApi } from "./index";
import { channelOf, mockApiFactory } from "./mockTestSupport";

const create = mockApiFactory({ messageCount: 500, seed: 7 });

function collect(api: MockEngineApi, id: string) {
  const segments: TrafficSegment[] = [];
  const stop = api.subscribeTraffic(id, (batch) => segments.push(...batch));
  return { segments, stop };
}

const decode = (segments: TrafficSegment[], direction: TrafficSegment["direction"]) =>
  segments.filter((s) => s.direction === direction).map((s) => atob(s.base64));

const length = (texts: string[]) => texts.reduce((n, text) => n + text.length, 0);

describe("bridge transcript", () => {
  it("shows both directions of ASTM: ENQ, frames and EOT one way, ACK the other", async () => {
    vi.useFakeTimers();
    const api = create();
    const { segments, stop } = collect(api, "ch-br-ci4100");
    vi.advanceTimersByTime(20_000);
    stop();
    const forward = decode(segments, "a_to_b");
    const reverse = decode(segments, "b_to_a");
    expect(forward).toContain("\x05");
    expect(forward).toContain("\x04");
    expect(forward.some((d) => d.startsWith("\x02") && d.endsWith("\r\n"))).toBe(true);
    expect(reverse.length).toBeGreaterThan(0);
    expect(reverse.every((d) => d === "\x06")).toBe(true);
    expect(segments.every((s) => !Number.isNaN(Date.parse(s.at)))).toBe(true);
  });

  it("shows HL7 over MLLP with the acknowledgement in the reverse direction", async () => {
    vi.useFakeTimers();
    const api = create();
    const { segments, stop } = collect(api, "ch-br-dxh900");
    vi.advanceTimersByTime(12_000);
    stop();
    const forward = decode(segments, "a_to_b");
    const reverse = decode(segments, "b_to_a");
    expect(forward.some((d) => d.startsWith("\x0bMSH|") && d.endsWith("\x1c\r"))).toBe(true);
    expect(reverse.some((d) => d.startsWith("\x0bMSH|") && d.includes("MSA|AA"))).toBe(true);
  });

  it("shows AU frames with ACK back, and plain bytes without a framer", async () => {
    vi.useFakeTimers();
    const api = create();
    const au = collect(api, "ch-br-au5800");
    const raw = collect(api, "ch-br-b221");
    vi.advanceTimersByTime(12_000);
    au.stop();
    raw.stop();
    expect(decode(au.segments, "b_to_a").every((d) => d === "\x06")).toBe(true);
    expect(decode(au.segments, "a_to_b").length).toBeGreaterThan(0);
    expect(decode(raw.segments, "a_to_b").some((d) => d.endsWith("\r\n"))).toBe(true);
  });

  it("books exactly what it shows on the channel and shows one viewer", async () => {
    vi.useFakeTimers();
    const api = create();
    const id = "ch-br-ci4100";
    const before = await channelOf(api, id);
    api.subscribeSnapshots(() => {});
    const { segments, stop } = collect(api, id);
    vi.advanceTimersByTime(15_000);
    const watched = await channelOf(api, id);
    expect(watched.tapActive).toBe(true);
    expect(watched.bridge?.tap?.transcriptViewers).toBe(1);
    stop();
    const after = await channelOf(api, id);
    const forward = decode(segments, "a_to_b");
    const reverse = decode(segments, "b_to_a");
    expect(segments.length).toBeGreaterThan(10);
    expect((after.bridge?.bytesAToB ?? 0) - (before.bridge?.bytesAToB ?? 0)).toBe(length(forward));
    expect((after.bridge?.bytesBToA ?? 0) - (before.bridge?.bytesBToA ?? 0)).toBe(length(reverse));
    expect((after.bridge?.messagesAToB ?? 0) - (before.bridge?.messagesAToB ?? 0)).toBe(
      forward.filter((d) => d === "\x04").length,
    );
    expect(after.bridge?.tap?.transcriptViewers).toBe(0);
    expect(after.tapActive).toBe(false);
  });
});

describe("message channel transcript", () => {
  it("shows the source side a_to_b and the destination side b_to_a", async () => {
    vi.useFakeTimers();
    const api = create();
    const id = "ch-msg-xn1000";
    const detail = await api.getChannel(id);
    const before = detail.snapshot;
    const { segments, stop } = collect(api, id);
    vi.advanceTimersByTime(30_000);
    stop();
    const incoming = decode(segments, "a_to_b");
    const outgoing = decode(segments, "b_to_a");
    expect(incoming).toContain("\x05");
    expect(outgoing.length).toBeGreaterThan(0);
    expect(detail.destinations[0]?.protocol).toBe("hl7_mllp");
    expect(outgoing.some((d) => d.startsWith("\x0bMSH|"))).toBe(true);
    expect(outgoing.includes("\x05")).toBe(false);
    // The channel counts what the source sent and what the engine sent on, not the replies.
    const received = segments.filter((s) => s.direction === "a_to_b" && s.flow === "inbound");
    const sent = segments.filter((s) => s.direction === "b_to_a" && s.flow === "outbound");
    const after = await channelOf(api, id);
    expect(after.counters.bytesIn - before.counters.bytesIn).toBe(
      length(decode(received, "a_to_b")),
    );
    expect(after.counters.bytesOut - before.counters.bytesOut).toBe(length(decode(sent, "b_to_a")));
    expect(after.source?.received).toBe(after.counters.messagesIn);
  });

  it("marks each segment with the way it went: the engine's response to the source and each destination's reply", async () => {
    vi.useFakeTimers();
    const api = create();
    const id = "ch-msg-sta";
    const { segments, stop } = collect(api, id);
    vi.advanceTimersByTime(30_000);
    stop();
    const source = segments.filter((s) => s.direction === "a_to_b");
    const destinations = segments.filter((s) => s.direction === "b_to_a");
    const detail = await api.getChannel(id);
    const sourceEndpoint = detail.snapshot.endpoints[0]?.endpointId;
    expect(source.some((s) => s.flow === "inbound")).toBe(true);
    expect(source.some((s) => s.flow === "outbound" && atob(s.base64) === "\x06")).toBe(true);
    expect(source.every((s) => s.endpointId === sourceEndpoint && s.destinationId === null)).toBe(
      true,
    );
    expect(destinations.some((s) => s.flow === "outbound")).toBe(true);
    expect(destinations.some((s) => s.flow === "inbound")).toBe(true);
    expect(destinations.every((s) => s.destinationId !== null)).toBe(true);
  });

  it("gives each message-channel segment its role: received, response, sent or reply", async () => {
    vi.useFakeTimers();
    const api = create();
    const { segments, stop } = collect(api, "ch-msg-sta");
    vi.advanceTimersByTime(30_000);
    stop();
    const rolesOf = (pane: "a_to_b" | "b_to_a") =>
      new Set(segments.filter((s) => s.direction === pane).map((s) => s.role));
    expect([...rolesOf("a_to_b")].sort()).toEqual(["received", "response"]);
    expect([...rolesOf("b_to_a")].sort()).toEqual(["reply", "sent"]);
    expect(segments.filter((s) => s.role === "reply").every((s) => s.flow === "inbound")).toBe(
      true,
    );
  });

  it("gives bridge segments no role", async () => {
    vi.useFakeTimers();
    const api = create();
    const { segments, stop } = collect(api, "ch-br-ci4100");
    vi.advanceTimersByTime(30_000);
    stop();
    expect(segments.length).toBeGreaterThan(0);
    expect(segments.every((s) => s.role === undefined)).toBe(true);
  });

  it("tags the Destinations pane with every enabled destination", async () => {
    vi.useFakeTimers();
    const api = create();
    const id = "ch-msg-sta";
    const detail = await api.getChannel(id);
    const ids = detail.destinations.map((d) => d.destinationId);
    expect(ids).toHaveLength(2);
    const { segments, stop } = collect(api, id);
    vi.advanceTimersByTime(30_000);
    stop();
    const tagged = new Set(segments.map((s) => s.destinationId).filter((d) => d !== null));
    expect([...tagged].sort()).toEqual([...ids].sort());
    const [first] = ids;
    const replies = segments.filter((s) => s.destinationId === first && s.flow === "inbound");
    expect(replies.length).toBeGreaterThan(0);
    expect(replies.every((s) => atob(s.base64).startsWith("\x0bMSH|"))).toBe(true);
  });

  it("sends nothing to a disabled destination", async () => {
    vi.useFakeTimers();
    const api = create();
    const id = "ch-msg-sta";
    const config = (await api.getChannel(id)).config;
    if (config.kind !== "message") throw new Error("not a message channel");
    const [first, second] = config.destinations;
    if (first === undefined || second === undefined) throw new Error("fixture");
    const document = await api.getConfig();
    const channel = document.channels.find((c) => c.id === id);
    if (channel?.kind !== "message") throw new Error("not a message channel");
    channel.destinations = channel.destinations.map((d) =>
      d.id === second.id ? { ...d, enabled: false } : d,
    );
    await api.applyConfig(document, "disable a destination");
    const { segments, stop } = collect(api, id);
    vi.advanceTimersByTime(30_000);
    stop();
    const tagged = new Set(segments.map((s) => s.destinationId).filter((d) => d !== null));
    expect([...tagged]).toEqual([first.id]);
  });
});

describe("one transcript viewer per channel", () => {
  /** What the engine says to a second viewer of `id`: delivered after subscribing, like the IPC client's. */
  function conflict(api: MockEngineApi, id: string): Promise<EngineError> {
    return new Promise((resolve) => {
      api.subscribeTraffic(
        id,
        () => {},
        (error) => resolve(error),
      );
    });
  }

  it("rejects a second viewer at once and frees the channel when the first leaves", async () => {
    vi.useFakeTimers();
    const api = create();
    const first = collect(api, "ch-br-au5800");
    const error = await conflict(api, "ch-br-au5800");
    expect(error).toMatchObject({
      code: "conflict",
      message: "Transcript in use by another client",
      fieldErrors: [],
    });
    expect(error.correlationId).toMatch(/^c-[0-9a-f]{6}$/);
    const other = collect(api, "ch-br-e801");
    other.stop();
    first.stop();
    first.stop();
    const again = collect(api, "ch-br-au5800");
    again.stop();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("does not hold the channel after the failed attempt", async () => {
    const api = create();
    const first = collect(api, "ch-msg-au680");
    await conflict(api, "ch-msg-au680");
    first.stop();
    expect(() => api.subscribeTraffic("ch-msg-au680", () => {})()).not.toThrow();
  });
});

describe("transcript of a channel that moves no bytes", () => {
  it("stays silent and books nothing while the channel is paused", async () => {
    vi.useFakeTimers();
    const api = create();
    const id = "ch-msg-vitros350";
    const before = await channelOf(api, id);
    const { segments, stop } = collect(api, id);
    vi.advanceTimersByTime(10_000);
    stop();
    expect(segments).toEqual([]);
    expect((await channelOf(api, id)).counters).toEqual(before.counters);
  });

  it("stops flowing when the channel is paused while watched", async () => {
    vi.useFakeTimers();
    const api = create();
    const id = "ch-br-ci4100";
    const { segments, stop } = collect(api, id);
    vi.advanceTimersByTime(5000);
    const seen = segments.length;
    expect(seen).toBeGreaterThan(0);
    await api.pauseChannel(id);
    vi.advanceTimersByTime(10_000);
    expect(segments).toHaveLength(seen);
    expect((await channelOf(api, id)).tapActive).toBe(true);
    stop();
  });
});
