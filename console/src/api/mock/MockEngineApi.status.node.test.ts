import { describe, expect, it, vi } from "vitest";
import type { ChannelSnapshot, Snapshot } from "../types";
import { mockApiFactory } from "./mockTestSupport";

const create = mockApiFactory({ messageCount: 500, seed: 7 });

describe("node and destinations", () => {
  it("reports totals that match the channels and open alerts", async () => {
    const snapshot = await create().getSnapshot();
    const totals = snapshot.node.totals;
    expect(totals).not.toBeNull();
    const sum = (pick: (c: ChannelSnapshot) => number) =>
      snapshot.channels.reduce((total, c) => total + pick(c), 0);
    expect(totals?.deliveriesUnresolved).toBe(sum((c) => c.queue?.needsAttention ?? 0));
    expect(totals?.deliveriesQueued).toBe(sum((c) => c.queue?.depth ?? 0));
    expect(totals?.alertsWarning).toBe(
      snapshot.openAlerts.filter((a) => a.severity === "warning").length,
    );
    expect(totals?.alertsCritical).toBe(
      snapshot.openAlerts.filter((a) => a.severity === "critical").length,
    );
    expect(snapshot.node.pendingConfigVersion).toBeNull();
    expect(snapshot.node.store?.messageCount).toBe(500);
  });

  it("publishes the pending version while an apply is in flight", async () => {
    vi.useFakeTimers();
    const api = create();
    const received: Snapshot[] = [];
    api.subscribeSnapshots((s) => received.push(s));
    await Promise.resolve();
    const config = await api.getConfig();
    const result = await api.applyConfig({ ...config }, "no change");
    const pending = received.filter((s) => s.node.pendingConfigVersion !== null);
    expect(pending).toHaveLength(1);
    expect(pending[0]?.node.pendingConfigVersion).toBe(result.version);
    expect(pending[0]?.node.configVersion).toBe(config.version);
    const last = received[received.length - 1];
    expect(last?.node.configVersion).toBe(result.version);
    expect(last?.node.pendingConfigVersion).toBeNull();
  });

  it("reports each destination with its label, oldest queued time, unresolved counts and counters", async () => {
    vi.useFakeTimers();
    const api = create();
    const detail = await api.getChannel("ch-msg-sta");
    if (detail.config.kind !== "message") throw new Error("not a message channel");
    const labels = detail.config.destinations.map((d) => d.label);
    expect(detail.destinations.map((d) => d.label)).toEqual(labels);
    const now = Date.now();
    for (const destination of detail.destinations) {
      const waiting = destination.queued + destination.sending;
      expect(destination.sending).toBeLessThanOrEqual(1);
      expect(destination.oldestQueuedAt === null).toBe(waiting === 0);
      if (destination.oldestQueuedAt !== null) {
        expect(Date.parse(destination.oldestQueuedAt)).toBeLessThan(now);
      }
      expect(destination.counters.failed).toBe(destination.unresolved.failed);
      expect(destination.counters.rejected).toBe(destination.unresolved.rejected);
      expect(destination.counters.acknowledged).toBeGreaterThanOrEqual(0);
    }
    const oldest = detail.destinations
      .map((d) => d.oldestQueuedAt)
      .filter((at) => at !== null)
      .sort()[0];
    // The channel's oldest is no younger than any destination's.
    expect((detail.snapshot.queue?.oldestQueuedAt ?? "") <= (oldest ?? "")).toBe(true);
  });

  it("counts the failed deliveries whose mapping failed, within the failed ones", async () => {
    vi.useFakeTimers();
    const api = create();
    const detail = await api.getChannel("ch-msg-sta");
    const withMapping = detail.destinations.filter((d) => d.transformErrors > 0);
    expect(withMapping.length).toBeGreaterThan(0);
    for (const destination of detail.destinations) {
      expect(destination.transformErrors).toBeLessThanOrEqual(destination.unresolved.failed);
    }
  });

  it("schedules destination attempts a few seconds ahead while deliveries queue", async () => {
    vi.useFakeTimers();
    const api = create();
    const detail = await api.getChannel("ch-msg-sta");
    const now = Date.now();
    for (const destination of detail.destinations) {
      expect(destination.enabled).toBe(true);
      expect(destination.lastAttemptAt).toBeTruthy();
      if (destination.queued > 0) {
        const next = Date.parse(destination.nextAttemptAt ?? "");
        expect(next).toBeGreaterThan(now);
        expect(next - now).toBeLessThanOrEqual(21_000);
      } else {
        expect(destination.nextAttemptAt).toBeNull();
      }
    }
    expect(detail.destinations.some((d) => d.nextAttemptAt !== null)).toBe(true);
    await api.pauseChannel("ch-msg-sta");
    const paused = await api.getChannel("ch-msg-sta");
    expect(paused.destinations.every((d) => d.nextAttemptAt === null)).toBe(true);
  });
});

describe("alerts", () => {
  it("counts occurrences of a persisting condition over time", async () => {
    vi.useFakeTimers();
    const api = create();
    const first = (await api.getSnapshot()).openAlerts;
    expect(first.every((a) => Number.isInteger(a.occurrences) && (a.occurrences ?? 0) >= 1)).toBe(
      true,
    );
    const target = first.find((a) => a.kind === "endpoint_down");
    api.subscribeSnapshots(() => {});
    vi.advanceTimersByTime(130_000);
    const later = (await api.getSnapshot()).openAlerts.find((a) => a.id === target?.id);
    expect(later?.occurrences ?? 0).toBeGreaterThanOrEqual((target?.occurrences ?? 0) + 4);
    const cleared = await api.listAlerts("cleared");
    expect(cleared.every((a) => (a.occurrences ?? 0) >= 1)).toBe(true);
  });
});
