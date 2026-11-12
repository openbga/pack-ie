import { act, screen, within } from "@testing-library/react";
import type { ReactElement } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { renderRoutes } from "@/lib/test/router";
import { destination, messageChannel, side, sourceStatus } from "../shared-operate/testFixtures";
import { DestinationsSection, SourceSection } from "./MessageSections";

const NOW = new Date("2026-11-06T12:00:00.000Z");
const at = (secondsAgo: number) => new Date(NOW.getTime() - secondsAgo * 1000).toISOString();
const inSeconds = (seconds: number) => new Date(NOW.getTime() + seconds * 1000).toISOString();

beforeEach(() => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  vi.setSystemTime(NOW);
});
afterEach(() => vi.useRealTimers());

const renderWithProviders = (element: ReactElement) => renderRoutes([{ path: "/", element }], "/");

const text = (element: HTMLElement) => element.textContent ?? "";

describe("DestinationsSection", () => {
  const live = messageChannel({
    id: "ch-hl7",
    queue: { depth: 7, needsAttention: 3, oldestQueuedAt: at(90) },
  });
  const lis = destination({
    destinationId: "dst-1",
    label: "LIS results",
    endpoint: { endpointId: "ep-lis", label: "LIS listener", state: "retrying" },
    queued: 4,
    sending: 1,
    oldestQueuedAt: at(95),
    unresolved: { rejected: 0, failed: 2, timed_out: 1, outcome_unknown: 0 },
    nextAttemptAt: inSeconds(9),
    lastAttemptAt: at(20),
    lastAcknowledgedAt: at(90),
    lastError: {
      at: at(20),
      summary: "LIS did not acknowledge in time",
      nextAction: "Check the LIS.",
    },
  });
  const archive = destination({
    destinationId: "dst-2",
    label: "Archive",
    endpoint: { endpointId: "ep-arch", label: "Archive folder", state: "disabled" },
    protocol: "delimited_file",
    enabled: false,
    queued: 3,
    sending: 0,
    oldestQueuedAt: at(3600),
    unresolved: { rejected: 0, failed: 0, timed_out: 0, outcome_unknown: 0 },
    nextAttemptAt: null,
    lastAttemptAt: null,
    lastAcknowledgedAt: null,
    lastError: null,
  });

  it("shows every destination with its queue, oldest queued, attempts and last error", async () => {
    const { container } = renderWithProviders(
      <DestinationsSection channel={live} destinations={[lis, archive]} />,
    );
    const first = screen.getByRole("region", { name: "Destination LIS results" });
    expect(text(first)).toContain("LIS listener Retrying");
    expect(text(first)).toContain("ProtocolHL7 v2");
    expect(text(first)).toContain("EnabledYes");
    expect(text(first)).toContain("Queued4");
    expect(text(first)).toContain("Sending1");
    expect(text(first)).toContain("3 need attention");
    expect(text(first)).toContain("2 failed (1 in the mapping) · 1 timed out");
    expect(text(first)).toContain("1 of them: the mapping failed — fix the mapping, then resend.");
    expect(text(first)).toContain("Oldest queued1 min ago");
    expect(text(first)).toContain("Next attemptin 9 s");
    expect(text(first)).toContain("Last attempt20 s ago");
    expect(text(first)).toContain("Last acknowledged1 min ago");
    expect(text(first)).toContain("Acknowledged since start40");
    expect(text(first)).toContain("LIS did not acknowledge in time");
    expect(text(first)).toContain("Next step: Check the LIS.");

    const second = screen.getByRole("region", { name: "Destination Archive" });
    expect(text(second)).toContain("Disabled");
    expect(text(second)).toContain(
      "3 queued deliveries wait until it is enabled, and no new messages are queued for it.",
    );
    expect(text(second)).toContain("UnresolvedNone");
    expect(text(second)).toContain("Oldest queued1 h ago");
    expect(text(second)).toContain("Next attemptNone: the destination is disabled");
    expect(text(second)).toContain("Last attemptNone yet");
    expect(text(second)).toContain("Last acknowledgedNever");
    await expectNoA11yViolations(container);
  });

  it("counts the next attempt down every second", () => {
    renderWithProviders(<DestinationsSection channel={live} destinations={[lis]} />);
    const region = screen.getByRole("region", { name: "Destination LIS results" });
    expect(within(region).getByText("in 9 s")).toBeTruthy();
    act(() => vi.advanceTimersByTime(3000));
    expect(within(region).getByText("in 6 s")).toBeTruthy();
    act(() => vi.advanceTimersByTime(6000));
    expect(within(region).getByText("now")).toBeTruthy();
  });

  it("totals the channel and links to its messages and its unresolved deliveries", () => {
    renderWithProviders(<DestinationsSection channel={live} destinations={[lis, archive]} />);
    expect(screen.getByText("Not yet delivered").nextElementSibling?.textContent).toBe("7");
    expect(screen.getByText("Need attention").nextElementSibling?.textContent).toBe(
      "3 need attention · 2 failed in the mapping, never sent",
    );
    // The first "Oldest queued" is the channel's; each destination has its own below.
    expect(screen.getAllByText("Oldest queued")[0]?.nextElementSibling?.textContent).toBe(
      "1 min ago",
    );
    expect(screen.getByRole("link", { name: "View messages" }).getAttribute("href")).toBe(
      "/messages?channel=ch-hl7",
    );
    expect(
      screen.getByRole("link", { name: "View unresolved deliveries (3)" }).getAttribute("href"),
    ).toBe("/messages?channel=ch-hl7&quick=attention&range=any");
  });

  it("encodes the channel id in the links and still offers the unresolved link with none", () => {
    const quiet = messageChannel({
      id: "ch a/b",
      queue: { depth: 0, needsAttention: 0, oldestQueuedAt: null },
    });
    renderWithProviders(<DestinationsSection channel={quiet} destinations={[]} />);
    expect(screen.getByRole("link", { name: "View messages" }).getAttribute("href")).toBe(
      "/messages?channel=ch%20a%2Fb",
    );
    expect(
      screen.getByRole("link", { name: "View unresolved deliveries" }).getAttribute("href"),
    ).toBe("/messages?channel=ch%20a%2Fb&quick=attention&range=any");
    expect(screen.getByText("Need attention").nextElementSibling?.textContent).toBe("None");
    expect(screen.getByText("Nothing waiting")).toBeTruthy();
    expect(screen.getByText("This channel has no destinations.")).toBeTruthy();
  });

  it("says Not reported, never an invented figure, when the engine reports no queue or source", async () => {
    const unknown = messageChannel({ source: null, queue: null });
    const { container } = renderWithProviders(
      <DestinationsSection channel={unknown} destinations={[]} />,
    );
    expect(screen.getAllByText("Not reported")).toHaveLength(3);
    expect(
      screen.getByText("Not reported: the engine does not report this channel's destinations."),
    ).toBeTruthy();
    await expectNoA11yViolations(container);
  });
});

describe("SourceSection", () => {
  it("shows the endpoint, protocol, session, last received and the five counters", async () => {
    const live = messageChannel({
      source: sourceStatus({ lastReceivedAt: at(15), received: 1200, duplicates: 2 }),
    });
    const { container } = renderWithProviders(<SourceSection channel={live} />);
    const section = screen.getByRole("heading", { name: "Source connector" }).closest("section");
    if (section === null) throw new Error("no section");
    expect(text(section)).toContain("Chemistry source Connected");
    expect(text(section)).toContain("ProtocolHL7 v2");
    expect(text(section)).toContain("SessionReceiving");
    expect(text(section)).toContain("Active peer10.0.4.20:50321");
    expect(text(section)).toContain("Last received15 s ago");
    for (const [label, value] of [
      ["Received", "1,200"],
      ["Duplicates", "2"],
      ["Invalid", "1"],
      ["Filtered", "3"],
      ["Aborted", "0"],
    ] as const) {
      expect(within(section).getByText(label).nextElementSibling?.textContent).toBe(value);
    }
    await expectNoA11yViolations(container);
  });

  it.each([
    ["idle", "Idle"],
    ["receiving", "Receiving"],
    ["waiting_for_peer", "Waiting for peer"],
    ["closed", "Closed"],
  ] as const)("reads the %s session as %s", (session, label) => {
    renderWithProviders(
      <SourceSection channel={messageChannel({ source: sourceStatus({ session }) })} />,
    );
    expect(screen.getByText("Session").nextElementSibling?.textContent).toBe(label);
  });

  it("handles nothing received yet, no peer and a listening endpoint", () => {
    const source = sourceStatus({
      lastReceivedAt: null,
      session: "waiting_for_peer",
      endpoint: side({
        endpointId: "ep-src",
        label: "",
        role: "source",
        state: "listening",
        activePeer: null,
      }),
    });
    renderWithProviders(<SourceSection channel={messageChannel({ source })} />);
    expect(screen.getByText("Nothing received yet")).toBeTruthy();
    expect(screen.getByText("None")).toBeTruthy();
    expect(screen.getByText(/waiting for peer/)).toBeTruthy();
  });

  it("says Not reported when the engine reports no source", () => {
    renderWithProviders(<SourceSection channel={messageChannel({ source: null })} />);
    expect(
      screen.getByText("Not reported: the engine does not report this channel's source connector."),
    ).toBeTruthy();
  });
});
