import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { renderRoutes } from "@/lib/test/router";
import {
  ago,
  alert,
  bridgeStatus,
  channel,
  channelDetail,
  engineError,
  failingChannel,
  side,
  snapshot,
  stubApi,
} from "../shared-operate/testFixtures";
import { channelRoutes, logEvent, renderChannel } from "./testFixtures";

/** A bridge that is retrying, with a tap, an overflow episode and everything filled in. */
function busyBridge() {
  return channel({
    id: "ch-alpha",
    label: "Alpha bridge",
    state: "retrying",
    nextRetryAt: new Date(Date.now() + 7000).toISOString(),
    counters: { bytesIn: 0, bytesOut: 0, messagesIn: 0, messagesOut: 0, retries: 3 },
    configVersion: 6,
    instanceStartedAt: ago(600),
    buffers: [
      {
        direction: "a_to_b",
        usedBytes: 32_768,
        capacityBytes: 65_536,
        segments: 12,
        maxSegments: 256,
        overflowing: true,
      },
      {
        direction: "b_to_a",
        usedBytes: 0,
        capacityBytes: 65_536,
        segments: 0,
        maxSegments: 256,
        overflowing: false,
      },
    ],
    bridge: bridgeStatus({
      sessionState: "closed",
      endpointA: side({ consecutiveFailureCount: 2, activePeer: null }),
      endpointB: side({
        endpointId: "ep-lis",
        label: "LIS listener",
        kind: "tcp_listener",
        role: "b",
        state: "retrying",
        consecutiveFailureCount: 4,
        activePeer: "10.0.4.12:51044",
        lastActivityAt: ago(90),
      }),
      bytesAToB: 2048,
      bytesBToA: 512,
      messagesAToB: 10,
      messagesBToA: 12,
      framingErrors: 1,
      tapDropped: 0,
      overflowCount: 2,
      tap: { framer: "hl7_mllp", recordMessages: false, transcriptViewers: 0 },
    }),
    recentErrors: [
      { at: ago(5), summary: "LIS listener write failed" },
      { at: ago(300), summary: "Analyzer COM3 read failed" },
    ],
    lastError: {
      at: ago(5),
      summary:
        'Channel ch-alpha B endpoint "LIS listener" write failed: peer reset the connection. The engine retries.',
      nextAction: "Check the LIS; the engine retries on its own.",
    },
  });
}

describe("ChannelPage, bridge", () => {
  it("shows every required item: identity, state, endpoints, bytes, framing, buffers, retry, error, versions", async () => {
    const detail = channelDetail(busyBridge());
    const { container } = renderChannel(detail, {}, detail.snapshot, {
      node: {
        ...snapshot().node,
        configVersion: 8,
        pendingConfigVersion: 9,
      },
    });

    expect(await screen.findByRole("heading", { level: 1, name: "Alpha bridge" })).toBeTruthy();
    const facts = (await screen.findByText("Channel id")).closest("dl") as HTMLElement;
    expect(facts.textContent).toContain("ch-alpha");
    expect(facts.textContent).toMatch(/Retrying in [1-7] s/);
    expect(facts.textContent).toContain("SessionClosed");
    expect(facts.textContent).toContain("Configuration version6 (not the active version)");
    expect(facts.textContent).toContain("Active configurationVersion 8");
    expect(facts.textContent).toContain("Pending configurationVersion 9");
    expect(facts.textContent).toContain("Retries3");
    expect(facts.textContent).toMatch(/Next retry\s*in [1-7] s/);

    const endpoints = screen.getByRole("table", { name: "Endpoints of this channel" });
    const [a, b] = within(endpoints).getAllByRole("row").slice(1);
    expect(a?.textContent).toContain("Side A Analyzer COM3 Connected");
    expect(a?.textContent).toContain("Serial");
    expect(b?.textContent).toContain("Side B LIS listener Retrying");
    expect(b?.textContent).toContain("TCP listener");
    expect(b?.textContent).toContain("10.0.4.12:51044");
    expect(b?.textContent).toContain("4");
    expect(b?.textContent).toContain("1 min ago");
    expect(within(endpoints).getByRole("link", { name: "LIS listener" }).getAttribute("href")).toBe(
      "/endpoints/ep-lis",
    );

    const traffic = screen.getByRole("table", { name: "Bytes by direction" });
    const [forward, backward] = within(traffic).getAllByRole("row").slice(1);
    expect(forward?.textContent).toContain("Analyzer COM3 → LIS listener2.0 KB10");
    expect(backward?.textContent).toContain("LIS listener → Analyzer COM3512 B12");
    expect(screen.getByText("Framing errors").nextElementSibling?.textContent).toBe("1");

    const meter = screen.getByRole("meter", { name: "Analyzer COM3 → LIS listener" });
    expect(meter.getAttribute("aria-valuetext")).toContain(
      "32.0 KB of 64.0 KB · 12 of 256 segments",
    );
    expect(screen.getByText("Overflow episodes").nextElementSibling?.textContent).toBe("2");
    expect(screen.getByText(/Overflowing: the writer waits/)).toBeTruthy();

    const error = screen.getByText("Last error").closest("[role=alert]") as HTMLElement;
    const details = error.querySelector("dl")?.textContent;
    expect(details).toContain("Operationwrite");
    expect(details).toContain("EndpointB: LIS listener");
    expect(details).toContain("Causepeer reset the connection");
    expect(details).toContain("Failure streak4");
    expect(details).toMatch(/RetryNext attempt in [1-7] s \(retry 3\)/);
    expect(error.textContent).toContain("Check the LIS; the engine retries on its own.");

    expect(
      screen.getByRole("table", { name: "Recent errors of this channel" }).textContent,
    ).toContain("Analyzer COM3 read failed");
    expect(screen.getByRole("link", { name: "Edit" }).getAttribute("href")).toBe(
      "/channels/ch-alpha/edit",
    );
    expect(screen.getByRole("link", { name: "Channels" }).getAttribute("href")).toBe("/channels");
    await expectNoA11yViolations(container);
  });

  it("lists the channel's recent events with a link to Logs for this channel", async () => {
    const queryLogs = vi.fn(async () => ({
      events: [
        logEvent(1, "Channel state changed to retrying"),
        logEvent(2, "Bridge session closed", "warn"),
      ],
      olderCursor: null,
    }));
    renderChannel(channelDetail(busyBridge()), { queryLogs });
    const table = await screen.findByRole("table", { name: "Recent events of this channel" });
    expect(table.textContent).toContain("Channel state changed to retrying");
    expect(table.textContent).toContain("Bridge session closed");
    expect(queryLogs).toHaveBeenCalledWith({ channelId: "ch-alpha" }, { before: null, limit: 20 });
    expect(screen.getByRole("link", { name: "View in Logs" }).getAttribute("href")).toBe(
      "/logs?channel=ch-alpha",
    );
  });

  it("says when there are no events, and shows an error with retry when they cannot be loaded", async () => {
    const queryLogs = vi
      .fn()
      .mockRejectedValueOnce(engineError("engine_unavailable", "The engine is not running."))
      .mockResolvedValue({ events: [], olderCursor: null });
    const { user } = renderChannel(channelDetail(channel({ id: "ch-alpha" })), { queryLogs });
    expect(await screen.findByText("The events could not be loaded")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByText("No events are recorded for this channel yet.")).toBeTruthy();
  });

  it("lists the channel's active alerts from the snapshot, with the next step", async () => {
    const live = channel({ id: "ch-alpha" });
    renderChannel(channelDetail(live), {}, live, {
      openAlerts: [
        alert({ id: "a1", channelId: "ch-alpha", severity: "critical", title: "Endpoint down" }),
        alert({ id: "a2", channelId: "other", title: "Not this channel" }),
      ],
    });
    const list = await screen.findByRole("list", { name: "Active alerts of this channel" });
    expect(list.textContent).toContain("Critical");
    expect(list.textContent).toContain("Endpoint down");
    expect(list.textContent).toContain("Check the serial cable");
    expect(list.textContent).not.toContain("Not this channel");
  });

  it("handles every field the engine does not report, without inventing values", async () => {
    const bare = channel({
      id: "ch-alpha",
      configVersion: null,
      instanceStartedAt: null,
      bridge: null,
      buffers: [],
      recentErrors: [],
      lastError: null,
    });
    const { container } = renderChannel(channelDetail(bare), {}, bare, {
      node: { ...snapshot().node, totals: null, store: null },
    });
    expect(await screen.findByRole("heading", { level: 1, name: "Alpha bridge" })).toBeTruthy();
    const facts = screen.getByText("Channel id").closest("dl") as HTMLElement;
    await waitFor(() => expect(facts.textContent).toContain("Pending configurationNone"));
    expect(facts.textContent).toContain("Configuration versionNot reported");
    expect(facts.textContent).not.toContain("Running since");
    expect(screen.getAllByText("Not reported").length).toBeGreaterThan(3);
    expect(screen.getByText("No buffer data yet. The channel has not started.")).toBeTruthy();
    expect(screen.getByText("No recent errors.")).toBeTruthy();
    expect(screen.getByText("No active alerts for this channel.")).toBeTruthy();
    await expectNoA11yViolations(container);
  });

  it("without a tap shows that no framer counts messages", async () => {
    renderChannel(channelDetail(channel({ id: "ch-alpha" })));
    const traffic = await screen.findByRole("table", { name: "Bytes by direction" });
    expect(within(traffic).getAllByText("No framer")).toHaveLength(2);
  });

  it("shows a failing bridge's state, last error and buffer", async () => {
    const detail = channelDetail(failingChannel());
    renderChannel(detail);
    expect(await screen.findByRole("heading", { level: 1, name: "Zulu bridge" })).toBeTruthy();
    expect(screen.getAllByText("Degraded").length).toBeGreaterThan(0);
    const note = screen.getByText("Buffer full: the LIS stopped reading").closest("[role=alert]");
    expect(note?.textContent).toContain("Check that the LIS listener is running");
  });

  it("shows a paused channel as paused by the operator, with its stop reason and no retry", async () => {
    const detail = channelDetail(channel({ id: "ch-alpha" }));
    renderChannel(
      detail,
      {},
      channel({ id: "ch-alpha", state: "stopped", stopReason: "paused", instanceStartedAt: null }),
    );
    expect((await screen.findAllByText("Paused by operator")).length).toBeGreaterThan(0);
    expect(screen.getByText("Paused by an operator")).toBeTruthy();
    expect(screen.getByText(/No retry is scheduled: paused by operator/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Resume" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Pause" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Restart" })).toBeNull();
  });

  it("says the channel was not found, with a way back to the list", async () => {
    renderRoutes(channelRoutes, "/channels/gone", {
      api: stubApi({
        getChannel: async () => {
          throw engineError("not_found", "No longer exists");
        },
        getSnapshot: async () => snapshot(),
      }),
    });
    expect(
      await screen.findByRole("heading", { level: 1, name: "Channel not found" }),
    ).toBeTruthy();
    expect(screen.getByRole("link", { name: "Back to channels" }).getAttribute("href")).toBe(
      "/channels",
    );
  });

  it("shows the heading while loading and an error with retry when the engine fails", async () => {
    const getChannel = vi
      .fn()
      .mockRejectedValueOnce(engineError("engine_unavailable", "The engine is not running."))
      .mockResolvedValue(channelDetail());
    const { user } = renderRoutes(channelRoutes, "/channels/ch-alpha", {
      api: stubApi({
        getChannel,
        getSnapshot: async () => snapshot(),
        queryLogs: async () => ({ events: [], olderCursor: null }),
      }),
    });
    expect(screen.getByRole("heading", { level: 1, name: "Channel" })).toBeTruthy();
    expect(await screen.findByText("The channel could not be loaded")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByRole("heading", { level: 1, name: "Alpha bridge" })).toBeTruthy();
  });
});
