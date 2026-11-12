// The read-only tests of `live.test.tsx` (its `describe` registers them): logs, alerts, secrets
// and settings in the console's model, and every page rendered from the engine's data.

import { screen } from "@testing-library/react";
import type { ReactElement } from "react";
import { expect, it } from "vitest";
import { AlertsPage } from "@/features/alerts/AlertsPage";
import { ChannelPage } from "@/features/channels/ChannelPage";
import { ChannelsPage } from "@/features/channels/ChannelsPage";
import { ChannelEditPage } from "@/features/config/ChannelEditPage";
import { EndpointEditPage } from "@/features/config/EndpointEditPage";
import { DashboardPage } from "@/features/dashboard/DashboardPage";
import { EndpointPage } from "@/features/endpoints/EndpointPage";
import { EndpointsPage } from "@/features/endpoints/EndpointsPage";
import { LogsPage } from "@/features/logs/LogsPage";
import { MessagesPage } from "@/features/messages/MessagesPage";
import { SettingsPage } from "@/features/settings/SettingsPage";
import { renderRoutes } from "@/lib/test/router";
import { connect, target } from "./liveHarness";

export function readTests() {
  it("pages the logs back with before_seq and reads alerts, secrets and settings", async () => {
    const { api, close } = await connect();
    try {
      const first = await api.queryLogs({}, { before: null, limit: 25 });
      expect(first.events.length).toBeGreaterThan(1);
      const seqs = first.events.map((e) => e.seq);
      expect(seqs).toEqual([...seqs].sort((a, b) => b - a));
      expect(first.olderCursor).toBe(seqs.at(-1));
      if (first.olderCursor !== null) {
        const second = await api.queryLogs({}, { before: first.olderCursor, limit: 25 });
        expect(second.events.length).toBeGreaterThan(0);
        expect(Math.max(...second.events.map((e) => e.seq))).toBeLessThan(first.olderCursor);
        expect(second.events.map((e) => e.seq)).toEqual(
          [...second.events.map((e) => e.seq)].sort((a, b) => b - a),
        );
      }
      const forChannel = await api.queryLogs({ channelId: target.id }, { before: null, limit: 10 });
      expect(forChannel.events.every((e) => e.channelId === target.id)).toBe(true);

      const all = await api.listAlerts("all");
      const open = await api.listAlerts("open");
      const cleared = await api.listAlerts("cleared");
      expect(all).toHaveLength(open.length + cleared.length);
      expect(open.every((a) => a.state === "open")).toBe(true);
      expect(cleared.every((a) => a.state === "cleared")).toBe(true);

      const secrets = await api.listSecrets();
      expect(Array.isArray(secrets)).toBe(true);
      for (const secret of secrets) expect(typeof secret.hasValue).toBe("boolean");

      // `read_bootstrap` is not part of the harness: what the engine alone says is shown.
      const settings = await api.getEngineSettings();
      expect(["console", "service"]).toContain(settings.hosting);
      expect(settings).toMatchObject({ stateDir: null, logRetentionDays: null });
      expect(settings.nodeId.length).toBeGreaterThan(0);
      expect((await api.getEngineInfo()).protocolVersion).toBeGreaterThan(0);
    } finally {
      close();
    }
  });

  it("renders every page from the engine's data without an error", async () => {
    const { api, close } = await connect();
    try {
      const snapshot = await api.getSnapshot();
      const channel = snapshot.channels[0]?.id ?? "";
      const endpoint = snapshot.endpoints[0]?.id ?? "";
      const pages: [string, string, () => ReactElement][] = [
        ["/dashboard", "dashboard", () => <DashboardPage />],
        ["/channels", "channels", () => <ChannelsPage />],
        ["/channels/:id", `/channels/${channel}`, () => <ChannelPage />],
        ["/channels/:id/edit", `/channels/${channel}/edit`, () => <ChannelEditPage />],
        ["/channels/new", "/channels/new", () => <ChannelEditPage />],
        ["/endpoints", "/endpoints", () => <EndpointsPage />],
        ["/endpoints/:id", `/endpoints/${endpoint}`, () => <EndpointPage />],
        ["/endpoints/:id/edit", `/endpoints/${endpoint}/edit`, () => <EndpointEditPage />],
        ["/messages", "/messages", () => <MessagesPage />],
        ["/logs", "/logs", () => <LogsPage />],
        ["/alerts", "/alerts", () => <AlertsPage />],
        ["/settings/:tab", "/settings/engine", () => <SettingsPage />],
        ["/settings/:tab", "/settings/versions", () => <SettingsPage />],
        ["/settings/:tab", "/settings/secrets", () => <SettingsPage />],
      ];
      const report: string[] = [];
      for (const [pattern, path, page] of pages) {
        const view = renderRoutes(
          [{ path: pattern, element: page() }],
          path.startsWith("/") ? path : `/${path}`,
          { api },
        );
        await screen.findByRole("heading", { level: 1 }, { timeout: 8000 });
        await new Promise((resolve) => setTimeout(resolve, 600));
        // A page may warn about the engine's state (an endpoint not connected); what must not
        // appear is a failure to read or show the engine's data.
        const alerts = screen
          .queryAllByRole("alert")
          .map((a) => a.textContent ?? "")
          .filter((text) => /could not|did not|not answering/i.test(text));
        report.push(`${path}: ${alerts.length === 0 ? "ok" : `ALERT ${alerts.join(" | ")}`}`);
        view.unmount();
      }
      expect(report.filter((line) => line.includes("ALERT"))).toEqual([]);
    } finally {
      close();
    }
  }, 120_000);
}
