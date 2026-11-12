import { describe, expect, it } from "vitest";
import {
  ago,
  alert,
  bridgeStatus,
  channel,
  messageChannel,
  side,
  snapshot,
  sourceStatus,
} from "../shared-operate/testFixtures";
import { rankChannels } from "./rank";

/** Quiet running channels with no traffic in the last minute (rank 6 by default). */
const idle = (id: string, label: string) =>
  channel({
    id,
    label,
    lastActivityAt: ago(600),
    bridge: bridgeStatus({
      endpointA: side({ lastActivityAt: ago(600) }),
      endpointB: side({ lastActivityAt: ago(600) }),
    }),
  });

const order = (data: ReturnType<typeof snapshot>) =>
  rankChannels(data).map((row) => row.channel.id);

describe("rankChannels", () => {
  it("orders by the eight ranks of the spec", () => {
    const data = snapshot({
      channels: [
        channel({ id: "paused", label: "A", state: "stopped", stopReason: "paused" }),
        channel({ id: "starting", label: "A", state: "starting" }),
        idle("idle", "A"),
        channel({ id: "active", label: "A", lastActivityAt: ago(5) }),
        messageChannel({
          id: "unresolved",
          label: "A",
          queue: { depth: 1, needsAttention: 2, oldestQueuedAt: null },
        }),
        channel({ id: "degraded", label: "A", state: "degraded" }),
        channel({ id: "retrying", label: "A", state: "retrying" }),
        channel({ id: "critical", label: "A" }),
      ],
      openAlerts: [alert({ channelId: "critical", severity: "critical" })],
    });
    const rows = rankChannels(data);
    expect(rows.map((row) => [row.channel.id, row.rank])).toEqual([
      ["critical", 1],
      ["retrying", 2],
      ["degraded", 3],
      ["unresolved", 4],
      ["active", 5],
      ["idle", 6],
      ["starting", 7],
      ["paused", 8],
    ]);
  });

  it("ranks a running channel with a warning alert in rank 4", () => {
    const data = snapshot({
      channels: [idle("quiet", "A"), idle("warned", "B")],
      openAlerts: [alert({ channelId: "warned", severity: "warning" })],
    });
    expect(rankChannels(data).map((row) => [row.channel.id, row.rank])).toEqual([
      ["warned", 4],
      ["quiet", 6],
    ]);
  });

  it("puts a critical alert first whatever the state, even a paused one", () => {
    const data = snapshot({
      channels: [
        channel({ id: "retry", state: "retrying", label: "A" }),
        channel({ id: "paused", state: "stopped", stopReason: "paused", label: "B" }),
      ],
      openAlerts: [alert({ channelId: "paused", severity: "critical" })],
    });
    expect(order(data)).toEqual(["paused", "retry"]);
  });

  it("ignores cleared alerts and alerts of other channels", () => {
    const data = snapshot({
      channels: [idle("a", "A"), idle("b", "B")],
      openAlerts: [
        alert({ channelId: "a", severity: "critical", state: "cleared", clearedAt: ago(5) }),
        alert({ channelId: "zzz", severity: "critical" }),
      ],
    });
    expect(rankChannels(data).map((row) => row.rank)).toEqual([6, 6]);
  });

  it("inside a rank: a recent error first, then the newest error, then A to Z", () => {
    const failed = (id: string, label: string, secondsAgo: number) =>
      channel({
        id,
        label,
        state: "retrying",
        lastError: { at: ago(secondsAgo), summary: "failed", nextAction: "wait" },
      });
    const data = snapshot({
      channels: [
        channel({ id: "quiet-a", label: "Aardvark", state: "retrying" }),
        failed("old", "Bravo", 3600),
        failed("recent-old", "Charlie", 200),
        failed("recent-new", "Delta", 10),
        channel({ id: "quiet-b", label: "Zebra", state: "retrying" }),
      ],
    });
    // Recent errors (newest first), then the older error, then quiet channels by name.
    expect(order(data)).toEqual(["recent-new", "recent-old", "old", "quiet-a", "quiet-b"]);
  });

  it("never sorts a retrying channel below a degraded one, or a paused one above an active one", () => {
    const data = snapshot({
      channels: [
        channel({
          id: "paused",
          label: "A",
          state: "stopped",
          stopReason: "paused",
          lastError: { at: ago(1), summary: "x", nextAction: "y" },
        }),
        channel({
          id: "degraded",
          label: "B",
          state: "degraded",
          lastError: { at: ago(1), summary: "x", nextAction: "y" },
        }),
        channel({ id: "retrying", label: "Z", state: "retrying" }),
        idle("idle", "C"),
      ],
    });
    expect(order(data)).toEqual(["retrying", "degraded", "idle", "paused"]);
  });

  it("ranks every stopped channel last unless it has a critical alert", () => {
    const data = snapshot({
      channels: [
        channel({ id: "paused", label: "A", state: "stopped", stopReason: "paused" }),
        channel({ id: "fatal", label: "B", state: "stopped", stopReason: "fatal" }),
        channel({ id: "off", label: "C", state: "stopped", stopReason: "disabled" }),
      ],
    });
    expect(order(data)).toEqual(["paused", "fatal", "off"]);
  });

  it("measures traffic against the snapshot's clock, from either side or the source", () => {
    const data = snapshot({
      channels: [
        idle("idle", "A"),
        channel({
          id: "side-b",
          label: "B",
          lastActivityAt: ago(600),
          bridge: bridgeStatus({ endpointB: side({ lastActivityAt: ago(10) }) }),
        }),
        messageChannel({
          id: "msg",
          label: "C",
          lastActivityAt: null,
          source: sourceStatus({ lastReceivedAt: ago(20) }),
        }),
      ],
    });
    expect(rankChannels(data).map((row) => [row.channel.id, row.rank])).toEqual([
      ["side-b", 5],
      ["msg", 5],
      ["idle", 6],
    ]);
  });
});
