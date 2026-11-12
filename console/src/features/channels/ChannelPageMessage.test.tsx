import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { realMessageChannel } from "@/api/ipc/test/realChannel";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import {
  ago,
  alert,
  channel,
  channelDetail,
  destination,
  engineError,
  failingChannel,
  messageChannel,
} from "../shared-operate/testFixtures";
import { logEvent, renderChannel } from "./testFixtures";

describe("ChannelPage, message channel", () => {
  const lis = destination({
    label: "LIS results",
    endpoint: { endpointId: "ep-lis", label: "LIS listener", state: "retrying" },
    sending: 1,
    nextAttemptAt: new Date(Date.now() + 9000).toISOString(),
    lastError: {
      at: ago(20),
      summary: "LIS did not acknowledge in time",
      nextAction: "Check the LIS.",
    },
  });

  it("shows every item: identity, state, source, destinations, alerts, errors, events and links", async () => {
    const live = messageChannel({
      queue: { depth: 7, needsAttention: 3, oldestQueuedAt: ago(90) },
      recentErrors: [{ at: ago(20), summary: "LIS did not acknowledge in time" }],
      lastError: {
        at: ago(20),
        summary: "LIS did not acknowledge in time",
        nextAction: "Check the LIS.",
      },
      state: "degraded",
    });
    const queryLogs = vi.fn(async () => ({
      events: [logEvent(1, "Delivery timed out")],
      olderCursor: null,
    }));
    const { container } = renderChannel(
      channelDetail(live, [lis, destination({ destinationId: "dst-2", label: "Archive" })]),
      { queryLogs },
      live,
      { openAlerts: [alert({ channelId: "ch-hl7", title: "Delivery backlog" })] },
    );

    expect(await screen.findByRole("heading", { level: 1, name: "Results to LIS" })).toBeTruthy();
    expect(screen.getByText("Channel id").nextElementSibling?.textContent).toBe("ch-hl7");
    expect(screen.getByRole("heading", { level: 2, name: "Source connector" })).toBeTruthy();
    expect(screen.getByRole("region", { name: "Destination LIS results" })).toBeTruthy();
    expect(screen.getByRole("region", { name: "Destination Archive" })).toBeTruthy();
    expect(screen.getAllByText("LIS did not acknowledge in time").length).toBeGreaterThan(1);
    expect(screen.getByRole("link", { name: "View messages" }).getAttribute("href")).toBe(
      "/messages?channel=ch-hl7",
    );
    expect(
      screen.getByRole("link", { name: "View unresolved deliveries (3)" }).getAttribute("href"),
    ).toBe("/messages?channel=ch-hl7&quick=attention&range=any");
    const alerts = await screen.findByRole("list", { name: "Active alerts of this channel" });
    expect(alerts.textContent).toContain("Delivery backlog");
    expect(await screen.findByText("Delivery timed out")).toBeTruthy();
    expect(queryLogs).toHaveBeenCalledWith({ channelId: "ch-hl7" }, { before: null, limit: 20 });
    expect(screen.getByRole("link", { name: "View in Logs" }).getAttribute("href")).toBe(
      "/logs?channel=ch-hl7",
    );
    expect(screen.getByRole("heading", { level: 2, name: "Live transcript" })).toBeTruthy();
    expect(screen.getByRole("heading", { level: 3, name: "Source" })).toBeTruthy();
    expect(screen.getByRole("heading", { level: 3, name: "Destinations" })).toBeTruthy();
    await expectNoA11yViolations(container);
  });

  it("handles a source, a queue and destinations the engine does not report", async () => {
    const live = messageChannel({ source: null, queue: null, lastActivityAt: null });
    renderChannel(channelDetail(live), {}, live);
    expect(
      await screen.findByText(
        "Not reported: the engine does not report this channel's source connector.",
      ),
    ).toBeTruthy();
    expect(screen.getAllByText("Not reported").length).toBeGreaterThan(0);
    expect(
      screen.getByText("Not reported: the engine does not report this channel's destinations."),
    ).toBeTruthy();
    expect(document.body.textContent).not.toMatch(/undefined|NaN|\[object/);
  });

  it("says a channel the engine build does not run is stopped, calmly, and still shows what it reports", async () => {
    const { channel: live, destinations } = realMessageChannel();
    const { container } = renderChannel(channelDetail(live, destinations), {}, live);
    expect(await screen.findByRole("heading", { level: 1, name: live.label })).toBeTruthy();
    expect(screen.getAllByText("Stopped: not implemented").length).toBeGreaterThan(0);
    expect(screen.getByText(/This engine build does not run message channels yet/)).toBeTruthy();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByText("Last error")).toBeNull();
    expect(screen.queryByText(/Retrying|No retry is scheduled/)).toBeNull();
    // What the engine does report is still there.
    expect(screen.getByRole("region", { name: "Destination CSV" })).toBeTruthy();
    for (const session of screen.getAllByText("Session")) {
      expect(session.nextElementSibling?.textContent).toBe("Closed");
    }
    expect(screen.queryByRole("button", { name: "Pause" })).toBeNull();
    await expectNoA11yViolations(container);
  });
});

describe("ChannelPage, commands", () => {
  it("pauses only after the operator confirms what will happen, then reports the outcome", async () => {
    const pauseChannel = vi.fn(async () => undefined);
    const { user } = renderChannel(channelDetail(channel({ id: "ch-alpha" })), { pauseChannel });

    await user.click(await screen.findByRole("button", { name: "Pause" }));
    const dialog = await screen.findByRole("alertdialog", { name: "Pause Alpha bridge?" });
    expect(dialog.textContent).toContain("The session closes");
    expect(dialog.textContent).toContain("sees the connection drop");

    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(pauseChannel).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Pause" }));
    await user.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", { name: "Pause channel" }),
    );
    await waitFor(() => expect(pauseChannel).toHaveBeenCalledWith("ch-alpha"));
    expect(await screen.findByText("Channel paused. No retry is scheduled.")).toBeTruthy();
  });

  it("tells a message channel's pause consequence: the sender is refused, accepted messages stay queued", async () => {
    const live = messageChannel();
    const { user } = renderChannel(channelDetail(live), {}, live);
    await user.click(await screen.findByRole("button", { name: "Pause" }));
    const dialog = await screen.findByRole("alertdialog", { name: "Pause Results to LIS?" });
    expect(dialog.textContent).toContain("sender will see its connection refused");
    expect(dialog.textContent).toContain("Messages already accepted stay queued");
  });

  it("restarts after a confirmation that says the session closes and the counters reset", async () => {
    const restartChannel = vi.fn(async () => undefined);
    const { user } = renderChannel(channelDetail(channel({ id: "ch-alpha" })), { restartChannel });
    await user.click(await screen.findByRole("button", { name: "Restart" }));
    const dialog = await screen.findByRole("alertdialog", { name: "Restart Alpha bridge?" });
    expect(dialog.textContent).toContain("The session closes");
    expect(dialog.textContent).toContain("counters of this instance reset");
    await user.click(within(dialog).getByRole("button", { name: "Restart channel" }));
    await waitFor(() => expect(restartChannel).toHaveBeenCalledWith("ch-alpha"));
    expect(await screen.findByText("Restarting from configuration version 7.")).toBeTruthy();
  });

  it("resumes at once, without a confirmation, and says which version it starts from", async () => {
    const resumeChannel = vi.fn(async () => undefined);
    const paused = channel({ id: "ch-alpha", state: "stopped", stopReason: "paused" });
    const { user } = renderChannel(channelDetail(paused), { resumeChannel }, paused);
    await user.click(await screen.findByRole("button", { name: "Resume" }));
    expect(screen.queryByRole("alertdialog")).toBeNull();
    await waitFor(() => expect(resumeChannel).toHaveBeenCalledWith("ch-alpha"));
    expect(await screen.findByText("Starting from configuration version 7.")).toBeTruthy();
  });

  it("shows a refusal as the engine words it, not as a generic error", async () => {
    const restartChannel = vi.fn(async () => {
      throw engineError("conflict", "Channel is paused: use Resume");
    });
    const { user } = renderChannel(channelDetail(failingChannel()), { restartChannel });
    await user.click(await screen.findByRole("button", { name: "Restart" }));
    await user.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Restart channel",
      }),
    );
    expect(await screen.findByText("Could not restart Zulu bridge")).toBeTruthy();
    expect(screen.getByText("Channel is paused: use Resume")).toBeTruthy();
  });

  it("shows a rejected resume with the engine's text", async () => {
    const resumeChannel = vi.fn(async () => {
      throw engineError(
        "conflict",
        "Disabled in configuration: enable it in the configuration editor",
      );
    });
    const paused = channel({ id: "ch-alpha", state: "stopped", stopReason: "paused" });
    const { user } = renderChannel(channelDetail(paused), { resumeChannel }, paused);
    await user.click(await screen.findByRole("button", { name: "Resume" }));
    expect(
      await screen.findByText("Disabled in configuration: enable it in the configuration editor"),
    ).toBeTruthy();
  });
});
