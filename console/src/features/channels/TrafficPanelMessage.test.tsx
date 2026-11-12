import { screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import type { TrafficSegment } from "@/api/types";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { mockGridLayout } from "@/lib/test/layout";
import {
  AT,
  copiedText,
  items,
  refuseSubscription,
  segment,
  setup,
  start,
  stubClipboard,
} from "./trafficTestFixtures";

beforeEach(() => mockGridLayout());

const DESTINATIONS = [
  { destinationId: "dst-lis", label: "LIS results" },
  { destinationId: "dst-archive", label: "Archive" },
];

/** A message-channel segment: the pane, which way it went, and the destination it is tagged with. */
function tagged(
  seq: number,
  direction: TrafficSegment["direction"],
  role: NonNullable<TrafficSegment["role"]>,
  destinationId: string | null,
  ...values: number[]
): TrafficSegment {
  const flow = role === "received" || role === "reply" ? "inbound" : "outbound";
  return { ...segment(seq, direction, ...values), flow, role, destinationId };
}

describe("TrafficPanel, message channel", () => {
  const setupMessage = () => setup({ kind: "message", destinations: DESTINATIONS });

  it("shows the Source and Destinations panes, each segment with an arrow and a word for its direction", async () => {
    const { user, push, container } = setupMessage();
    await start(user);
    push([
      tagged(1, "a_to_b", "received", null, 0x41),
      tagged(2, "a_to_b", "response", null, 0x06),
      tagged(3, "b_to_a", "sent", "dst-lis", 0x42),
      tagged(4, "b_to_a", "reply", "dst-lis", 0x06),
    ]);
    const source = items("Source");
    expect(source).toHaveLength(2);
    expect(source[0]?.textContent).toContain("← Received");
    expect(source[1]?.textContent).toContain("→ Response");
    const destinations = items("Destinations");
    expect(destinations[0]?.textContent).toContain("→ Sent");
    expect(destinations[1]?.textContent).toContain("← Reply");
    expect(source[0]?.querySelector("time")?.getAttribute("datetime")).toBe(AT);
    expect(source[1]?.textContent).toContain("<ACK>");
    expect(screen.getByRole("heading", { name: "Source" })).toBeTruthy();
    expect(screen.getByRole("heading", { name: "Destinations" })).toBeTruthy();
    expect(screen.getByText(/← came in to the engine, → went out of it/)).toBeTruthy();
    await expectNoA11yViolations(container);
  });

  it("reads the direction from the pane when a segment does not say", async () => {
    const { user, push } = setupMessage();
    await start(user);
    push([segment(1, "a_to_b", 0x41), segment(2, "b_to_a", 0x42)]);
    expect(items("Source")[0]?.textContent).toContain("← Received");
    expect(items("Destinations")[0]?.textContent).toContain("→ Sent");
  });

  it("uses the role, not the pane or the direction, for the arrow and the word", async () => {
    const { user, push } = setupMessage();
    await start(user);
    push([{ ...tagged(1, "b_to_a", "reply", "dst-lis", 0x06), flow: "outbound" }]);
    expect(items("Destinations")[0]?.textContent).toContain("← Reply");
  });

  it("tags destination segments with the destination's label", async () => {
    const { user, push } = setupMessage();
    await start(user);
    push([
      tagged(1, "b_to_a", "sent", "dst-lis", 0x41),
      tagged(2, "b_to_a", "sent", "dst-archive", 0x42),
      tagged(3, "b_to_a", "reply", "dst-gone", 0x43),
    ]);
    const rows = items("Destinations");
    expect(rows[0]?.textContent).toContain("LIS results");
    expect(rows[1]?.textContent).toContain("Archive");
    expect(rows[2]?.textContent).toContain("dst-gone");
    expect(screen.queryByText(/did not tag these Destinations segments/)).toBeNull();
    expect(items("Source")).toHaveLength(0);
  });

  it("shows untagged destination segments with a note, and drops the note once tags appear", async () => {
    const { user, push } = setupMessage();
    await start(user);
    expect(screen.queryByText(/did not tag these Destinations segments/)).toBeNull();
    push([tagged(1, "b_to_a", "sent", null, 0x41), segment(2, "b_to_a", 0x42)]);
    expect(screen.getByText(/did not tag these Destinations segments/)).toBeTruthy();
    expect(items("Destinations")).toHaveLength(2);
    expect(items("Destinations")[0]?.textContent).not.toContain("LIS results");

    push([tagged(3, "b_to_a", "sent", "dst-lis", 0x43)]);
    expect(screen.queryByText(/did not tag these Destinations segments/)).toBeNull();
    expect(items("Destinations")[2]?.textContent).toContain("LIS results");
  });

  it("does not note untagged segments of the Source pane or of a bridge", async () => {
    const message = setupMessage();
    await start(message.user);
    message.push([tagged(1, "a_to_b", "received", null, 0x41)]);
    expect(screen.queryByText(/did not tag these Destinations segments/)).toBeNull();
    message.unmount();

    const bridge = setup();
    await start(bridge.user);
    bridge.push([segment(1, "b_to_a", 0x41)]);
    expect(screen.queryByText(/did not tag these Destinations segments/)).toBeNull();
    expect(items("B → A")[0]?.textContent).not.toMatch(/Received|Sent|Response/);
  });

  it("freezes both panes while capture goes on, then shows the newest", async () => {
    const { user, push } = setupMessage();
    await start(user);
    push([tagged(1, "a_to_b", "received", null, 0x41)]);
    await user.click(screen.getByRole("button", { name: "Freeze" }));
    push([
      tagged(2, "a_to_b", "response", null, 0x06),
      tagged(3, "b_to_a", "sent", "dst-lis", 0x42),
    ]);
    expect(items("Source")).toHaveLength(1);
    expect(items("Destinations")).toHaveLength(0);
    expect(screen.getByText("Display frozen; 2 new segments captured.")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Unfreeze" }));
    expect(items("Source")).toHaveLength(2);
    expect(items("Destinations")).toHaveLength(1);
  });

  it("keeps the newest 1000 segments in each pane", async () => {
    const { user, push } = setupMessage();
    await start(user);
    push([
      ...Array.from({ length: 1100 }, (_, i) => tagged(i + 1, "a_to_b", "received", null, 0x41)),
      ...Array.from({ length: 1005 }, (_, i) =>
        tagged(2000 + i, "b_to_a", "sent", "dst-lis", 0x42),
      ),
    ]);
    expect(items("Source")).toHaveLength(1000);
    expect(items("Destinations")).toHaveLength(1000);
    expect(screen.getByText("Newest 1,000 of 1,100 segments")).toBeTruthy();
    expect(screen.getByText("Newest 1,000 of 1,005 segments")).toBeTruthy();
  });

  it("shows the hex of a segment on request, and Clear empties both panes", async () => {
    const { user, push } = setupMessage();
    await start(user);
    push([tagged(1, "a_to_b", "received", null, 0x02, 0x41, 0x03)]);
    expect(items("Source")[0]?.textContent).toContain("<STX>A<ETX>");
    await user.click(screen.getByRole("checkbox", { name: "Show hex" }));
    expect(items("Source")[0]?.textContent).toContain("02 41 03");
    await user.click(screen.getByRole("button", { name: "Clear" }));
    expect(items("Source")).toHaveLength(0);
  });

  it("copies both panes with each segment's direction and destination", async () => {
    const { user, push } = setupMessage();
    const writeText = stubClipboard();
    await start(user);
    push([
      tagged(1, "a_to_b", "received", null, 0x05),
      tagged(2, "b_to_a", "sent", "dst-lis", 0x06),
    ]);
    await user.click(screen.getByRole("button", { name: "Copy" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledOnce());
    const text = copiedText(writeText);
    expect(text).toContain("== Source ==");
    expect(text).toContain("← Received");
    expect(text).toContain("== Destinations ==");
    expect(text).toContain("→ Sent  [LIS results]");
  });

  it("shows the engine's words when another client already views the transcript", async () => {
    const { user } = setup({
      kind: "message",
      destinations: DESTINATIONS,
      subscribe: refuseSubscription,
    });
    await start(user);
    expect(await screen.findByText("Transcript in use by another client")).toBeTruthy();
  });
});
