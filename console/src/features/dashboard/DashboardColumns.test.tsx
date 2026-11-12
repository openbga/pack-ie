import { screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { mockGridLayout } from "@/lib/test/layout";
import {
  ago,
  alert,
  bridgeStatus,
  channel,
  messageChannel,
  side,
  snapshot,
} from "../shared-operate/testFixtures";
import { cellsOf, renderDashboard, rowOf, rows } from "./fixtures";

beforeEach(() => mockGridLayout());

describe("DashboardPage columns", () => {
  it("shows each column's content for a bridge and a message channel", async () => {
    const data = snapshot({
      channels: [
        channel({
          id: "ch-bridge",
          label: "Bridge one",
          bridge: bridgeStatus({
            bytesAToB: 2048,
            bytesBToA: 512,
            endpointA: side({ lastActivityAt: ago(8) }),
            endpointB: side({
              endpointId: "ep-lis",
              label: "LIS listener",
              state: "listening",
              activePeer: null,
              lastActivityAt: ago(120),
            }),
            sessionState: "waiting_for_peer",
            overflowCount: 2,
          }),
          buffers: [
            {
              direction: "a_to_b",
              usedBytes: 32_768,
              capacityBytes: 65_536,
              segments: 4,
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
          counters: { bytesIn: 0, bytesOut: 0, messagesIn: 0, messagesOut: 0, retries: 3 },
          nextRetryAt: new Date(Date.now() + 5000).toISOString(),
          state: "retrying",
          lastError: { at: ago(4), summary: "Peer closed the connection", nextAction: "Wait" },
        }),
        messageChannel({
          id: "ch-msg",
          label: "Message one",
          queue: { depth: 4, needsAttention: 2, oldestQueuedAt: null },
          lastActivityAt: ago(500),
          destinations: [
            {
              destinationId: "dst-1",
              label: "LIS",
              enabled: true,
              acknowledged: 11,
              lastAcknowledgedAt: ago(40),
            },
          ],
        }),
      ],
    });
    renderDashboard(async () => data);
    await screen.findByRole("grid", { name: "Channels" });

    const bridge = cellsOf(rowOf("Bridge one")).map((cell) => cell.textContent);
    expect(bridge[0]).toBe("Bridge one Bridge");
    expect(bridge[1]).toMatch(/^Retrying in [1-5] s/);
    expect(bridge[2]).toContain("A Analyzer COM3 Connected");
    expect(bridge[2]).toContain("B LIS listener Listening, no peer");
    expect(bridge[3]).toBe("A → B 2.0 KBB → A 512 B");
    expect(bridge[4]).toMatch(/^A \d+ s agoB 2 min ago$/);
    expect(bridge[5]).toBe("50% fullOverflow");
    expect(bridge[6]).toBe("Waiting for peer");
    expect(bridge[7]).toMatch(/^3 · in [1-5] s$/);
    expect(bridge[8]).toBe("Peer closed the connection");

    const message = cellsOf(rowOf("Message one")).map((cell) => cell.textContent);
    expect(message[2]).toContain("Source Chemistry source Connected");
    expect(message[2]).toContain("To LIS listener Listening");
    expect(message[3]).toBe("Received 120Acknowledged 11");
    expect(message[4]).toMatch(/^Received \d+ s agoAcknowledged \d+ s ago$/);
    expect(message[5]).toBe("4 queued2 unresolved");
    expect(message[6]).toBe("Receiving");
    expect(message[7]).toBe("None");
    expect(message[8]).toBe("None");
  });

  it("shows each destination's acknowledgements, labelled, when a message channel has several", async () => {
    const destination = (label: string, acknowledged: number, at: string | null) => ({
      destinationId: label,
      label,
      enabled: true,
      acknowledged,
      lastAcknowledgedAt: at,
    });
    const data = snapshot({
      channels: [
        messageChannel({
          id: "ch-two",
          label: "Two destinations",
          destinations: [destination("LIS", 2136, ago(5)), destination("Backup", 1068, null)],
        }),
        messageChannel({ id: "ch-old", label: "Old engine" }),
      ],
    });
    renderDashboard(async () => data);
    await screen.findByRole("grid", { name: "Channels" });
    const two = cellsOf(rowOf("Two destinations"));
    expect(two[3]?.textContent).toBe("Received 120Acknowledged 2,136 (LIS) · 1,068 (Backup)");
    expect(two[3]?.querySelector("[title]")?.getAttribute("title")).toBe(
      "Acknowledged 2,136 (LIS) · 1,068 (Backup)",
    );
    expect(two[4]?.textContent).toMatch(/Acknowledged \d+ s ago \(LIS\) · never \(Backup\)$/);
    expect(cellsOf(rowOf("Old engine"))[3]?.textContent).toBe(
      "Received 120Acknowledged not reported",
    );
  });

  it("marks the sides of a receiver-then-sender bridge with direction arrows, and a down side with a cross", async () => {
    const data = snapshot({
      channels: [
        channel({
          bridge: bridgeStatus({
            connectStrategy: "receiver_then_sender",
            receiver: "a",
            endpointB: side({ label: "LIS client", state: "disconnected" }),
          }),
        }),
      ],
    });
    renderDashboard(async () => data);
    await screen.findByRole("grid", { name: "Channels" });
    const endpoints = cellsOf(rows()[0] as HTMLElement)[2] as HTMLElement;
    expect(within(endpoints).getByRole("img", { name: "Receives" })).toBeTruthy();
    expect(within(endpoints).getByRole("img", { name: "Sends" })).toBeTruthy();
    expect(endpoints.textContent).toContain("LIS client Disconnected");
  });

  it("shows danger emphasis, in words too, for a channel with a critical alert whatever its state", async () => {
    const data = snapshot({
      channels: [
        channel({ id: "ch-quiet", label: "Quiet bridge" }),
        channel({
          id: "ch-paused",
          label: "Paused bridge",
          state: "stopped",
          stopReason: "paused",
        }),
      ],
      openAlerts: [alert({ channelId: "ch-paused", severity: "critical", endpointId: null })],
    });
    renderDashboard(async () => data);
    await screen.findByRole("grid", { name: "Channels" });
    const [first, second] = rows();
    expect(first?.textContent).toContain("Paused bridge");
    expect(first?.textContent).toContain("Critical alert");
    expect(first?.querySelector("[class*=critical]")).not.toBeNull();
    expect(second?.querySelector("[class*=critical]")).toBeNull();
    expect(second?.textContent).not.toContain("Critical alert");
  });

  it("never styles a paused, disabled or not-implemented channel as a failure", async () => {
    const data = snapshot({
      channels: [
        channel({ id: "p", label: "Paused", state: "stopped", stopReason: "paused" }),
        channel({ id: "d", label: "Disabled", state: "stopped", stopReason: "disabled" }),
        channel({ id: "n", label: "NotImpl", state: "stopped", stopReason: "not_implemented" }),
      ],
    });
    renderDashboard(async () => data);
    await screen.findByRole("grid", { name: "Channels" });
    for (const [name, text] of [
      ["Paused", "Paused by operator"],
      ["Disabled", "Disabled"],
      ["NotImpl", "Stopped: not implemented"],
    ] as const) {
      const chip = within(cellsOf(rowOf(name))[1] as HTMLElement).getByText(text);
      expect(chip.closest("[data-tone]")?.getAttribute("data-tone")).toBe("neutral");
    }
  });
});
